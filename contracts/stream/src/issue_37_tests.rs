/// feat/37-sender-stream-cap
///
/// Tests for the per-sender active stream cap:
///
/// 1. Creating (cap + 1) streams is rejected with `SenderStreamCapReached`.
/// 2. The cap is stored in persistent storage and configurable by the admin.
/// 3. Cancelling a stream decrements the active count, freeing space.
/// 4. A stream completing (withdrawing past end_time) also decrements the count.
/// 5. Setting the cap to 0 means unlimited (default safety guard).
#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
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
    admin:       Address,
    sender:      Address,
}

fn setup() -> T {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let admin  = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000_000);

    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    T { env, contract_id, token_id, admin, sender }
}

fn client(t: &T) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn make_params(nonce: u64) -> crate::types::CreateStreamParams {
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

/// Create one stream from `sender` to a fresh recipient and return its ID.
fn create_one(t: &T, nonce: u64) -> u64 {
    let c         = client(t);
    let recipient = Address::generate(&t.env);
    c.create_stream(
        &t.sender, &recipient, &t.token_id,
        &10_000, &1_000, &false,
        &make_params(nonce),
    )
}

// ── Test 1 ─────────────────────────────────────────────────────────────────────
/// Creating `cap + 1` streams is rejected with `SenderStreamCapReached`.
#[test]
fn test_cap_plus_one_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Set cap to 3.
    const CAP: u32 = 3;
    c.set_sender_cap(&CAP);
    assert_eq!(c.get_sender_cap(), CAP);

    // Create exactly `CAP` streams — all must succeed.
    for nonce in 0..CAP {
        create_one(&t, nonce as u64);
    }

    assert_eq!(c.get_sender_active_stream_count(&t.sender), CAP);

    // The (cap + 1)-th stream must be rejected.
    let recipient = Address::generate(&t.env);
    let result = c.try_create_stream(
        &t.sender, &recipient, &t.token_id,
        &10_000, &1_000, &false,
        &make_params(CAP as u64),
    );

    assert!(result.is_err(), "cap+1 stream must be rejected");
    assert_eq!(
        result.unwrap_err().unwrap_err(),
        StreamError::SenderStreamCapReached,
        "expected SenderStreamCapReached",
    );
}

// ── Test 2 ─────────────────────────────────────────────────────────────────────
/// The cap is configurable by the admin and stored persistently.
#[test]
fn test_cap_is_configurable() {
    let t = setup();
    let c = client(&t);

    // Default cap is 1000.
    assert_eq!(c.get_sender_cap(), 1_000);

    // Admin raises it.
    c.set_sender_cap(&5_000);
    assert_eq!(c.get_sender_cap(), 5_000);

    // Admin lowers it.
    c.set_sender_cap(&2);
    assert_eq!(c.get_sender_cap(), 2);
}

// ── Test 3 ─────────────────────────────────────────────────────────────────────
/// Cancelling a stream decrements the active count, freeing a slot.
#[test]
fn test_cancel_decrements_count() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Set cap to 2.
    c.set_sender_cap(&2);

    let id0 = create_one(&t, 0);
    let id1 = create_one(&t, 1);

    assert_eq!(c.get_sender_active_stream_count(&t.sender), 2);

    // A third stream must be rejected at this point.
    {
        let recipient = Address::generate(&t.env);
        let r = c.try_create_stream(
            &t.sender, &recipient, &t.token_id,
            &10_000, &1_000, &false, &make_params(2),
        );
        assert!(r.is_err(), "third stream must be rejected while count == cap");
    }

    // Cancel one stream.
    c.cancel_stream(&id0, &t.sender);
    assert_eq!(c.get_sender_active_stream_count(&t.sender), 1,
        "count must drop to 1 after cancel");

    // Now a new stream must be accepted.
    let id2 = create_one(&t, 3);
    assert_eq!(c.get_sender_active_stream_count(&t.sender), 2,
        "count must be 2 after creating a replacement");

    // Clean up.
    c.cancel_stream(&id1, &t.sender);
    c.cancel_stream(&id2, &t.sender);
    assert_eq!(c.get_sender_active_stream_count(&t.sender), 0);
}

