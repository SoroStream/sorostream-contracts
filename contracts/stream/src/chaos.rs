//! Deterministic chaos-testing framework for the SoroStream contract (issue #656).
//!
//! # What this module is
//!
//! A *test-only* harness that injects random, reproducible faults into a Soroban
//! test environment and asserts that the protocol **degrades gracefully** instead
//! of panicking, corrupting accounting, or emitting an error code that is not
//! part of the [`StreamError`](crate::errors::StreamError) contract.
//!
//! The three fault families the issue calls for are covered by [`ChaosToken`]
//! (token-contract failures), [`ChaosEnv`] (ledger resets, clock skew, storage
//! loss, budget starvation) and [`ChaosRng`] (the deterministic fault schedule).
//!
//! # Determinism
//!
//! Every fault decision is drawn from [`ChaosRng`], a self-contained xorshift64*
//! generator. A campaign run with seed `S` therefore always injects the exact
//! same faults in the exact same order, so a failure reported in CI can be
//! reproduced locally by re-running with the printed seed. This is a deliberate
//! choice over `proptest`'s shrinking: chaos campaigns are long-running, and a
//! reproducible-but-unshrunk sequence is worth more than a minimal one.
//!
//! # Graceful degradation, concretely
//!
//! "Graceful" is defined as one of three outcomes, and anything else is a
//! finding:
//!
//! | Outcome | Meaning |
//! |---|---|
//! | [`ChaosReport::handled`] | the entry point returned a typed `Err` |
//! | [`ChaosReport::recovered`] | the entry point succeeded despite the fault |
//! | [`ChaosReport::finding`] | panic, undefined error code, or broken invariant |
//!
//! # Usage
//!
//! ```ignore
//! let mut chaos = ChaosEnv::new(ChaosConfig::new(0xC0FFEE));
//! chaos.inject_token_fault(&token, FaultKind::TokenTransferRejects);
//! match client.try_cancel_stream(&id, &sender) {
//!     Ok(Ok(())) => chaos.record_recovered("cancel", None),
//!     Ok(Err(code)) => chaos.record_error("cancel", code),
//!     Err(_) => chaos.record_panic("cancel", "host rejected the invocation"),
//! }
//! chaos.assert_no_findings();
//! ```

extern crate alloc;

use alloc::format;
use alloc::string::String as StdString;
use alloc::vec::Vec as StdVec;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, String, Symbol};

/// Fault modes understood by [`ChaosToken`].
///
/// Stored as a `u32` rather than a `#[contracttype]` enum so the mock token
/// stays within the host's contract-data limits and can be reconfigured from a
/// test without a separate control transaction.
///
/// # Why there is no "fail the next N calls" mode here
///
/// The obvious design — a counter in token storage that `transfer` decrements
/// before returning an error — does not work, and silently does not work: the
/// host rolls back every storage write made by a failing invocation, including
/// the decrement. The counter therefore never reaches zero and the fault never
/// clears, which looks like a contract bug and is not one.
///
/// The countdown consequently lives *outside* the token, in
/// [`ChaosEnv::arm_token_failures`], which re-arms the mode from the host
/// between protocol calls where the arming write is its own successful
/// invocation.
pub mod token_fault {
    /// Transfers succeed normally.
    pub const NONE: u32 = 0;
    /// Every transfer fails with [`ChaosTokenError::TransferFailed`].
    pub const ALWAYS_FAIL: u32 = 1;
    /// Transfers fail with [`ChaosTokenError::InsufficientBalance`].
    pub const INSUFFICIENT: u32 = 2;
    /// Transfers abort the host invocation, modelling a token contract that
    /// panics rather than returning an error.
    pub const PANIC: u32 = 3;
}

/// Errors raised by the injectable mock token.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ChaosTokenError {
    /// Injected transient transfer failure.
    TransferFailed = 1,
    /// The sender's modelled balance is below the requested amount.
    InsufficientBalance = 2,
    /// The token is frozen and refuses all movement.
    Frozen = 3,
}

