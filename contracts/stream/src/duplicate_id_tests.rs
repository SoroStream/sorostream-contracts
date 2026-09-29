/// Duplicate stream ID collision test
///
/// Two streams created with identical sender, recipient, token, amount, and
/// timing parameters in the **same simulated ledger** must:
///
/// 1. Produce different stream IDs.
/// 2. Both be independently retrievable via `get_stream`.
/// 3. Both be independently claimable / cancellable without interference.
///
/// Without this guard an ID collision could silently overwrite the first stream.
///
/// The contract already handles this via its nonce-based ID derivation and the
/// retry loop in `create_stream`.  These tests exercise that path with distinct
/// nonce values in the same ledger.
#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ── helpers ───────────────────────────────────────────────────────────────────

struct T {
    env:         Env,
    contract_id: Address,
    token_id:    Address,
    sender:      Address,
    recipient:   Address,
}

fn setup() -> T {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id    = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender    = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    T { env, contract_id, token_id, sender, recipient }
}

fn client(t: &T) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn params(nonce: u64) -> crate::types::CreateStreamParams {
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
    }
}

// ── Test 1 ─────────────────────────────────────────────────────────────────────
/// Two streams created with identical parameters in the same ledger must
/// produce different IDs.
///
/// We use two different nonce values (0 and 1) — the same `create_stream` call
/// site mandates that callers supply distinct nonces precisely to avoid this.
/// The test confirms the contract honours that guarantee end-to-end.
#[test]
fn test_identical_params_same_ledger_produce_different_ids() {
    let t = setup();
    let c = client(&t);

    // Fix the ledger so both calls share exactly the same timestamp.
    t.env.ledger().set_timestamp(1_000);

    let id_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(0), // nonce 0
    );

    // Same timestamp — the ledger has not advanced.
    let id_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(1), // nonce 1 — distinct nonce → distinct ID
    );

    assert_ne!(id_a, id_b, "two streams with different nonces must have different IDs");
}

// ── Test 2 ─────────────────────────────────────────────────────────────────────
/// Both streams created in the same ledger are independently retrievable.
#[test]
fn test_both_streams_independently_retrievable() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(500);

    let id_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &20_000, &2_000, &false,
        &params(0),
    );
    let id_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &20_000, &2_000, &false,
        &params(1),
    );

    // Both must be retrievable.
    let stream_a = c.get_stream(&id_a);
    let stream_b = c.get_stream(&id_b);

    // IDs must match what was returned.
    assert_eq!(stream_a.id, id_a);
    assert_eq!(stream_b.id, id_b);

    // They are independent objects even though all parameters match.
    assert_ne!(stream_a.id, stream_b.id);

    // Both have the correct deposit.
    assert_eq!(stream_a.deposit, 20_000);
    assert_eq!(stream_b.deposit, 20_000);
}

// ── Test 3 ─────────────────────────────────────────────────────────────────────
/// Both streams are independently claimable: withdrawing from A does not affect B.
#[test]
fn test_both_streams_independently_claimable() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(0);

    // flow_rate = 10_000 / 1_000 = 10 tokens/s
    let id_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(0),
    );
    let id_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(1),
    );

    // Advance 100 s — each stream should have 1_000 tokens claimable.
    t.env.ledger().set_timestamp(100);

    let claimable_a = c.get_claimable(&id_a);
    let claimable_b = c.get_claimable(&id_b);

    assert_eq!(claimable_a, 1_000, "stream A: 100s * 10/s = 1000 claimable");
    assert_eq!(claimable_b, 1_000, "stream B: 100s * 10/s = 1000 claimable");

    // Withdraw from A.
    c.withdraw(&id_a, &t.recipient);

    // B must still have 1_000 claimable (A's withdrawal must not touch B).
    let claimable_b_after = c.get_claimable(&id_b);
    assert_eq!(
        claimable_b_after, 1_000,
        "stream B claimable must not be affected by stream A's withdrawal"
    );

    // A should now have 0 claimable (just withdrew everything up to t=100).
    let claimable_a_after = c.get_claimable(&id_a);
    assert_eq!(
        claimable_a_after, 0,
        "stream A claimable must be 0 immediately after withdrawal"
    );
}

// ── Test 4 ─────────────────────────────────────────────────────────────────────
/// Cancelling stream A must not affect stream B.
#[test]
fn test_cancel_a_does_not_affect_b() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(0);

    let id_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(0),
    );
    let id_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params(1),
    );

    t.env.ledger().set_timestamp(200);

    // Cancel A.
    c.cancel_stream(&id_a, &t.sender);

    // B must still be active with the correct deposit.
    let stream_b = c.get_stream(&id_b);
    assert_eq!(
        stream_b.status, crate::types::StreamStatus::Active,
        "stream B must still be Active after A is cancelled"
    );
    assert_eq!(stream_b.deposit, 10_000);

    // Stream B is still claimable (200 s elapsed → 2_000 tokens).
    let claimable_b = c.get_claimable(&id_b);
    assert_eq!(claimable_b, 2_000, "stream B must have 2000 claimable at t=200");
}

// ── Test 5 ─────────────────────────────────────────────────────────────────────
/// Three streams with identical parameters in the same ledger all get unique IDs.
#[test]
fn test_three_identical_params_unique_ids() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(42);

    let id_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &5_000, &500, &false, &params(10),
    );
    let id_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &5_000, &500, &false, &params(11),
    );
    let id_c = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &5_000, &500, &false, &params(12),
    );

    assert_ne!(id_a, id_b, "A and B must differ");
    assert_ne!(id_b, id_c, "B and C must differ");
    assert_ne!(id_a, id_c, "A and C must differ");

    // All three independently retrievable.
    assert_eq!(c.get_stream(&id_a).id, id_a);
    assert_eq!(c.get_stream(&id_b).id, id_b);
    assert_eq!(c.get_stream(&id_c).id, id_c);
}
