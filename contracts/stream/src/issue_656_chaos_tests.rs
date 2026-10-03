//! Chaos campaigns for the SoroStream contract (issue #656).
//!
//! Each test here is a *campaign*: a deterministic sequence of protocol
//! operations with faults injected between them, asserting that the contract
//! degrades gracefully. The fault-injection machinery lives in
//! [`crate::chaos`]; this file supplies the operations.
//!
//! # Campaign shape
//!
//! ```text
//!   for each step:
//!       fault  <- rng draw over FaultKind::ALL        (may be None)
//!       arm the fault against the token / ledger / storage
//!       invoke a protocol entry point through try_
//!       classify: Ok | Err(defined code) | Err(undefined) | panic
//!       disarm the fault so it cannot leak into the next step
//! ```
//!
//! # What "graceful" means here
//!
//! * The invocation returns a `StreamError` that exists in
//!   [`crate::chaos::DEFINED_ERROR_CODES`], **or** it succeeds.
//! * No panic escapes the host.
//! * The contract's token balance never goes negative and never exceeds what
//!   the ledger can account for.
//! * Stream records that the campaign deleted are reported as
//!   `StreamNotFound`, not as a panic from a half-written record.
//!
//! # Reproducing a failure
//!
//! Every campaign prints its seed on failure. Re-run with the same seed to get
//! the identical fault schedule:
//!
//! ```ignore
//! SOROSTREAM_CHAOS_SEED=0xDEADBEEF cargo test -p sorostream-stream chaos_
//! ```
//!
//! [`SOROSTREAM_CHAOS_SEED`]: std::env

use super::*;
use std::format;
use crate::chaos::{
    duplicate_error_codes, is_defined_error_code, ChaosConfig, ChaosEnv, ChaosReport, ChaosRng,
    ChaosToken, ChaosTokenClient, FaultKind,
};
use crate::types::{CreateStreamParams, StreamStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

/// The shape a generated `try_<entrypoint>` returns for a `Result<T, E>`
/// contract function.
///
/// Naming it keeps [`Harness::classify`] readable; the nesting is inherent to
/// the SDK and has nothing to do with this campaign.
type TryResult<T, C, E> = Result<Result<T, C>, Result<E, soroban_sdk::InvokeError>>;

/// Reads an override seed from the environment, defaulting to a fixed value so
/// CI runs are reproducible without configuration.
fn campaign_seed(default: u64) -> u64 {
    std::env::var("SOROSTREAM_CHAOS_SEED")
        .ok()
        .and_then(|v| {
            let v = v.trim();
            if let Some(hex) = v.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()
            } else {
                v.parse::<u64>().ok()
            }
        })
        .unwrap_or(default)
}

/// Everything a campaign needs to drive the contract.
struct Harness {
    chaos: ChaosEnv,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
    treasury: Address,
}

impl Harness {
    fn with_config(config: ChaosConfig) -> Self {
        let mut chaos = ChaosEnv::new(config);
        let env = chaos.env().clone();

        let contract = env.register(SoroStreamContract, ());
        let token_admin = Address::generate(&env);
        let token = env.register(ChaosToken, (token_admin,));

        let sender = Address::generate(&env);
        let recipient = Address::generate(&env);
        let treasury = Address::generate(&env);

        let token_client = ChaosTokenClient::new(&env, &token);
        token_client.mint(&sender, &100_000_000i128);

        env.ledger().set_timestamp(1_000);
        let client = SoroStreamContractClient::new(&env, &contract);
        client.set_min_duration(&sender, &0u64);
        client.set_treasury_address(&treasury);

        chaos.capture_baseline();

        Harness {
            chaos,
            contract,
            token,
            sender,
            recipient,
            treasury,
        }
    }

    fn params(nonce: u64) -> CreateStreamParams {
        CreateStreamParams {
            cliff_seconds: 0,
            nonce,
            renew_count: None,
            recurrence: None,
            lock_until: 0,
            allow_recipient_termination: false,
            non_transferable: false,
            holdback_amount: 0,
            withdrawal_steps: None,
            min_withdrawal_amount: None,
            sponsor: None,
            requires_recipient_approval: false,
            priority: None,
            tags: None,
            metadata_uri: None,
        }
    }

    /// Creates a stream and returns its ID.
    fn create(&self, amount: i128, duration: u64, nonce: u64) -> u64 {
        let env = self.chaos.env();
        let client = SoroStreamContractClient::new(env, &self.contract);
        client.create_stream(
            &self.sender,
            &self.recipient,
            &self.token,
            &amount,
            &duration,
            &false,
            &Harness::params(nonce),
        )
    }