/// A minimal SEP-41-style token whose failures the test drives directly.
///
/// The stream contract only ever calls [`ChaosToken::symbol`],
/// [`ChaosToken::balance`] and [`ChaosToken::transfer`] on a token address, so
/// this mock implements exactly that surface. Unlike a real SAC it performs no
/// authorization, because the point of the mock is to control the *token* side
/// of the boundary, not the auth side.
#[contract]
pub struct ChaosToken;

const MODE_KEY: &str = "MODE";
const ADMIN_KEY: &str = "ADM";
const SYMBOL: &str = "CHAOS";

#[contractimpl]
impl ChaosToken {
    /// Initializes the mock with a minting administrator.
    pub fn __constructor(env: Env, admin: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &admin);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, MODE_KEY), &token_fault::NONE);
    }

    /// Arms a fault mode until [`ChaosToken::clear_fault`] is called.
    pub fn set_fault(env: Env, mode: u32) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, MODE_KEY), &mode);
    }

    /// Clears any armed fault.
    pub fn clear_fault(env: Env) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, MODE_KEY), &token_fault::NONE);
    }

    /// Mints test supply. Unguarded on purpose — this is a test double.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let current: i128 = env.storage().persistent().get(&to).unwrap_or(0i128);
        env.storage().persistent().set(&to, &(current + amount));
    }

    /// Burns supply, used to drive a sender towards (or past) insolvency.
    pub fn burn(env: Env, from: Address, amount: i128) {
        let current: i128 = env.storage().persistent().get(&from).unwrap_or(0i128);
        env.storage().persistent().set(&from, &(current - amount));
    }

    /// Overwrites a balance outright, used to model a ledger that has lost or
    /// rewritten a trustor's holdings.
    pub fn set_balance(env: Env, who: Address, amount: i128) {
        env.storage().persistent().set(&who, &amount);
    }

    /// The token symbol, probed by the stream contract during validation.
    pub fn symbol(env: Env) -> String {
        String::from_str(&env, SYMBOL)
    }

    /// Current modelled balance.
    pub fn balance(env: Env, who: Address) -> i128 {
        env.storage().persistent().get(&who).unwrap_or(0i128)
    }

    /// Moves `amount` from `from` to `to`, subject to the armed fault mode.
    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), ChaosTokenError> {
        let mode: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, MODE_KEY))
            .unwrap_or(token_fault::NONE);

        if mode == token_fault::PANIC {
            // A token contract that aborts rather than returning a typed error.
            // The stream contract has no way to recover from this, so tests
            // assert only that the surrounding harness records a finding
            // instead of letting the panic escape the campaign.
            panic!("chaos: token contract aborted mid-transfer");
        }

        if mode == token_fault::ALWAYS_FAIL {
            return Err(ChaosTokenError::TransferFailed);
        }

        if mode == token_fault::INSUFFICIENT {
            return Err(ChaosTokenError::InsufficientBalance);
        }

        let from_balance: i128 = env.storage().persistent().get(&from).unwrap_or(0i128);
        if from_balance < amount {
            return Err(ChaosTokenError::InsufficientBalance);
        }
        let to_balance: i128 = env.storage().persistent().get(&to).unwrap_or(0i128);
        env.storage()
            .persistent()
            .set(&from, &(from_balance - amount));
        env.storage().persistent().set(&to, &(to_balance + amount));
        Ok(())
    }
}

/// Every error code the `StreamError` enum is allowed to surface on-chain.
///
/// A chaos campaign that observes a code outside this set is reporting a real
/// defect: either an entry point returns a host error instead of a declared
/// variant, or a variant was added without updating this table. Keeping the
/// table explicit (rather than deriving it from the enum) is what makes the
/// check meaningful at runtime, where only the `u32` discriminant survives the
/// contract boundary.
pub const DEFINED_ERROR_CODES: &[u32] = &[
    1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 14, 15, 16, 17, 19, 20, 21, 22, 25, 26, 27, 28, 29, 30, 31,
    34, 35, 36, 37, 38, 41, 43, 44, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 57, 58, 59, 60, 61, 64,
    65, 66, 67,
];

