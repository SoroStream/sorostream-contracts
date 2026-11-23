//! Tests for Issue #466: subscription mode.
//!
//! A subscription automatically starts a new cycle of the same rate and
//! duration when the current one completes, drawing the next cycle's
//! deposit from the sender's pre-approved SAC allowance — no interactive
//! re-authorization needed at renewal time, unlike the legacy `auto_renew`
//! flag. It can be cancelled by the sender at any time, refunding the
//! unused portion of the current cycle.

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient, StreamError, StreamStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&sender, &0u64);

    // Enough for several cycles.
    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    TestEnv {
        env,
        contract,
        token,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract)
}

fn balance(t: &TestEnv, who: &Address) -> i128 {
    TokenClient::new(&t.env, &t.token).balance(who)
}

fn approve(t: &TestEnv, amount: i128) {
    TokenClient::new(&t.env, &t.token).approve(
        &t.sender,
        &t.contract,
        &amount,
        &(t.env.ledger().sequence() + 1_000_000),
    );
}

// ── Creation ─────────────────────────────────────────────────────────────

#[test]
fn create_subscription_is_flagged_and_auto_renew() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );

    assert!(c.is_subscription(&stream_id));
    let stream = c.get_stream(&stream_id);
    assert!(stream.auto_renew);
    assert_eq!(stream.status, StreamStatus::Active);
}

// ── Renewal draws from allowance, no interactive re-auth needed ─────────

#[test]
fn renewal_draws_from_preapproved_allowance() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );

    // First cycle completes at t=1000; recipient claims and the renewal
    // fires within the same withdraw call.
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    assert_eq!(balance(&t, &t.recipient), 1_000_000);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.renewals_used, 1);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);

    // Second cycle.
    t.env.ledger().set_timestamp(2000);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(balance(&t, &t.recipient), 2_000_000);
    assert_eq!(c.get_stream(&stream_id).renewals_used, 2);
}

#[test]
fn renewal_stops_when_allowance_exhausted() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    // The stream's own deposit is funded directly at creation (not from the
    // allowance) — only *renewals* draw from it. Approve less than one
    // cycle's deposit so the first renewal attempt fails.
    approve(&t, 500_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    assert_eq!(balance(&t, &t.recipient), 1_000_000);
    // Stream completed instead of renewing (insufficient allowance for the
    // next cycle's deposit) — same as any other non-renewing completed
    // stream, the record persists (get_stream reports Expired once
    // now >= end_time, regardless of *why* it stopped renewing).
    assert_eq!(c.get_stream(&stream_id).status, StreamStatus::Expired);
    assert!(!c.is_subscription(&stream_id));
}

#[test]
fn renew_count_caps_subscription_renewals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &Some(1u32), &0u64,
    );

    // First renewal allowed (renewals_used 0 -> 1).
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_stream(&stream_id).renewals_used, 1);

    // Cap reached: second cycle completes without renewing. get_stream
    // reports Expired rather than Completed once now >= end_time, regardless
    // of the underlying reason the stream stopped (see refreshed_stream_view).
    t.env.ledger().set_timestamp(2000);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_stream(&stream_id).status, StreamStatus::Expired);
    assert_eq!(balance(&t, &t.recipient), 2_000_000);
}

// ── Cancellation ─────────────────────────────────────────────────────────

#[test]
fn cancel_subscription_refunds_unused_deposit() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );

    t.env.ledger().set_timestamp(300);
    c.cancel_subscription(&t.sender, &stream_id);

    // Recipient earned 300 seconds worth; sender gets the rest back.
    assert_eq!(balance(&t, &t.recipient), 300_000);
    assert_eq!(balance(&t, &t.sender), 10_000_000 - 1_000_000 + 700_000);
    assert!(c.try_get_stream(&stream_id).is_err());
    assert!(!c.is_subscription(&stream_id));
}

#[test]
fn cancel_subscription_stops_future_renewals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );
    c.cancel_subscription(&t.sender, &stream_id);

    // The stream is gone entirely — nothing left to renew.
    assert!(c.try_get_stream(&stream_id).is_err());
}

#[test]
fn cancel_subscription_rejects_non_sender() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    approve(&t, 10_000_000);

    let stream_id = c.create_subscription(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0, &0u64, &None::<u32>, &0u64,
    );

    let stranger = Address::generate(&t.env);
    let result = c.try_cancel_subscription(&stranger, &stream_id);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));
}

#[test]
fn cancel_subscription_rejects_ordinary_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    let result = c.try_cancel_subscription(&t.sender, &stream_id);
    assert_eq!(result, Err(Ok(StreamError::NotSubscription)));
}

// ── Legacy auto_renew (non-subscription) is unaffected ──────────────────

#[test]
fn ordinary_stream_is_not_a_subscription() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );
    assert!(!c.is_subscription(&stream_id));
}