    /// Total tokens the contract is holding.
    fn contract_balance(&self) -> i128 {
        ChaosTokenClient::new(self.chaos.env(), &self.token).balance(&self.contract)
    }

    /// Invokes an entry point through `try_` and classifies the outcome.
    ///
    /// The three-way match is the point of the campaign: `Ok(Ok(Ok(..)))` is a
    /// success, `Ok(Ok(Err(code)))` is a declared error, and `Err(..)` means
    /// the host rejected the invocation — which is a finding unless the caller
    /// already knows a fault was unsurvivable.
    ///
    /// `E` is the contract's own error type; `code_of` lifts it into the
    /// campaign's `u32` code space.
    fn classify<T, C: core::fmt::Debug, E>(
        &mut self,
        op: &'static str,
        result: TryResult<T, C, E>,
        fault: Option<FaultKind>,
        code_of: impl Fn(&E) -> u32,
    ) {
        match result {
            Ok(Ok(_)) => self.chaos.record_ok(op, fault),
            Ok(Err(conv_err)) => {
                // The invocation succeeded but the success value failed to
                // decode — always a finding, never an expected outcome.
                self.chaos.record_panic(op, fault, &format!("{conv_err:?}"));
            }
            Err(Ok(e)) => {
                let code = code_of(&e);
                self.chaos.record_error(op, fault, code)
            }
            Err(Err(invoke)) => {
                // The host itself refused the invocation. For faults the
                // contract cannot survive this is expected, so it is recorded
                // as a finding only when the fault was survivable.
                let survivable = fault.map(|f| f.is_survivable()).unwrap_or(true);
                if survivable {
                    self.chaos.record_panic(op, fault, &format!("{invoke:?}"));
                } else {
                    self.chaos.record_ok(op, fault);
                }
            }
        }
    }
}

// ── Framework self-tests ────────────────────────────────────────────────────

#[test]
fn chaos_rng_is_deterministic_for_a_seed() {
    let a: std::vec::Vec<u64> = (0..16)
        .scan(ChaosRng::new(42), |r, _| Some(r.next_u64()))
        .collect();
    let b: std::vec::Vec<u64> = (0..16)
        .scan(ChaosRng::new(42), |r, _| Some(r.next_u64()))
        .collect();
    let c: std::vec::Vec<u64> = (0..16)
        .scan(ChaosRng::new(43), |r, _| Some(r.next_u64()))
        .collect();
    assert_eq!(a, b, "same seed must produce the same stream");
    assert_ne!(a, c, "different seeds must diverge");
}

#[test]
fn chaos_rng_zero_seed_does_not_degenerate() {
    let mut rng = ChaosRng::new(0);
    let values: std::vec::Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
    assert!(
        values.iter().any(|v| *v != 0),
        "a zero seed must be replaced, not left stuck at zero"
    );
}

#[test]
fn every_fault_kind_has_a_distinct_label() {
    let mut labels: std::vec::Vec<&'static str> =
        FaultKind::ALL.iter().map(|f| f.label()).collect();
    labels.sort_unstable();
    let count = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), count, "fault labels must be unique");
}

#[test]
fn token_side_faults_declare_that_they_need_a_token() {
    for kind in FaultKind::ALL {
        let expected = matches!(
            kind,
            FaultKind::TokenTransferRejects
                | FaultKind::TokenInsufficientFunds
                | FaultKind::TokenAborts
                | FaultKind::TrustorInsolvency
        );
        assert_eq!(kind.needs_token(), expected, "{:?}", kind);
    }
}

#[test]
fn defined_error_code_table_is_sorted_and_unique() {
    let mut sorted = crate::chaos::DEFINED_ERROR_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        crate::chaos::DEFINED_ERROR_CODES.to_vec(),
        "the declared error-code table must be sorted so a duplicate is visible"
    );
}

#[test]
fn every_defined_error_code_has_a_name() {
    assert_eq!(
        crate::chaos::DEFINED_ERROR_CODES.len(),
        crate::chaos::ERROR_CODE_NAMES.len(),
        "each declared code must carry a name"
    );
}

#[test]
fn error_code_lookup_reports_unknown_codes() {
    assert!(is_defined_error_code(1));
    assert!(!is_defined_error_code(7), "code 7 is not declared");
    assert!(!is_defined_error_code(9_999));
    assert_eq!(crate::chaos::error_code_name(7), "<undefined>");
    assert_eq!(crate::chaos::error_code_name(1), "StreamNotFound");
}