/// Human-readable names for the codes in [`DEFINED_ERROR_CODES`].
///
/// Index `i` of this table describes code `DEFINED_ERROR_CODES[i]`.
pub const ERROR_CODE_NAMES: &[&str] = &[
    "StreamNotFound",
    "NotRecipient",
    "NotSender",
    "StreamNotActive",
    "ZeroAmount",
    "InvalidDuration",
    "InvalidCliff",
    "AlreadyInitialized",
    "NotInitialized",
    "DuplicateStream",
    "InvalidStartTime",
    "ContractPaused",
    "Overflow",
    "ZeroFlowRate",
    "BatchLengthMismatch",
    "StreamLocked",
    "NotAuthorized",
    "StreamNotPaused",
    "StreamDurationTooShort",
    "InvalidNonce",
    "MigrationAlreadyApplied",
    "StreamNotSettled",
    "WithdrawalCooldownActive",
    "RecipientNotWhitelisted",
    "MetadataTooLong",
    "InvalidEndTime",
    "ReentrancyDetected",
    "InvalidMetadataUri",
    "StreamNotComplete",
    "TokenNotWhitelisted",
    "InvalidTranches",
    "RateLimitExceeded",
    "InvalidSlippage",
    "DurationExceedsMax",
    "StartTimeTooFar",
    "IdCollision",
    "NextStepNotReached",
    "AmountBelowMinimum",
    "InvalidExpiryWindow",
    "NewSenderStreamCapExceeded",
    "InvalidRedirectTarget",
    "CircularRedirect",
    "RedirectRecipientMismatch",
    "DuplicateTokenInDualStream",
    "IsDualStream",
    "StreamNonTransferable",
    "AwaitingApproval",
    "StreamIsLocked",
    "RecipientNotAllowed",
    "MaxDepositExceeded",
    "CommentTooLong",
    "InsufficientStake",
    "StreamPausedOrInvalidParameter",
];

/// Returns `true` when `code` is a declared `StreamError` discriminant.
pub fn is_defined_error_code(code: u32) -> bool {
    DEFINED_ERROR_CODES.contains(&code)
}

/// Resolves a code to its variant name, or `"<undefined>"`.
pub fn error_code_name(code: u32) -> &'static str {
    match DEFINED_ERROR_CODES.iter().position(|c| *c == code) {
        Some(idx) => ERROR_CODE_NAMES[idx],
        None => "<undefined>",
    }
}

/// Reports codes that appear more than once in [`DEFINED_ERROR_CODES`].
///
/// A duplicate discriminant makes the on-chain error ambiguous: two different
/// Rust variants decode from the same `u32`, so a client cannot tell which
/// condition occurred. The campaign surfaces this as a finding rather than
/// silently tolerating it, because it is a protocol-level defect that no amount
/// of fault injection can fix at runtime.
pub fn duplicate_error_codes() -> StdVec<u32> {
    let mut seen: StdVec<u32> = StdVec::new();
    let mut dupes: StdVec<u32> = StdVec::new();
    for code in DEFINED_ERROR_CODES {
        if seen.contains(code) {
            if !dupes.contains(code) {
                dupes.push(*code);
            }
        } else {
            seen.push(*code);
        }
    }
    dupes
}

/// Fault families the engine can inject.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum FaultKind {
    /// The token contract rejects the next transfer.
    TokenTransferRejects,
    /// The token contract reports insufficient balance.
    TokenInsufficientFunds,
    /// The token contract aborts the host invocation.
    TokenAborts,
    /// The ledger clock jumps backwards.
    ClockRewind,
    /// The ledger sequence number jumps forward, ageing persistent entries.
    SequenceSkew,
    /// A stream record is removed from underneath the contract.
    StreamRecordLoss,
    /// The host resource budget is starved mid-invocation.
    BudgetStarvation,
    /// Ledger state is rolled back to an earlier snapshot.
    SnapshotRollback,
    /// The trustor is drained to zero balance.
    TrustorInsolvency,
}

