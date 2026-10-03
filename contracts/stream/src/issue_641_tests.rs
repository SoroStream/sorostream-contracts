//! Tests for issue #641: StreamExpiryNotification event.
//!
//! Validates that:
//! - StreamExpiryNotification is emitted when end_time - current_ledger <= 10.
//! - Event includes stream_id and ledgers_until_expiry.
//! - Event is NOT emitted when more than 10 ledgers remain.
//! - Event is emitted at most once per stream (idempotent guard).

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::StellarAssetClient,
    Address, Env, Symbol,
};

struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_641() -> Setup {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
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

fn make_stream(s: &Setup, amount: i128, duration_seconds: u64, nonce: u64) -> u64 {
    let c = SoroStreamContractClient::new(&s.env, &s.contract);
    c.create_stream(
        &s.sender,
        &s.recipient,
        &s.token,
        &amount,
        &duration_seconds,
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

/// Helper: count events with topic "StreamExpiryNotif" for a stream.
fn count_notification_events(env: &Env, stream_id: u64) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|(_, topics, _)| {
            topics.get(0)
                .and_then(|v| v.try_into_val(env).ok())
                .map(|s: Symbol| s == Symbol::new(env, "StreamExpiryNotif"))
                .unwrap_or(false)
            && topics.get(1)
                .and_then(|v| v.try_into_val(env).ok())
                .map(|id: u64| id == stream_id)
                .unwrap_or(false)
        })
        .count()
}

/// StreamExpiryNotification is emitted on withdraw when stream is within 10 ledgers.
///
/// A ledger is 5 seconds; 10 ledgers = 50 seconds. The stream is created with
/// a 200-second duration (40 ledgers). We advance 190 seconds (38 ledgers elapsed,
/// 2 ledgers remaining — well within the 10-ledger threshold) then withdraw.
#[test]
fn test_641_notification_emitted_within_10_ledgers_on_withdraw() {
    let s = setup_641();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // flow_rate = 1000 / 200 = 5 tokens/sec — need cliff=0 so we get claimable.
    // Use a nice amount divisible by duration: 1000 tokens / 200 sec = 5/sec.
    let stream_id = make_stream(&s, 1000, 200, 1);

    // Advance to t=190 (10 seconds = 2 ledgers remaining before end_time=200).
    s.env.ledger().set_timestamp(190);

    // Withdraw — this should trigger the notification because 10s / 5s = 2 ledgers <= 10.
    c.withdraw(&stream_id, &s.recipient).unwrap();

    assert_eq!(
        count_notification_events(&s.env, stream_id),
        1,
        "StreamExpiryNotif must be emitted once when within 10 ledgers of expiry"
    );
}

/// StreamExpiryNotification is NOT emitted when more than 10 ledgers remain.
///
/// 10 ledgers = 50 seconds. We withdraw at t=100 (100 seconds remaining = 20 ledgers).
#[test]
fn test_641_notification_not_emitted_when_far_from_expiry() {
    let s = setup_641();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // duration = 300 seconds (60 ledgers). Withdraw at t=100 (200s = 40 ledgers remaining).
    let stream_id = make_stream(&s, 3000, 300, 2);
    s.env.ledger().set_timestamp(100);

    c.withdraw(&stream_id, &s.recipient).unwrap();

    assert_eq!(
        count_notification_events(&s.env, stream_id),
        0,
        "StreamExpiryNotif must NOT be emitted when more than 10 ledgers remain"
    );
}

/// StreamExpiryNotification is emitted at most once per stream.
///
/// Two successive withdrawals within the 10-ledger window must produce only
/// one notification event, not two.
#[test]
fn test_641_notification_emitted_at_most_once() {
    let s = setup_641();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // duration = 200 sec; withdraw twice in the last 50 seconds.
    let stream_id = make_stream(&s, 1000, 200, 3);

    // First withdraw at t=155 (45 seconds remaining = 9 ledgers — within threshold).
    s.env.ledger().set_timestamp(155);
    c.withdraw(&stream_id, &s.recipient).unwrap();

    let after_first = count_notification_events(&s.env, stream_id);

    // Second withdraw at t=170 (30 seconds remaining = 6 ledgers — still within threshold).
    s.env.ledger().set_timestamp(170);
    c.withdraw(&stream_id, &s.recipient).unwrap();

    let after_second = count_notification_events(&s.env, stream_id);

    assert_eq!(after_first, 1, "notification must fire on first withdraw within window");
    assert_eq!(
        after_second,
        1,
        "notification must NOT fire again on second withdraw within window"
    );
}

/// StreamExpiryNotification is emitted on top_up when within 10 ledgers.
#[test]
fn test_641_notification_emitted_on_top_up_within_10_ledgers() {
    let s = setup_641();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // duration = 200 sec; top_up at t=190 (10 sec = 2 ledgers remaining).
    let stream_id = make_stream(&s, 1000, 200, 4);

    s.env.ledger().set_timestamp(190);

    // Top up with a small amount (must be >= flow_rate = 5 tokens/sec).
    c.top_up(&stream_id, &s.sender, &s.token, &50).unwrap();

    assert_eq!(
        count_notification_events(&s.env, stream_id),
        1,
        "StreamExpiryNotif must be emitted on top_up within 10 ledgers of expiry"
    );
}
