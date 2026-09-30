
//! Issue #661: storage key hashing / lookup performance.
//!
//! The only actual cryptographic hash in `storage.rs` is `derive_stream_id`'s
//! `env.crypto().sha256(...)` call, which runs once per `create_stream` to
//! derive a collision-resistant 64-bit stream ID from
//! `(sender, recipient, now, nonce)`. Swapping that for a fast, non-cryptographic
//! hash such as CRC32 (as the issue's literal example suggested) would be a
//! real security regression: CRC32 is a linear function over GF(2), so an
//! attacker can algebraically solve for a `nonce` that produces any chosen
//! stream ID — trivially grinding a collision with an existing or
//! soon-to-be-created stream — where SHA-256 requires infeasible brute force.
//! `derive_stream_id` is intentionally left untouched.
//!
//! The actual, safe, per-lookup cost this contract pays on *every* storage
//! read and write throughout `storage.rs` is `Symbol::new(env, "literal")` —
//! encoding a `&str` into a `Symbol` at runtime, 135 call sites, on every
//! single storage operation in the contract. `symbol_short!("literal")`
//! (for the many keys that fit its 9-byte limit) computes the identical
//! `Symbol` value entirely at compile time — semantically identical key,
//! zero runtime encoding cost. This is the change made for #661:
//! `storage.rs`'s 39 named key constants (36 of which fit within 9 bytes)
//! and all inline single-use key literals were converted from
//! `Symbol::new(env, "...")` to `symbol_short!("...")`, cutting the 135
//! runtime Symbol constructions down to the 6 call sites that genuinely
//! can't fit (`governance`, `migrations`, `cancel_fee` — all 10 bytes).
//!
//! Tests below verify (a) semantic equivalence — the optimization changed
//! *how* keys are constructed, never their value, so all existing storage
//! reads/writes against pre-existing keys must keep working identically —
//! and (b) provide a real, budget-instrumented cost baseline across a
//! 10,000-operation run for future regression comparison.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, IssuerFlags, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    admin: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone());
    token.issuer().set_flag(IssuerFlags::ClawbackEnabledFlag);
    let token_id = token.address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        admin,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn default_params(nonce: u64) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
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
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Semantic equivalence: symbol_short! keys read/write the same storage as
// the Symbol::new(env, ...) keys they replaced.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_661_admin_governance_and_pause_state_roundtrip_correctly() {
    let t = setup();
    let c = client(&t);

    // ADMIN_KEY (converted) — set during initialize(), read back correctly.
    // (No direct getter; exercised indirectly via emergency_pause's check_admin.)
    c.emergency_pause();
    assert!(!c.is_paused()); // PAUSED_KEY + PAUSE_EFFECTIVE_LEDGER_KEY (both converted)

    let starting_ledger = t.env.ledger().sequence();
    t.env.ledger().with_mut(|li| li.sequence_number = starting_ledger + 1);
    assert!(c.is_paused());

    c.emergency_resume();
    assert!(!c.is_paused());

    // GOVERNANCE_KEY (kept as runtime Symbol::new — 10 bytes) still roundtrips.
    let governance = Address::generate(&t.env);
    c.set_governance(&governance);
    assert_eq!(c.get_governance(), Some(governance));
}

#[test]
fn test_issue_661_stream_lifecycle_storage_keys_still_work() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Exercises STREAM_COUNT_KEY, the "gi" global-index tuple key, and the
    // plain stream_id key (unaffected — already a bare u64, not a Symbol).
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(0),
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id);
    assert_eq!(c.get_active_stream_count(), 1);
}

// ─────────────────────────────────────────────────────────────────────────
// Cost baseline: 10,000 operations, budget-instrumented.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_661_ten_thousand_storage_reads_cost_baseline() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(0),
    );

    t.env.budget().reset_unlimited();
    const OPS: u32 = 10_000;
    for _ in 0..OPS {
        // Each call reads PAUSED_KEY + PAUSE_EFFECTIVE_LEDGER_KEY (both
        // symbol_short! constants) plus the plain stream_id key — the exact
        // storage-key-construction path #661 targeted.
        let _ = c.is_paused();
        let _ = c.get_stream(&stream_id);
    }
    let cpu = t.env.budget().cpu_instruction_cost();

    // Not a hard regression gate (no prior-version baseline to compare
    // against in this same crate) — this recorded number is the reference
    // point for any future regression check on this storage-key path.
    std::println!(
        "issue_661: {OPS} x (is_paused + get_stream) = {cpu} cpu instructions ({} avg/op)",
        cpu / (OPS as u64)
    );
    assert!(cpu > 0);
}