impl FaultKind {
    /// All fault kinds, in a stable order.
    pub const ALL: [FaultKind; 9] = [
        FaultKind::TokenTransferRejects,
        FaultKind::TokenInsufficientFunds,
        FaultKind::TokenAborts,
        FaultKind::ClockRewind,
        FaultKind::SequenceSkew,
        FaultKind::StreamRecordLoss,
        FaultKind::BudgetStarvation,
        FaultKind::SnapshotRollback,
        FaultKind::TrustorInsolvency,
    ];

    /// Stable label used in campaign reports.
    pub fn label(&self) -> &'static str {
        match self {
            FaultKind::TokenTransferRejects => "token_transfer_rejects",
            FaultKind::TokenInsufficientFunds => "token_insufficient_funds",
            FaultKind::TokenAborts => "token_aborts",
            FaultKind::ClockRewind => "clock_rewind",
            FaultKind::SequenceSkew => "sequence_skew",
            FaultKind::StreamRecordLoss => "stream_record_loss",
            FaultKind::BudgetStarvation => "budget_starvation",
            FaultKind::SnapshotRollback => "snapshot_rollback",
            FaultKind::TrustorInsolvency => "trustor_insolvency",
        }
    }

    /// Token-side faults require a token address to arm; ledger-side faults do not.
    pub fn needs_token(&self) -> bool {
        matches!(
            self,
            FaultKind::TokenTransferRejects
                | FaultKind::TokenInsufficientFunds
                | FaultKind::TokenAborts
                | FaultKind::TrustorInsolvency
        )
    }

    /// Whether the contract is expected to be able to absorb this fault.
    ///
    /// `TokenAborts` and `SnapshotRollback` are not: they destroy state the
    /// contract cannot reconstruct, so the harness records them separately
    /// rather than counting them as graceful degradation.
    pub fn is_survivable(&self) -> bool {
        !matches!(
            self,
            FaultKind::TokenAborts | FaultKind::SnapshotRollback | FaultKind::BudgetStarvation
        )
    }
}

/// A deterministic xorshift64* generator.
///
/// Chosen over a host-provided PRNG because the campaign must be reproducible
/// across SDK versions and independent of invocation ordering.
#[derive(Clone, Debug)]
pub struct ChaosRng {
    state: u64,
}

impl ChaosRng {
    /// Creates a generator. A zero seed is replaced with the golden-ratio
    /// constant, since xorshift is stuck at zero forever from that state.
    pub fn new(seed: u64) -> Self {
        ChaosRng {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// Draws the next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Draws a value in `0..bound`. Returns 0 when `bound` is 0.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as u32
        }
    }

    /// Returns `true` with probability `percent`/100.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }

    /// Picks a uniformly random element, or `None` when `items` is empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let idx = self.below(items.len() as u32) as usize;
            Some(&items[idx])
        }
    }

    /// Draws an amount in `low..=high`.
    pub fn range_i128(&mut self, low: i128, high: i128) -> i128 {
        if high <= low {
            return low;
        }
        let span = (high - low) as u64;
        low + (self.next_u64() % span) as i128
    }
}

/// Campaign configuration.
#[derive(Clone, Debug)]
pub struct ChaosConfig {
    /// Seed for [`ChaosRng`].
    pub seed: u64,
    /// Probability (0-100) that any given step injects a fault.
    pub fault_rate_percent: u32,
    /// Fault kinds eligible for injection.
    pub enabled: StdVec<FaultKind>,
    /// Fail the campaign if any finding is recorded.
    pub strict: bool,
}