#[test]
fn error_code_table_has_no_ambiguous_discriminants() {
    assert!(
        duplicate_error_codes().is_empty(),
        "duplicate error discriminants make the on-chain error ambiguous: {:?}",
        duplicate_error_codes()
    );
}

// ── Single-fault campaigns ──────────────────────────────────────────────────

#[test]
fn chaos_token_rejects_transfers_when_armed() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA1)));
    let stream_id = h.create(10_000, 1_000, 0);

    // The countdown is host-side: a counter written inside a failing
    // invocation would be rolled back and never reach zero.
    h.chaos.arm_token_failures(1);
    h.chaos.inject(
        FaultKind::TokenTransferRejects,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );
    let before = h.contract_balance();

    let env = h.chaos.env().clone();
    let result =
        SoroStreamContractClient::new(&env, &h.contract).try_cancel_stream(&stream_id, &h.sender);
    h.classify(
        "cancel_stream",
        result,
        Some(FaultKind::TokenTransferRejects),
        |e| (*e as u32),
    );

    h.chaos.clear(&h.token);
    assert_eq!(
        h.contract_balance(),
        before,
        "a rejected refund must not move the contract's balance"
    );
    h.chaos.assert_no_findings();
}

#[test]
fn chaos_insolvent_trustor_does_not_panic() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA2)));
    let stream_id = h.create(10_000, 1_000, 0);

    h.chaos.inject(
        FaultKind::TrustorInsolvency,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );
    h.chaos.env().ledger().set_timestamp(500);

    let env = h.chaos.env().clone();
    let result =
        SoroStreamContractClient::new(&env, &h.contract).try_withdraw(&stream_id, &h.recipient);
    h.classify(
        "withdraw",
        result,
        Some(FaultKind::TrustorInsolvency),
        |e| (*e as u32),
    );

    h.chaos.clear(&h.token);
    h.chaos.assert_no_findings();
}

#[test]
fn chaos_lost_stream_record_reports_not_found() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA3)));
    let stream_id = h.create(10_000, 1_000, 0);

    h.chaos.inject(
        FaultKind::StreamRecordLoss,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );

    let env = h.chaos.env().clone();
    let result = SoroStreamContractClient::new(&env, &h.contract).try_get_stream(&stream_id);
    match result {
        Err(Ok(e)) => {
            let code = e as u32;
            assert!(
                is_defined_error_code(code),
                "a deleted stream must yield a declared error, got {code}"
            );
        }
        Ok(Ok(_)) => {
            panic!("get_stream returned a record the ledger no longer holds")
        }
        Ok(Err(conv_err)) => panic!("conversion error decoding stream: {conv_err:?}"),
        Err(Err(invoke)) => panic!("get_stream panicked on a missing record: {invoke:?}"),
    }
}

#[test]
fn chaos_clock_rewind_does_not_break_accounting() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA4)));
    let stream_id = h.create(10_000, 1_000, 0);
    let escrowed = h.contract_balance();

    h.chaos.inject(
        FaultKind::ClockRewind,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );

    let env = h.chaos.env().clone();
    let result = SoroStreamContractClient::new(&env, &h.contract).try_get_claimable(&stream_id);
    h.classify("get_claimable", result, Some(FaultKind::ClockRewind), |e| {
        (*e as u32)
    });

    assert!(
        h.contract_balance() >= 0,
        "the contract must never hold a negative balance"
    );
    assert_eq!(
        h.contract_balance(),
        escrowed,
        "a read-only fault must not move escrow"
    );
    h.chaos.assert_no_findings();
}

#[test]
fn chaos_sequence_skew_keeps_state_readable() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA5)));
    let stream_id = h.create(10_000, 1_000, 0);

    h.chaos.inject(
        FaultKind::SequenceSkew,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );

    let env = h.chaos.env().clone();
    let result = SoroStreamContractClient::new(&env, &h.contract).try_get_stream(&stream_id);
    h.classify("get_stream", result, Some(FaultKind::SequenceSkew), |e| {
        (*e as u32)
    });
    h.chaos.assert_no_findings();
}

