//! Tests for issue #633: get_active_stream_count entry point.
//!
//! Validates that:
//! - Count starts at 0 before any stream is created.
//! - Count increments by 1 for each created stream.
//! - Count decrements by 1 when a stream is cancelled.
//! - Count decrements by 1 when a stream expires naturally.
//! - Return type is u64.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_633() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);

    Setup { env, contract, token, sender, recipient }
}

fn make_stream(s: &Setup, amount: i128, duration: u64, nonce: u64) -> u64 {
    let c = SoroStreamContractClient::new(&s.env, &s.contract);
    c.create_stream(
        &s.sender,
        &s.recipient,
        &s.token,
        &amount,
        &duration,
        &false,
        &0,
        &CreateStreamParams {
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
        },
    )
    .unwrap()
}

/// Protocol starts with zero active streams.
#[test]
fn test_633_initial_count_is_zero() {
    let s = setup_633();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let count: u64 = c.get_active_stream_count();
    assert_eq!(count, 0u64, "initial active stream count must be 0");
}

/// Creating streams increments the count by 1 per stream.
#[test]
fn test_633_count_increments_on_create() {
    let s = setup_633();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    assert_eq!(c.get_active_stream_count(), 0u64);

    make_stream(&s, 100_000, 1000, 1);
    assert_eq!(c.get_active_stream_count(), 1u64, "after first create: count must be 1");

    make_stream(&s, 100_000, 1000, 2);
    assert_eq!(c.get_active_stream_count(), 2u64, "after second create: count must be 2");

    make_stream(&s, 100_000, 1000, 3);
    assert_eq!(c.get_active_stream_count(), 3u64, "after third create: count must be 3");
}

/// Cancelling a stream decrements the active count.
#[test]
fn test_633_count_decrements_on_cancel() {
    let s = setup_633();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let id1 = make_stream(&s, 100_000, 1000, 10);
    let id2 = make_stream(&s, 100_000, 1000, 11);
    assert_eq!(c.get_active_stream_count(), 2u64);

    c.cancel_stream(&id1, &s.sender).unwrap();
    assert_eq!(
        c.get_active_stream_count(),
        1u64,
        "cancelling one stream must reduce count to 1"
    );

    c.cancel_stream(&id2, &s.sender).unwrap();
    assert_eq!(
        c.get_active_stream_count(),
        0u64,
        "cancelling last stream must reduce count to 0"
    );
}

/// Marking a stream expired (post end_time) decrements the active count.
#[test]
fn test_633_count_decrements_on_expiry() {
    let s = setup_633();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // Create a short-lived stream with duration = 100 seconds.
    let stream_id = make_stream(&s, 100_000, 100, 20);
    assert_eq!(c.get_active_stream_count(), 1u64);

    // Advance time past the stream's end_time.
    s.env.ledger().set_timestamp(200);

    // mark_expired records the transition and decrements the counter.
    c.mark_expired(&stream_id).unwrap();

    assert_eq!(
        c.get_active_stream_count(),
        0u64,
        "expiring the stream must reduce count to 0"
    );
}

/// The return value type is u64 (not u32), verified by explicitly binding to u64.
#[test]
fn test_633_return_type_is_u64() {
    let s = setup_633();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);
    // This will fail to compile if get_active_stream_count returns a non-u64 type.
    let _count: u64 = c.get_active_stream_count();
}