impl ChaosConfig {
    /// Default campaign: every fault kind, 25% injection rate, strict.
    pub fn new(seed: u64) -> Self {
        ChaosConfig {
            seed,
            fault_rate_percent: 25,
            enabled: FaultKind::ALL.to_vec(),
            strict: true,
        }
    }

    /// Builds a campaign with no faults, used as a control group.
    pub fn control(seed: u64) -> Self {
        ChaosConfig {
            enabled: StdVec::new(),
            ..ChaosConfig::new(seed)
        }
    }

    /// Sets the per-step injection probability, clamped to `0..=100`.
    pub fn with_fault_rate(mut self, percent: u32) -> Self {
        self.fault_rate_percent = percent.min(100);
        self
    }

    /// Restricts the campaign to `kinds`.
    pub fn with_faults(mut self, kinds: StdVec<FaultKind>) -> Self {
        self.enabled = kinds;
        self
    }
}

/// One classified step of a campaign.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepRecord {
    /// Index of the step within the campaign.
    pub step: u32,
    /// Operation label, e.g. `"withdraw"`.
    pub op: &'static str,
    /// Fault injected before the step, if any.
    pub fault: Option<FaultKind>,
    /// Outcome classification.
    pub outcome: Outcome,
    /// Error code observed, when the step returned one.
    pub error_code: Option<u32>,
}

/// How a step ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Returned `Ok`.
    Succeeded,
    /// Returned a declared `StreamError`.
    Handled(u32),
    /// Succeeded despite an armed fault.
    Recovered,
    /// Panic, undefined error code, or broken invariant.
    Finding(StdString),
}

/// Aggregate result of a chaos campaign.
#[derive(Clone, Debug, Default)]
pub struct ChaosReport {
    /// Seed the campaign ran with.
    pub seed: u64,
    /// Per-step records.
    pub steps: StdVec<StepRecord>,
    /// Fault kinds that were actually armed, with counts.
    pub injected: StdVec<(FaultKind, u32)>,
}

impl ChaosReport {
    /// Number of steps that returned `Ok`.
    pub fn succeeded(&self) -> u32 {
        self.steps
            .iter()
            .filter(|s| s.outcome == Outcome::Succeeded)
            .count() as u32
    }

    /// Number of steps that returned a declared `StreamError`.
    pub fn handled(&self) -> u32 {
        self.steps
            .iter()
            .filter(|s| matches!(s.outcome, Outcome::Handled(_)))
            .count() as u32
    }

    /// Number of steps that succeeded while a fault was armed.
    pub fn recovered(&self) -> u32 {
        self.steps
            .iter()
            .filter(|s| s.outcome == Outcome::Recovered)
            .count() as u32
    }

    /// Steps that indicate a defect.
    pub fn findings(&self) -> StdVec<&StepRecord> {
        self.steps
            .iter()
            .filter(|s| matches!(s.outcome, Outcome::Finding(_)))
            .collect()
    }

    /// True when no step produced a finding.
    pub fn is_clean(&self) -> bool {
        self.findings().is_empty()
    }

    /// Multi-line human-readable summary, suitable for CI output.
    pub fn summary(&self) -> StdString {
        let mut out = StdString::new();
        out.push_str(&format!(
            "chaos campaign seed=0x{:X} steps={} injected={} ok={} handled={} recovered={} findings={}\n",
            self.seed,
            self.steps.len(),
            self.injected.iter().map(|(_, c)| *c).sum::<u32>(),
            self.succeeded(),
            self.handled(),
            self.recovered(),
            self.findings().len()
        ));
        if !self.injected.is_empty() {
            let kinds: StdVec<StdString> = self
                .injected
                .iter()
                .map(|(k, c)| format!("{}={}", k.label(), c))
                .collect();
            out.push_str(&format!("  injected: {}\n", kinds.join(", ")));
        }
        for step in self.findings() {
            let detail = match &step.outcome {
                Outcome::Finding(msg) => msg.clone(),
                _ => StdString::new(),
            };
            out.push_str(&format!(
                "  FINDING step={} op={} fault={} :: {}\n",
                step.step,
                step.op,
                step.fault.map(|f| f.label()).unwrap_or("none"),
                detail
            ));
        }
        out
    }
}