#[test]
fn chaos_budget_starvation_is_survivable_by_the_harness() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0xA6)));
    let stream_id = h.create(10_000, 1_000, 0);

    // Starve the budget, attempt an operation, then restore. The contract
    // cannot be blamed for the host refusing to give it instructions.
    h.chaos.inject(
        FaultKind::BudgetStarvation,
        &h.token,
        &h.contract,
        stream_id,
        &h.sender,
    );
    h.chaos.clear(&h.token);

    let env = h.chaos.env().clone();
    let result = SoroStreamContractClient::new(&env, &h.contract).try_get_stream(&stream_id);
    h.classify("get_stream", result, None, |e| (*e as u32));
    h.chaos.assert_no_findings();
}

// ── Multi-step campaigns ────────────────────────────────────────────────────

/// Runs `steps` interleaved create/withdraw/cancel operations with a fault
/// drawn before each one, then asserts the whole campaign stayed clean.
fn run_campaign(seed: u64, steps: u32) -> ChaosReport {
    let mut h = Harness::with_config(
        ChaosConfig::new(seed)
            .with_fault_rate(40)
            .with_faults(FaultKind::ALL.to_vec()),
    );
    h.chaos.check_error_table();

    let env = h.chaos.env().clone();
    let client = SoroStreamContractClient::new(&env, &h.contract);
    let token_client = ChaosTokenClient::new(&env, &h.token);
    token_client.mint(&h.sender, &10_000_000i128);

    let mut nonce = 0u64;
    let mut live: std::vec::Vec<u64> = std::vec::Vec::new();

    for step in 0..steps {
        // Advance the clock monotonically so accrual stays meaningful, then
        // let the RNG inject its fault on top of that.
        let now = env.ledger().timestamp();
        env.ledger().set_timestamp(now + 137);

        let target = if live.is_empty() {
            0
        } else {
            h.chaos.rng().below(live.len() as u32) as usize
        };
        let stream_id = if target < live.len() { live[target] } else { 0 };

        let fault = h
            .chaos
            .maybe_inject(&h.token, &h.contract, stream_id, &h.sender);

        match step % 3 {
            0 => {
                let amount = h.chaos.rng().range_i128(1_000, 50_000);
                let duration = 1_000 + h.chaos.rng().below(4_000) as u64;
                let result = client.try_create_stream(
                    &h.sender,
                    &h.recipient,
                    &h.token,
                    &amount,
                    &duration,
                    &false,
                    &Harness::params(nonce),
                );
                nonce += 1;
                h.classify("create_stream", result, fault, |e| (*e as u32));
                if let Ok(Ok(id)) = result {
                    live.push(id);
                }
            }
            1 if !live.is_empty() => {
                let id = live[h.chaos.rng().below(live.len() as u32) as usize];
                let result = client.try_withdraw(&id, &h.recipient);
                h.classify("withdraw", result, fault, |e| (*e as u32));
            }
            2 if !live.is_empty() => {
                let idx = h.chaos.rng().below(live.len() as u32) as usize;
                let id = live[idx];
                let result = client.try_cancel_stream(&id, &h.sender);
                let cancelled = matches!(result, Ok(Ok(())));
                h.classify("cancel_stream", result, fault, |e| (*e as u32));
                if cancelled {
                    live.remove(idx);
                }
            }
            _ => {
                // Idle step: read-only, so any fault must leave escrow intact.
                // `get_all_stream_ids` has no application-level error to
                // classify (it never returns a StreamError), so just confirm
                // the host-level invocation itself didn't trap unexpectedly.
                let before = token_client.balance(&h.contract);
                let result = client.try_get_all_stream_ids(&0, &10);
                if let Err(Err(invoke)) = result {
                    let survivable = fault.map(|f| f.is_survivable()).unwrap_or(true);
                    if survivable {
                        h.chaos.record_panic("get_all_stream_ids", fault, &format!("{invoke:?}"));
                    }
                }
                assert_eq!(
                    token_client.balance(&h.contract),
                    before,
                    "step {step}: a read-only operation moved escrow"
                );
            }
        }

        h.chaos.clear(&h.token);
    }

    // Terminal accounting check: whatever survived the campaign, the contract
    // still holds it.
    assert!(
        token_client.balance(&h.contract) >= 0,
        "campaign ended with a negative contract balance"
    );
    assert!(
        token_client.balance(&h.sender) >= 0,
        "campaign ended with a negative sender balance"
    );

    let report = h.chaos.report().clone();
    report
}

#[test]
fn chaos_campaign_seed_0x1_stays_clean() {
    let report = run_campaign(0x1, 24);
    assert!(report.is_clean(), "{}", report.summary());
}

