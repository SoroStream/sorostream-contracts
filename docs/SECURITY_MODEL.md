# SoroStream Contract Security Model

**Version:** 1.0.0  
**Last updated:** 2026-08-25  
**Contract version:** see `get_version()`  
**Status:** Living document — update whenever the trust surface or role model changes.

---

## Table of Contents

1. [Purpose and Scope](#1-purpose-and-scope)
2. [Security Properties](#2-security-properties)
3. [Trust Assumptions](#3-trust-assumptions)
4. [Privileged Roles](#4-privileged-roles)
   - 4.1 [Admin](#41-admin)
   - 4.2 [Guardian](#42-guardian)
   - 4.3 [Governance](#43-governance)
   - 4.4 [Sender](#44-sender)
   - 4.5 [Recipient](#45-recipient)
   - 4.6 [Delegate](#46-delegate)
   - 4.7 [Anyone (permissionless)](#47-anyone-permissionless)
5. [Threat Model](#5-threat-model)
   - 5.1 [In scope](#51-in-scope)
   - 5.2 [Out of scope](#52-out-of-scope)
   - 5.3 [Attack vectors](#53-attack-vectors)
6. [What the Contract Protects Against](#6-what-the-contract-protects-against)
7. [What the Contract Does Not Protect Against](#7-what-the-contract-does-not-protect-against)
8. [Invariants](#8-invariants)
9. [Verification and Testing](#9-verification-and-testing)
10. [Governance Roadmap](#10-governance-roadmap)
11. [Related Documents](#11-related-documents)

---

## 1. Purpose and Scope

This document is the authoritative security model for the SoroStream payment-streaming
protocol deployed on Stellar's Soroban platform. It defines:

- The **trust assumptions** the contract makes about its environment and participants.
- The **privileged roles** that exist and what each role can and cannot do.
- The **threat model** — what attacks the contract is designed to resist, and what
  it deliberately leaves outside its security boundary.

This document covers the on-chain stream contract (`contracts/stream/`) and references
the adjacent contracts (`contracts/governance/`, `contracts/multisig/`,
`contracts/treasury/`, `contracts/proxy/`) where they interact with stream security.
It does not cover off-chain infrastructure, frontends, or wallet software.

For the full attack-vector analysis see [`docs/THREAT_MODEL.md`](./THREAT_MODEL.md).  
For the complete admin capability matrix see [`SECURITY.md`](../SECURITY.md).  
For the authorisation enforcement details see [`docs/permissions.md`](./permissions.md).

---

## 2. Security Properties

The contract is designed to uphold these properties at all times after `initialize`:

| # | Property | Description |
|---|----------|-------------|
| **P1** | **Fund conservation** | The sum of all claimable amounts plus unstreamed deposits never exceeds the total tokens transferred into the contract. No tokens are created or destroyed. |
| **P2** | **Claimable ≤ deposit** | The claimable amount for any stream at any point in time never exceeds that stream's original deposit. Formally verified by Kani. |
| **P3** | **Monotonic accrual** | A recipient's claimable balance is non-decreasing over time (never goes backwards). Formally verified by Kani. |
| **P4** | **Cliff gating** | No tokens are claimable before `cliff_time`. `get_claimable` returns `0` for any timestamp strictly before the cliff. Formally verified by Kani. |
| **P5** | **Auth gating** | Every state-mutating instruction that affects funds requires an authorisation signature from the appropriate party. The contract verifies Soroban-native `require_auth()` — it cannot be bypassed by passing a different address. |
| **P6** | **Role isolation** | Each role is constrained to exactly the operations listed in Section 4. No role can escalate its own privileges without the cooperation of a higher-privilege role. |
| **P7** | **Reentrancy safety** | A reentrancy guard prevents a recipient contract from calling `withdraw` recursively during settlement. All state mutations occur before outbound token transfers (checks-effects-interactions). |
| **P8** | **Nonce uniqueness** | Each `(sender, nonce)` pair may create at most one stream. Reuse is rejected with `DuplicateStream`. |
| **P9** | **Pause scope** | When the contract is paused, stream creation is blocked. Existing streams continue to accrue and can be withdrawn, cancelled, or topped up. A pause cannot permanently lock user funds. |
| **P10** | **Fee-change timelock** | Protocol fee increases require a 7-day on-chain timelock before they take effect. Users have 7 days to cancel streams if the new fee is unacceptable. |

---

## 3. Trust Assumptions

The contract is correct and secure **provided the following assumptions hold**. Where
an assumption is violated, the corresponding risk is noted.

### A1 — Stellar / Soroban VM is honest

The contract trusts the Soroban execution environment unconditionally: deterministic
WASM execution, correct auth enforcement, monotonically advancing ledger timestamps,
and faithful storage reads/writes. A compromised validator supermajority that can
manipulate consensus lies outside the contract's security boundary.

**Risk if violated:** Arbitrary state manipulation; all on-chain security guarantees
collapse simultaneously. This is an infrastructure risk, not a contract risk.

### A2 — SAC tokens conform to SEP-0041

The contract assumes that any token passed to `create_stream` correctly implements the
SEP-0041 token interface: transfers move exactly the stated amount, balances are
consistent, and the token contract is not upgradeable to malicious code after stream
creation.

**Risk if violated:** A fee-on-transfer or rebasing token causes the contract to hold
less than the recorded `deposit`, making full withdrawal impossible without a WASM
upgrade. A token that can be upgraded to malicious code after stream creation can
drain all streams using that token.

**Current status:** No on-chain token allowlist exists. The `docs/ADDING_TOKENS.md`
due-diligence checklist is the only current control. A token whitelist is planned
(see Section 10).

### A3 — The admin key is not compromised

The admin address currently has broad powers including the ability to upgrade the
contract WASM without a timelock. The contract assumes the admin key is held securely
and operated honestly.

**Risk if violated:** A compromised admin can immediately upgrade to malicious WASM,
redirect fee revenue, or manipulate stream-creation parameters. See Section 4.1 and
[`SECURITY.md — Admin Trust Assumptions`](../SECURITY.md#admin-trust-assumptions)
for the full impact matrix.

**Current status:** The admin is a single EOA. Multi-sig and an upgrade timelock are
planned mitigations (see Section 10).

### A4 — Senders and recipients use separate mainnet keys

The contract assumes senders and recipients hold their own private keys and do not
reuse keys across testnet and mainnet. Nonce-based deduplication applies per network
(Soroban enforces network passphrase separation), but a key shared across networks
increases the risk of accidental nonce collision in testing.

**Risk if violated:** Low practical impact due to Soroban network passphrase
enforcement. Primarily a key-hygiene concern.

### A5 — Ledger timestamps advance within ±5 seconds of real time

Stream vesting rates are computed from `env.ledger().timestamp()`. The contract
assumes this timestamp is set by validator consensus and cannot be manipulated by a
single actor.

**Risk if violated:** A validator coalition controlling timestamp manipulation could
cause streams to vest faster or slower than intended. Kani proofs verify that even
with adversarial timestamps within realistic bounds, `claimable ≤ deposit` holds.

### A6 — The frontend is an untrusted relay

The contract does not trust any frontend or off-chain component to enforce security
properties. All authorisation is verified on-chain via `require_auth()`. A compromised
frontend can mislead users into approving transactions, but it cannot forge signatures
or bypass on-chain checks.

**Risk if violated (frontend):** Social engineering / phishing. Users should always
verify contract addresses and transaction parameters directly, independent of any
frontend display.

---

## 4. Privileged Roles

### 4.1 Admin

The admin is the highest-privilege role. It is stored at `DataKey::Admin` in
persistent storage and set at `initialize`. Transfer of the admin role requires the
current admin's authorisation via `set_admin`.

**Capabilities:**

| Instruction | Effect |
|-------------|--------|
| `set_admin` | Transfers admin role to a new address. Permanent if done unintentionally. |
| `emergency_pause` / `emergency_resume` | Pauses or resumes all stream creation. Auto-expires after `MAX_PAUSE_DURATION`. |
| `upgrade` | Replaces the contract WASM immediately. **No timelock.** |
| `migrate` | Runs a named post-upgrade migration step. Replay-guarded. |
| `set_max_streams` | Sets the global active-stream cap. Setting to 0 blocks all new streams. |
| `set_sender_stream_limit` | Sets a per-sender active-stream cap. Setting to 0 blocks a specific sender. |
| `set_min_duration` / `set_max_duration` | Sets the range of allowed stream durations. |
| `propose_fee_change` / `execute_fee_change` | Proposes and applies a fee change after a mandatory 7-day timelock. |
| `set_protocol_fee` | Sets the fee directly (legacy path; prefer the timelocked path). |
| `set_treasury_address` | Redirects protocol fee income to a new address immediately. |
| `sweep_fees` | Transfers accumulated fee income to a destination. |
| `set_whitelist_enabled` / `add_to_whitelist` / `remove_from_whitelist` | Controls recipient gating. Enabling without pre-approving recipients locks out all new streams. |
| `set_token_whitelist_enabled` / `add_token_to_whitelist` / `remove_token_from_whitelist` | Controls which tokens are accepted. |
| `add_fee_exempt` / `remove_fee_exempt` | Controls per-address fee exemptions. |
| `set_guardian` | Sets the guardian address. |
| `set_governance` | Sets the governance contract address. |
| `set_withdrawal_cooldown` | Sets a minimum time between consecutive withdrawals per stream. |
| `set_rate_limit_window` / `set_rate_limit_max` | Sets stream-creation rate limits. |
| `add_to_blocklist` / `remove_from_blocklist` | Blocks or unblocks addresses from any participation. |
| `set_creation_fee` | Sets the flat XLM creation fee. |
| `set_delegate` | Admin cannot call this — only the stream sender can grant a delegate. |
| `recalibrate_stats` | Recalculates aggregate protocol statistics. |
| `register_federation` / `unregister_federation` | Manages the on-chain federation name registry. |

**What a compromised admin can do:**
1. Immediately upgrade the WASM to arbitrary code — can drain all funds in one transaction.
2. Redirect all future fee income to an attacker-controlled address.
3. Pause the contract for up to `MAX_PAUSE_DURATION` seconds (auto-expires; cannot be permanent without a repeated attack).
4. Block specific senders or recipients from participating.
5. Enable the whitelist without pre-approving recipients, locking out all new stream creation.
6. Raise protocol fees to 100% via `propose_fee_change` (subject to 7-day timelock).

**What a compromised admin cannot do (without a WASM upgrade):**
- Steal funds from existing active streams by calling any currently deployed instruction.
- Impersonate a sender or recipient to cancel or withdraw from their streams.
- Bypass the 7-day fee-change timelock.
- Permanently pause the contract — `emergency_pause` auto-expires.

### 4.2 Guardian

The guardian is a secondary address stored at `DataKey::Guardian` in persistent
storage. It is an optional role; if not set, the guardian capability is inactive.

**Capabilities:**
- `pause` — triggers a global pause. Limited to pause only; cannot upgrade, change fees, or access funds.

**Purpose:** The guardian role allows an operational key to handle emergency pauses
without holding the full admin blast radius. The admin and guardian keys should be
stored separately so a single incident cannot compromise both.

**Limitations:** The guardian cannot unpause — that requires the `governance` role.
This separation prevents a compromised guardian from cycling pause/unpause to disrupt
the protocol.

### 4.3 Governance

The governance address is stored at `DataKey::Governance` in persistent storage. It
is an optional role intended to be a multisig or DAO contract.

**Capabilities:**
- `unpause` — lifts a guardian-triggered pause.

**Purpose:** Governance acts as the counterpart to the guardian, ensuring that a
pause cannot be lifted unilaterally by the same party that initiated it.

### 4.4 Sender

The sender is the stream creator and payer. The sender's address is embedded in the
`Stream` struct at creation time (`stream.sender`) and cannot be changed after
creation. Per-stream authorisation is enforced by `stream.sender == sender` in
addition to `sender.require_auth()`.

**Capabilities (per stream):**
- `create_stream` and all `create_stream_*` variants — stream creation.
- `cancel_stream`, `partial_cancel_stream` — early termination; unstreamed funds returned to sender.
- `top_up` — extend duration by adding more tokens.
- `pause_stream`, `resume_stream` — temporarily freeze a stream's vesting.
- `cancel_auto_renew`, `update_metadata`, `update_metadata_uri` — non-financial stream management.
- `set_slippage_params` — configure oracle deviation parameters.
- `set_delegate` — grant a delegate address withdrawal rights for a specific stream.
- `revoke_delegate` — revoke a delegate.
- `release_milestone`, `release_holdback`, `clawback_holdback` — milestone and holdback management.
- `archive_stream` — remove a fully-settled stream from storage.
- `batch_create_stream`, `batch_cancel_stream` — batch operations.
- `lock_stream` — irrevocably renounce the right to cancel (one-way; cannot be reversed).
- `approve_stream` — recipient-side only; senders cannot call this.

**Limitations:** The sender cannot withdraw funds on the recipient's behalf without
being granted a delegate role by the recipient.

### 4.5 Recipient

The recipient is the stream beneficiary. Like the sender, the recipient's address is
embedded in `stream.recipient` at creation and enforced on every recipient-gated call.

**Capabilities (per stream):**
- `withdraw`, `batch_withdraw` — claim accrued tokens.
- `recipient_terminate` — early termination (only if `allow_recipient_termination = true`).
- `archive_stream` — remove a fully-settled stream from storage (alongside sender).
- `transfer_recipient` — transfer recipient rights to a new address (only if `non_transferable = false`).
- `set_redirect`, `clear_redirect` — redirect withdrawal proceeds to a target stream.
- `approve_stream` — accept a stream that was created with `requires_recipient_approval = true`.

### 4.6 Delegate

A delegate is an address granted withdrawal rights for a specific stream by that
stream's sender via `set_delegate`. The delegate is stored at
`DataKey::Delegate(stream_id)` in persistent storage.

**Capabilities (for the specific stream):**
- `withdraw` — claim accrued tokens on behalf of the recipient (tokens go to the recipient, not the delegate).

**Limitations:** The delegate cannot cancel, top up, pause, or perform any other
operation. The delegate acts strictly as a withdrawal proxy.

### 4.7 Anyone (permissionless)

All query instructions are permissionless — no `require_auth()` is required. Any
address (or off-chain caller) may read:

- `get_stream`, `get_claimable`, `simulate_claimable`
- `get_all_stream_ids`, `get_streams_by_sender`, `get_streams_by_recipient`
- `get_active_streams_by_sender`, `get_active_streams_by_recipient`
- `get_stats`, `get_protocol_fee_info`, `get_admin_log`
- `is_paused`, `get_pause_expiry`, `get_guardian`, `get_governance`
- `is_fee_exempt`, `remaining_quota`, `is_blocked`, `is_participant`
- `resolve_federation`, `get_delegate`, `get_stream_health`

---

## 5. Threat Model

### 5.1 In scope

The contract is designed to resist:

- Unauthorised fund withdrawal by any party other than the designated recipient (or authorised delegate).
- Arithmetic overflow or underflow causing over-distribution.
- Reentrancy attacks during settlement callbacks.
- Denial-of-service via storage exhaustion or stream-ID collision.
- Replay attacks (duplicate stream creation with the same `nonce`).
- Fee manipulation outside the timelock window.
- Unauthorised admin-role assumption.
- Timestamp-manipulation-induced over-vesting (within validator-consensus bounds).

### 5.2 Out of scope

The contract explicitly does not protect against:

- A compromised admin key performing an immediate WASM upgrade (no timelock exists today).
- Non-SAC tokens with fee-on-transfer, rebasing, or post-creation upgradeability.
- Off-chain phishing or social engineering of users into approving malicious transactions.
- Validator-level consensus attacks (validator supermajority compromise).
- Frontend or indexer manipulation that does not affect on-chain state.
- Wallet key compromise of any individual sender or recipient.
- Economic attacks (e.g. griefing via stream-slot exhaustion up to the global cap).

### 5.3 Attack vectors

The table below summarises the most significant attack vectors, their current
mitigations, and residual risks. For complete details, refer to
[`docs/THREAT_MODEL.md`](./THREAT_MODEL.md).

| Vector | Mitigation | Residual Risk |
|--------|-----------|---------------|
| **Arithmetic overflow** | Kani-verified `claimable ≤ deposit`; all arithmetic uses `checked_*` operations | Inputs exceeding Kani symbolic bounds (>10-year streams, >100,000 XLM deposits) are not exhaustively proved |
| **Reentrancy** | Checks-effects-interactions pattern; per-stream reentrancy guard; `try_invoke_contract` for callbacks | If a transaction panics after acquiring the lock but before releasing it, the stream becomes permanently locked — guard release is verified in all code paths |
| **Admin key compromise** | 7-day fee-change timelock; auto-expiring pause; on-chain audit log; guardian separation | Immediate WASM upgrade has no timelock; treasury redirection takes effect immediately |
| **Non-SAC or malicious token** | `docs/ADDING_TOKENS.md` plus the mandatory token allowlist | Stream creation rejects token addresses that have not been explicitly approved by the admin |
| **Storage exhaustion / DoS** | Global stream cap (`set_max_streams`); per-sender cap (`set_sender_stream_limit`); paginated queries | Well-funded attacker can fill slots to the cap; cap must be monitored |
| **Replay / duplicate stream** | Per-sender nonce registry; `DuplicateStream` error on reuse | Nonces must be managed carefully in batch creation workflows |
| **MEV / transaction ordering** | No AMM or price-sensitive operations; `withdraw` amount is purely time-based | Reordering a `cancel` ahead of a `withdraw` is benign — both paths are correct |
| **Frontend manipulation** | All auth enforced on-chain; contract addresses published in `deployments/` | Social engineering cannot be prevented on-chain |
| **Timestamp manipulation** | Monotonic consensus-enforced timestamps; Kani proofs valid for all `u64` timestamps | Single-second rounding affects high-value short streams; cliff periods mitigate this |
| **WASM upgrade** | Admin auth required; events emitted on upgrade | No timelock — planned mitigation (see Section 10) |
| **Cross-contract callback** | `try_invoke_contract` ignores callback failures; reentrancy guard active during callback | Callback consuming excessive compute increases recipient transaction fees |

---

## 6. What the Contract Protects Against

**User funds in active streams are protected from theft without a WASM upgrade.**
Using only the instructions deployed in the current WASM, no party — including the
admin — can transfer a stream's deposit to an arbitrary address. Every fund movement
requires authorisation from the appropriate stream participant:

- Only the **recipient** (or their delegate) can call `withdraw`.
- Only the **sender** can call `cancel_stream`, `top_up`, or `partial_cancel_stream`.
- The **admin** has no instruction that directly transfers stream deposits to a third party.

**Vesting arithmetic is formally verified.** The Kani model checker has exhaustively
proved that within realistic parameter bounds:
- `claimable ≤ deposit` at any point in time.
- `claimable` is non-decreasing — a recipient's entitlement never shrinks.
- `claimable = 0` before `cliff_time`.
- `total_streamed + refund = deposit` on cancellation (balance conservation).

**Reentrancy is blocked.** A malicious recipient contract cannot call `withdraw` again
before the first invocation completes. State updates happen before token transfers,
and a reentrancy guard is held for the duration of the settlement path.

**Replay attacks are blocked.** Each `(sender, nonce)` pair can create at most one
stream. Used nonces are recorded permanently. Soroban's native sequence numbers
additionally prevent replaying the same ledger transaction.

**Fee increases are time-locked.** The `propose_fee_change` / `execute_fee_change`
path enforces a 7-day wait, giving users advance notice before any fee increase takes
effect.

**Pauses are bounded.** `emergency_pause` sets an expiry at
`now + MAX_PAUSE_DURATION`. After expiry, the contract auto-unpauses. No single admin
action can permanently freeze user funds.

---

## 7. What the Contract Does Not Protect Against

**Admin WASM upgrade (no timelock today).** The most significant risk. A compromised
or malicious admin can call `upgrade` to deploy arbitrary bytecode in a single
transaction. This immediately affects all funds under management. There is no
on-chain delay. An upgrade timelock is the highest-priority planned mitigation.

**Non-SAC token risks.** Any Stellar address can be passed as `token`. A token that
charges a transfer fee, rebases balances, or is later upgraded to malicious logic can
break vesting invariants for streams using that token. The impact is limited to streams
of that specific token — it does not affect streams of other tokens.

**Social engineering and phishing.** A user tricked into signing a transaction to a
different contract or with unexpected parameters bears that loss individually. The
contract cannot verify user intent beyond the signed transaction.

**Economic griefing up to the stream cap.** A well-funded attacker can create streams
up to the global cap, temporarily preventing other users from creating new streams.
This is rate-limited by token cost and the admin can raise the cap in response.

**Validator consensus compromise.** A supermajority of Stellar validators colluding to
manipulate ledger state, timestamps, or WASM execution invalidates all on-chain
security guarantees simultaneously. This is an infrastructure risk beyond the scope
of smart-contract security.

---

## 8. Invariants

The following invariants must hold after `initialize` completes. Violation of any
invariant is a critical security defect.

| # | Invariant |
|---|-----------|
| **I1** | `deposit ≥ 0` for every stored stream. |
| **I2** | `start_time ≤ cliff_time ≤ end_time` for every stored stream. |
| **I3** | `start_time ≤ last_withdraw_time ≤ end_time` for every stored stream. |
| **I4** | `total_withdrawn ≤ deposit` for every stored stream. |
| **I5** | `flow_rate = deposit / (end_time - start_time)` (approximate; remainder is dust returned on cancel). |
| **I6** | `compute_claimable(stream, now) ≤ stream.deposit` for all valid `now` values. (Kani-verified.) |
| **I7** | `compute_claimable(stream, t2) ≥ compute_claimable(stream, t1)` whenever `t2 ≥ t1`. (Kani-verified.) |
| **I8** | `compute_claimable(stream, t) = 0` for all `t < cliff_time`. (Kani-verified.) |
| **I9** | `total_streamed + refund = deposit` on `cancel_stream`. (Kani-verified.) |
| **I10** | No stream ID is ever reused after a stream is removed from storage. |
| **I11** | `withdraw`, `cancel_stream`, and `top_up` always revert if `is_paused_or_auto_unpause()` returns `true`. |
| **I12** | The reentrancy guard (`stream.locked`) is always cleared before any code path exits from `withdraw`. |
| **I13** | `fee_bps ≤ 10_000` at all times. |

---

## 9. Verification and Testing

### Formal verification (Kani)

Pure vesting-math functions in `contracts/stream/src/vesting_math.rs` are formally
verified by the [Kani model checker](https://model-checking.github.io/kani/). Kani
generates symbolic inputs and uses SAT/SMT solving to exhaustively check invariants
I6–I9. Verification runs automatically in CI.

Covered bounds: deposit up to 1,000,000,000,000 stroops; duration up to 315,360,000
seconds (10 years); flow rate up to 1,000,000,000 stroops/second.

### Property-based testing (proptest)

`contracts/stream/src/proptest_tests.rs` runs 10,000+ random-input iterations per
property across the full Soroban VM, verifying balance conservation, monotonic
withdrawal, and state-machine validity.

### Differential fuzzing

`contracts/stream/src/differential_fuzz.rs` runs 1,000,000 iterations comparing the
contract's vesting math against an independent reference implementation. Any divergence
greater than 1 stroop fails the test.

### Integration tests

`contracts/stream/src/integration_tests.rs` deploys the stream contract alongside a
real SAC token contract and tests the full lifecycle including treasury fees,
auto-renewal, and partial cancellation balance conservation.

### On-chain audit log

All admin actions are written to a circular on-chain audit log (readable via
`get_admin_log`) and emit `AdminAction` events. Off-chain monitors can detect
unexpected admin activity in real time.

---

## 10. Governance Roadmap

The following mitigations are planned to reduce the trust surface. They are tracked
as issues in this repository.

| Mitigation | Target | Priority | Status |
|------------|--------|----------|--------|
| **Multi-sig admin** | Replace the single-key admin EOA with a 2-of-3 (or N-of-M) multisig using `contracts/multisig/` | Critical | Planned |
| **Upgrade timelock** | Require a mandatory 7-day delay before any WASM upgrade takes effect, giving users time to withdraw | Critical | Planned |
| **On-chain token allowlist** | Restrict `create_stream` to a contract-enforced list of approved SAC tokens | High | Planned |
| **DAO governance** | Transfer admin role to `contracts/governance/` so fee and upgrade proposals require token-holder votes | High | Planned |
| **Guardian full implementation** | Complete the guardian role with on-chain scope restrictions (pause-only, no other capabilities) | Medium | In progress |
| **Admin rotation audit** | Require on-chain evidence of multisig quorum before `set_admin` can execute | Medium | Planned |
| **Formal verification expansion** | Extend Kani proofs to cover streaming edge cases above current symbolic bounds | Low | Planned |

Until multi-sig is deployed, the admin key must be kept in cold storage. The guardian
key must be stored separately from the admin key so that a single security incident
cannot compromise both.

---

## 11. Related Documents

| Document | Content |
|----------|---------|
| [`SECURITY.md`](../SECURITY.md) | Admin capability matrix; formally verified properties; Kani proof bounds |
| [`docs/THREAT_MODEL.md`](./THREAT_MODEL.md) | Full attack-vector analysis with per-vector residual risk ratings |
| [`docs/permissions.md`](./permissions.md) | Complete authorisation matrix for every instruction |
| [`docs/CONTRACT_SPEC.md`](./CONTRACT_SPEC.md) | Data model, state machine, error reference, and instruction reference |
| [`ARCHITECTURE.md`](../ARCHITECTURE.md) | Stream lifecycle state machine; contract system diagram; stream-ID generation |
| [`docs/STORAGE.md`](./STORAGE.md) | Persistent vs. temporary storage trade-offs |
| [`docs/events.md`](./events.md) | Full event schema for off-chain monitoring |
| [`docs/ADDING_TOKENS.md`](./ADDING_TOKENS.md) | Due-diligence checklist for adding new tokens |