// ── Test 4 ─────────────────────────────────────────────────────────────────────
/// Natural stream expiry (withdraw past end_time) decrements the active count.
#[test]
fn test_expiry_decrements_count() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Cap = 1 so the second creation is blocked until the first expires.
    c.set_sender_cap(&1);

    let recipient = Address::generate(&t.env);
    let stream_id = c.create_stream(
        &t.sender, &recipient, &t.token_id,
        &10_000, &1_000, &false, &make_params(0),
    );

    assert_eq!(c.get_sender_active_stream_count(&t.sender), 1);

    // Advance past end_time and withdraw (completes the stream).
    t.env.ledger().set_timestamp(2_000);
    c.withdraw(&stream_id, &recipient);

    assert_eq!(
        c.get_sender_active_stream_count(&t.sender), 0,
        "count must drop to 0 after stream completes",
    );

    // The sender can now create another stream.
    let recipient2 = Address::generate(&t.env);
    let r = c.try_create_stream(
        &t.sender, &recipient2, &t.token_id,
        &10_000, &1_000, &false, &make_params(1),
    );
    assert!(r.is_ok(), "new stream must be accepted after previous expired");
}

// ── Test 5 ─────────────────────────────────────────────────────────────────────
/// Cap = 0 means unlimited — large number of streams succeeds.
#[test]
fn test_cap_zero_means_unlimited() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Default cap is 1000; temporarily lower to test the zero sentinel.
    // (We don't test 1000 streams as that would be slow; instead set to 5 and
    //  then set to 0 to verify the zero path disables enforcement.)
    c.set_sender_cap(&3);

    // Fill to cap.
    for n in 0u64..3 {
        create_one(&t, n);
    }
    // Confirm cap is enforced.
    {
        let recipient = Address::generate(&t.env);
        let r = c.try_create_stream(
            &t.sender, &recipient, &t.token_id,
            &10_000, &1_000, &false, &make_params(100),
        );
        assert!(r.is_err(), "must be blocked at cap=3");
    }

    // Set cap to 0 — unlimited.
    c.set_sender_cap(&0);
    assert_eq!(c.get_sender_cap(), 0);

    // Now creation must be allowed even though active count is 3.
    let recipient = Address::generate(&t.env);
    let r = c.try_create_stream(
        &t.sender, &recipient, &t.token_id,
        &10_000, &1_000, &false, &make_params(200),
    );
    assert!(r.is_ok(), "cap=0 should mean unlimited");
}

// ── Test 6 ─────────────────────────────────────────────────────────────────────
/// Active count accurately tracks creates and cancels across multiple senders.
#[test]
fn test_count_is_per_sender() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let sender2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&sender2, &100_000_000);

    c.set_sender_cap(&2);

    // sender creates 2 streams.
    let id_a = create_one(&t, 0);
    let id_b = create_one(&t, 1);

    // sender2 creates 1 stream independently.
    let recipient = Address::generate(&t.env);
    let id_c = c.create_stream(
        &sender2, &recipient, &t.token_id,
        &10_000, &1_000, &false, &make_params(0),
    );

    assert_eq!(c.get_sender_active_stream_count(&t.sender),  2);
    assert_eq!(c.get_sender_active_stream_count(&sender2),   1);

    // Cancel sender's stream — only sender's count drops.
    c.cancel_stream(&id_a, &t.sender);
    assert_eq!(c.get_sender_active_stream_count(&t.sender),  1);
    assert_eq!(c.get_sender_active_stream_count(&sender2),   1);

    c.cancel_stream(&id_b, &t.sender);
    c.cancel_stream(&id_c, &sender2);

    assert_eq!(c.get_sender_active_stream_count(&t.sender),  0);
    assert_eq!(c.get_sender_active_stream_count(&sender2),   0);
}
