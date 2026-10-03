//! Tests for issue #624 — No expiry check on stream creation (past-expired streams).
//!
//! Acceptance criteria:
//!   • `create_stream_scheduled` panics with `StreamError::EndTimeInPast` when
//!     `start_time + duration_seconds <= current_ledger_timestamp`.
//!   • A valid scheduled stream (end_time strictly in the future) is still accepted.

#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let client = SoroStreamContractClient::new(&env, &contract_id);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    client.set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

// ─── Issue #624 tests ───────────────────────────────────────────────────────

/// A scheduled stream where `start_time + duration_seconds` is strictly in the
/// past must be rejected with `EndTimeInPast`.
#[test]
#[should_panic]
fn test_624_scheduled_stream_past_end_time_rejected() {
    let t = setup();
    let c = client(&t);

    // Advance ledger to t=2000 so that a stream ending at t=999 is in the past.
    t.env.ledger().set_timestamp(2_000);

    // start_time=500, duration=499 → end_time=999, which is < now(2000).
    let result = c.try_create_stream_scheduled(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &499u64,      // duration_seconds
        &500u64,      // start_time — but end_time = 999 < now(2000)
        &0u64,        // cliff_seconds
        &42u64,       // nonce
        &false,       // auto_renew
        &None,        // renew_count
    );

    // Must fail with EndTimeInPast.
    assert_eq!(
        result.err().unwrap().unwrap(),
        StreamError::EndTimeInPast,
        "Expected EndTimeInPast when end_time <= current ledger timestamp"
    );
}

/// A scheduled stream whose end_time equals the current ledger timestamp must
/// also be rejected (it expires the instant it is created).
#[test]
#[should_panic]
fn test_624_scheduled_stream_end_time_equal_now_rejected() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(1_000);

    // start_time=0, duration=1000 → end_time=1000 == now(1000).
    let result = c.try_create_stream_scheduled(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &1_000u64,    // duration_seconds
        &0u64,        // start_time
        &0u64,        // cliff_seconds
        &43u64,       // nonce
        &false,
        &None,
    );

    assert_eq!(
        result.err().unwrap().unwrap(),
        StreamError::EndTimeInPast,
        "Expected EndTimeInPast when end_time == current ledger timestamp"
    );
}

/// A scheduled stream with a valid future end_time must be accepted normally.
#[test]
fn test_624_scheduled_stream_future_end_time_accepted() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(1_000);

    // start_time=1000, duration=5000 → end_time=6000 > now(1000). Valid.
    let stream_id = c.create_stream_scheduled(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &5_000u64,    // duration_seconds
        &1_000u64,    // start_time == now
        &0u64,        // cliff_seconds
        &44u64,       // nonce
        &false,
        &None,
    );

    let stream = c.get_stream(&stream_id);
    assert!(
        stream.end_time > 1_000,
        "Stream end_time must be strictly in the future"
    );
}