/// The chaos driver: owns the environment, the fault schedule, and the report.
pub struct ChaosEnv {
    env: Env,
    rng: ChaosRng,
    config: ChaosConfig,
    report: ChaosReport,
    baseline: Option<soroban_sdk::testutils::Snapshot>,
    step: u32,
    /// Remaining steps for which the token should keep rejecting transfers.
    token_fail_steps: u32,
}

impl ChaosEnv {
    /// Creates a driver around a fresh environment.
    pub fn new(config: ChaosConfig) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let report = ChaosReport {
            seed: config.seed,
            ..Default::default()
        };
        ChaosEnv {
            env,
            rng: ChaosRng::new(config.seed),
            config,
            report,
            baseline: None,
            step: 0,
            token_fail_steps: 0,
        }
    }

    /// The underlying Soroban environment.
    pub fn env(&self) -> &Env {
        &self.env
    }

    /// Mutable access, for tests that need to advance the ledger directly.
    pub fn env_mut(&mut self) -> &mut Env {
        &mut self.env
    }

    /// The report accumulated so far.
    pub fn report(&self) -> &ChaosReport {
        &self.report
    }

    /// The RNG, for tests that need to draw from the same stream.
    pub fn rng(&mut self) -> &mut ChaosRng {
        &mut self.rng
    }

    /// Captures a rollback point.
    pub fn capture_baseline(&mut self) {
        self.baseline = Some(self.env.to_snapshot());
    }

    /// Returns the environment to the last captured rollback point.
    pub fn rollback(&mut self) {
        if let Some(snap) = self.baseline.clone() {
            self.env = Env::from_snapshot(snap);
        }
    }

    // ── Fault injection ─────────────────────────────────────────────────────

    /// Arms `fault`, returning the kind actually injected or `None`.
    ///
    /// The draw is made from the RNG on every call so the fault schedule stays
    /// aligned with the operation schedule regardless of which operations
    /// choose to arm faults themselves.
    pub fn maybe_inject(
        &mut self,
        token: &Address,
        contract: &Address,
        stream_id: u64,
        trustor: &Address,
    ) -> Option<FaultKind> {
        if !self.rng.chance(self.config.fault_rate_percent) {
            return None;
        }
        let kind = *self.rng.pick(&self.config.enabled)?;
        self.inject(kind, token, contract, stream_id, trustor);
        Some(kind)
    }

    /// Arms a specific fault unconditionally.
    pub fn inject(
        &mut self,
        kind: FaultKind,
        token: &Address,
        contract: &Address,
        stream_id: u64,
        trustor: &Address,
    ) {
        match kind {
            FaultKind::TokenTransferRejects => {
                let client = ChaosTokenClient::new(&self.env, token);
                client.set_fault(&token_fault::ALWAYS_FAIL);
            }
            FaultKind::TokenInsufficientFunds => {
                let client = ChaosTokenClient::new(&self.env, token);
                client.set_fault(&token_fault::INSUFFICIENT);
            }
            FaultKind::TokenAborts => {
                let client = ChaosTokenClient::new(&self.env, token);
                client.set_fault(&token_fault::PANIC);
            }
            FaultKind::TrustorInsolvency => {
                let client = ChaosTokenClient::new(&self.env, token);
                client.set_balance(trustor, &0i128);
            }
            FaultKind::ClockRewind => {
                let now = self.env.ledger().timestamp();
                self.env.ledger().set_timestamp(now.saturating_sub(5_000));
            }
            FaultKind::SequenceSkew => {
                let seq = self.env.ledger().sequence();
                self.env
                    .ledger()
                    .set_sequence_number(seq.saturating_add(250_000));
            }
            FaultKind::StreamRecordLoss => {
                let env = self.env.clone();
                env.as_contract(contract, || {
                    env.storage().persistent().remove(&stream_id);
                });
            }
            FaultKind::BudgetStarvation => {
                let mut budget = self.env.cost_estimate().budget();
                budget.reset_limits(1_000, 1_000);
            }
            FaultKind::SnapshotRollback => self.rollback(),
        }
    }

    /// Makes the next `steps` protocol operations fail at the token boundary.
    ///
    /// The countdown is held here rather than in the token because a failing
    /// invocation's storage writes are rolled back by the host — a counter
    /// decremented inside `transfer` can never reach zero. Each [`Self::clear`]
    /// re-arms the mode for one more step while the budget lasts, and this
    /// arming is its own successful invocation, so it is not rolled back.
    pub fn arm_token_failures(&mut self, steps: u32) {
        self.token_fail_steps = steps;
    }

    /// Disarms token faults and restores the host budget.
    ///
    /// Called between steps so a fault armed for one operation cannot leak
    /// into the next and make the campaign's fault schedule meaningless. A
    /// countdown armed through [`Self::arm_token_failures`] survives one extra
    /// step, which is what gives "fail the next N operations" its meaning.
    pub fn clear(&mut self, token: &Address) {
        let client = ChaosTokenClient::new(&self.env, token);
        if self.token_fail_steps > 0 {
            self.token_fail_steps -= 1;
            client.set_fault(&token_fault::ALWAYS_FAIL);
        } else {
            client.clear_fault();
        }
        let mut budget = self.env.cost_estimate().budget();
        budget.reset_default();
    }

    // ── Outcome recording ───────────────────────────────────────────────────

    /// Records a step that returned `Ok`.
    pub fn record_ok(&mut self, op: &'static str, fault: Option<FaultKind>) {
        let outcome = if fault.is_some() {
            Outcome::Recovered
        } else {
            Outcome::Succeeded
        };
        self.push(op, fault, outcome, None);
    }

    /// Records a step that returned a declared error code.
    pub fn record_error(&mut self, op: &'static str, fault: Option<FaultKind>, code: u32) {
        let outcome = if is_defined_error_code(code) {
            Outcome::Handled(code)
        } else {
            Outcome::Finding(format!(
                "undefined error code {code}; declared codes are {DEFINED_ERROR_CODES:?}"
            ))
        };
        self.push(op, fault, outcome, Some(code));
    }

    /// Records a step that panicked or otherwise escaped the contract.
    pub fn record_panic(&mut self, op: &'static str, fault: Option<FaultKind>, detail: &str) {
        let outcome = Outcome::Finding(format!("panic escaped {op}: {detail}"));
        self.push(op, fault, outcome, None);
    }

    /// Records a violated invariant.
    pub fn record_invariant_violation(&mut self, op: &'static str, detail: &str) {
        let outcome = Outcome::Finding(format!("invariant violated: {detail}"));
        self.push(op, None, outcome, None);
    }

    /// Reports error codes that are ambiguous on-chain.
    pub fn check_error_table(&mut self) {
        for code in duplicate_error_codes() {
            self.record_invariant_violation(
                "error_table",
                &format!("error code {code} is declared more than once"),
            );
        }
    }

    fn push(
        &mut self,
        op: &'static str,
        fault: Option<FaultKind>,
        outcome: Outcome,
        error_code: Option<u32>,
    ) {
        if let Some(kind) = fault {
            match self.report.injected.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, count)) => *count += 1,
                None => self.report.injected.push((kind, 1)),
            }
        }
        self.report.steps.push(StepRecord {
            step: self.step,
            op,
            fault,
            outcome,
            error_code,
        });
        self.step += 1;
    }

    /// Panics when the campaign recorded findings and the config is strict.
    pub fn assert_no_findings(&self) {
        if self.config.strict {
            assert!(
                self.report.is_clean(),
                "chaos campaign recorded findings:\n{}",
                self.report.summary()
            );
        }
    }
}