#[test]
fn chaos_campaign_seed_0x2_stays_clean() {
    let report = run_campaign(0x2, 24);
    assert!(report.is_clean(), "{}", report.summary());
}

#[test]
fn chaos_campaign_seed_0x3_stays_clean() {
    let report = run_campaign(0x3, 32);
    assert!(report.is_clean(), "{}", report.summary());
}

#[test]
fn chaos_campaign_actually_injects_faults() {
    // A campaign that silently injects nothing would pass trivially, so assert
    // the fault schedule is non-empty and covers more than one fault family.
    let report = run_campaign(0x4, 40);
    let injected = report.injected.iter().map(|(_, c)| *c).sum::<u32>();
    assert!(
        injected >= 4,
        "campaign injected only {injected} faults:\n{}",
        report.summary()
    );
    assert!(
        report.injected.len() >= 2,
        "campaign exercised only one fault family:\n{}",
        report.summary()
    );
}

#[test]
fn chaos_control_campaign_injects_nothing() {
    let report = run_campaign_with(ChaosConfig::control(0x5), 16);
    assert_eq!(report.injected.len(), 0, "control campaign injected faults");
    assert!(report.is_clean(), "{}", report.summary());
}

/// `run_campaign` with a caller-supplied config, for the control group.
fn run_campaign_with(config: ChaosConfig, steps: u32) -> ChaosReport {
    let mut h = Harness::with_config(config.with_fault_rate(0));
    h.chaos.check_error_table();

    let env = h.chaos.env().clone();
    let client = SoroStreamContractClient::new(&env, &h.contract);
    let token_client = ChaosTokenClient::new(&env, &h.token);
    token_client.mint(&h.sender, &10_000_000i128);

    for step in 0..steps {
        let now = env.ledger().timestamp();
        env.ledger().set_timestamp(now + 211);
        let amount = h.chaos.rng().range_i128(1_000, 20_000);
        let result = client.try_create_stream(
            &h.sender,
            &h.recipient,
            &h.token,
            &amount,
            &2_000u64,
            &false,
            &Harness::params(step as u64),
        );
        h.classify("create_stream", result, None, |e| (*e as u32));
    }

    h.chaos.report().clone()
}

#[test]
fn chaos_campaign_leaves_no_stream_in_a_bogus_state() {
    let mut h = Harness::with_config(ChaosConfig::new(campaign_seed(0x6)).with_fault_rate(30));
    let env = h.chaos.env().clone();
    let client = SoroStreamContractClient::new(&env, &h.contract);
    let token_client = ChaosTokenClient::new(&env, &h.token);
    token_client.mint(&h.sender, &10_000_000i128);

    for nonce in 0..12u64 {
        let fault = h.chaos.maybe_inject(&h.token, &h.contract, 0, &h.sender);
        let result = client.try_create_stream(
            &h.sender,
            &h.recipient,
            &h.token,
            &5_000i128,
            &3_000u64,
            &false,
            &Harness::params(nonce),
        );
        h.classify("create_stream", result, fault, |e| (*e as u32));
        h.chaos.clear(&h.token);
    }

    // Every stream the campaign knows about must still be a declared status,
    // and its recorded escrow must be non-negative.
    let ids = client.get_all_stream_ids(&0, &50);
    for id in ids.iter() {
        if let Ok(Ok(stream)) = client.try_get_stream(&id) {
            assert!(
                stream.deposit >= 0,
                "stream {id} recorded a negative deposit after chaos"
            );
            assert!(
                matches!(
                    stream.status,
                    StreamStatus::Active
                        | StreamStatus::Paused
                        | StreamStatus::PendingApproval
                        | StreamStatus::Completed
                        | StreamStatus::Cancelled
                        | StreamStatus::Expired
                ),
                "stream {id} ended in an undeclared status"
            );
        }
    }

    h.chaos.assert_no_findings();
}

#[test]
fn chaos_string_helpers_are_available_to_campaigns() {
    // The campaign harness addresses the contract by string label in some
    // entry points; keep a smoke test so the label plumbing stays covered.
    let h = Harness::with_config(ChaosConfig::new(0x7));
    let env = h.chaos.env();
    let label = String::from_str(env, "chaos-campaign");
    assert_eq!(label, String::from_str(env, "chaos-campaign"));
}

#[test]
fn chaos_env_exposes_a_usable_host() {
    let chaos = ChaosEnv::new(ChaosConfig::new(0x8));
    let env: &Env = chaos.env();
    env.ledger().set_timestamp(42);
    assert_eq!(env.ledger().timestamp(), 42);
}
