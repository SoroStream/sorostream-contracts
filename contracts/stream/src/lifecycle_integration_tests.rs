/// Lifecycle integration tests — issue #41
///
/// These tests simulate realistic multi-ledger scenarios using
/// `env.ledger().set_timestamp()` to advance time.  Each test is fully
/// end-to-end: mint → create → advance → withdraw/cancel/top_up → assert.
///
/// Branch: feat/41-lifecycle-integration-tests

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::types::{CreateStreamParams, StreamStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ── Shared helpers ────────────────────────────────────────────────────────────

struct Ctx {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> Ctx {
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

    // Disable the minimum-duration guard so we can use short durations in tests.
    SoroStreamContractClient::new(&env, &contract).set_min_duration(&sender, &0u64);

    Ctx { env, contract, token, sender, recipient }
}

fn client(ctx: &Ctx) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&ctx.env, &ctx.contract)
}

fn mint(ctx: &Ctx, to: &Address, amount: i128) {
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(to, &amount);
}

fn balance(ctx: &Ctx, who: &Address) -> i128 {
    TokenClient::new(&ctx.env, &ctx.token).balance(who)
}

/// Minimal no-frills CreateStreamParams.
fn bare_params(nonce: u64) -> CreateStreamParams {
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
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #1 — advance 10 ledger steps, withdraw at each step, cumulative
//            balance must equal flow_rate × elapsed at every checkpoint.
// ─────────────────────────────────────────────────────────────────────────────
//
// Stream: amount=10_000, duration=1000 s → flow_rate = 10 stroops/s
// We advance in 100-second steps, withdrawing at each step.
// After step k the recipient should have received exactly 100 * k * flow_rate.
//
#[test]
fn lifecycle_withdraw_at_each_ledger_step_matches_cumulative_flow() {
    let ctx = setup();
    let c = client(&ctx);

    // 10_000 / 1000 s = 10 stroops/s
    let amount: i128 = 10_000;
    let duration: u64 = 1_000;
    let flow_rate: i128 = amount / duration as i128; // = 10

    mint(&ctx, &ctx.sender, amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    let mut total_withdrawn: i128 = 0;

    for step in 1u64..=10 {
        let now = step * 100; // 100, 200, … 1000
        ctx.env.ledger().set_timestamp(now);
        c.withdraw(&stream_id, &ctx.recipient);

        let expected_cumulative = flow_rate * now as i128;
        total_withdrawn = balance(&ctx, &ctx.recipient);

        assert_eq!(
            total_withdrawn, expected_cumulative,
            "after step {} (t={}) expected cumulative={} got={}",
            step, now, expected_cumulative, total_withdrawn
        );
    }

    // Stream ends at t=1000; full deposit must have flowed to recipient.
    assert_eq!(total_withdrawn, amount);
    // Stream must have been cleaned up after the final withdrawal.
    assert!(c.try_get_stream(&stream_id).is_err(), "stream must be removed after completion");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #2 — cancel mid-stream: recipient receives earned portion,
//            sender receives refund of the unstreamed remainder.
// ─────────────────────────────────────────────────────────────────────────────
//
// Stream: amount=100_000, duration=1000 s → flow_rate = 100 stroops/s
// Cancel at t=350.
//   recipient_expected = 100 * 350 = 35_000
//   sender_expected    = 100_000 - 35_000 = 65_000
//
#[test]
fn lifecycle_cancel_mid_stream_recipient_gets_earned_sender_gets_refund() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 100_000;
    let duration: u64 = 1_000;
    let cancel_at: u64 = 350;
    let flow_rate: i128 = amount / duration as i128; // 100

    mint(&ctx, &ctx.sender, amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    ctx.env.ledger().set_timestamp(cancel_at);
    c.cancel_stream(&stream_id, &ctx.sender);

    let recipient_bal = balance(&ctx, &ctx.recipient);
    let sender_bal    = balance(&ctx, &ctx.sender);

    let expected_recipient = flow_rate * cancel_at as i128;
    let expected_sender    = amount - expected_recipient;

    assert_eq!(
        recipient_bal, expected_recipient,
        "recipient should receive earned portion at cancellation"
    );
    assert_eq!(
        sender_bal, expected_sender,
        "sender should receive refund of unstreamed remainder"
    );
    // Conservation: every stroop must be accounted for.
    assert_eq!(
        recipient_bal + sender_bal,
        amount,
        "total balance must equal original deposit (conservation)"
    );
    // Stream storage must be cleaned up after cancellation.
    assert!(c.try_get_stream(&stream_id).is_err(), "stream must be removed after cancel");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #3 — top_up mid-stream: new funds extend the claimable window,
//            and the recipient can claim the full topped-up amount.
// ─────────────────────────────────────────────────────────────────────────────
//
// Stream: amount=1_000_000, duration=1000 s → flow_rate=1000 stroops/s
//   end_time_initial = 1000
// Top-up at t=200 with 500_000 additional tokens:
//   additional_seconds = 500_000 / 1000 = 500
//   end_time_after_topup = 1000 + 500 = 1500
// Withdraw at t=600 → earned 600 s * 1000 = 600_000
// Withdraw at t=1500 (new end) → full deposit 1_500_000 received.
//
#[test]
fn lifecycle_top_up_mid_stream_extends_claimable_window() {
    let ctx = setup();
    let c = client(&ctx);

    let initial_amount: i128 = 1_000_000;
    let duration: u64 = 1_000;
    let top_up_amount: i128 = 500_000;
    let flow_rate: i128 = initial_amount / duration as i128; // 1000

    mint(&ctx, &ctx.sender, initial_amount + top_up_amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &initial_amount,
        &duration,
        &false,
        &bare_params(0),
    );

    // ── Top-up at t=200 ──────────────────────────────────────────────────────
    ctx.env.ledger().set_timestamp(200);
    c.top_up(&stream_id, &ctx.sender, &ctx.token, &top_up_amount);

    let stream = c.get_stream(&stream_id);
    let expected_end = 1_500u64; // 1000 + 500_000/1000
    assert_eq!(
        stream.end_time, expected_end,
        "end_time must increase by top_up / flow_rate seconds"
    );
    assert_eq!(
        stream.deposit,
        initial_amount + top_up_amount,
        "deposit must reflect top-up"
    );

    // ── Withdraw at t=600 — stream still active ───────────────────────────────
    ctx.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &ctx.recipient);

    let earned_600 = flow_rate * 600;
    assert_eq!(
        balance(&ctx, &ctx.recipient),
        earned_600,
        "recipient balance at t=600 should equal flow_rate * elapsed"
    );

    // Stream must still be active — new end_time is 1500.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);

    // ── Withdraw at t=1500 — stream completes ────────────────────────────────
    ctx.env.ledger().set_timestamp(1_500);
    c.withdraw(&stream_id, &ctx.recipient);

    let total = initial_amount + top_up_amount;
    assert_eq!(
        balance(&ctx, &ctx.recipient),
        total,
        "recipient must receive the full topped-up deposit by the new end_time"
    );

    // Stream should be removed after full drain.
    assert!(c.try_get_stream(&stream_id).is_err(), "stream must be removed after completion");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #4 — multi-step advance with intermediate withdrawals: verifies that
//            partial withdrawals properly update last_withdraw_time so that
//            subsequent claimable calculations start from the right base.
// ─────────────────────────────────────────────────────────────────────────────
//
// Stream: amount=3_000, duration=300 s → flow_rate=10 stroops/s
// Withdraw at t=100 (expect 1_000), t=200 (expect another 1_000), t=300 (expect final 1_000).
//
#[test]
fn lifecycle_incremental_withdrawals_track_last_withdraw_time() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 3_000;
    let duration: u64 = 300;
    let flow_rate: i128 = amount / duration as i128; // 10

    mint(&ctx, &ctx.sender, amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    // Step 1: t=100
    ctx.env.ledger().set_timestamp(100);
    c.withdraw(&stream_id, &ctx.recipient);
    assert_eq!(balance(&ctx, &ctx.recipient), flow_rate * 100);

    // Step 2: t=200 — only the incremental 100s should be claimable.
    ctx.env.ledger().set_timestamp(200);
    let claimable_before_withdraw = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_before_withdraw,
        flow_rate * 100,
        "claimable at t=200 should reflect only the 100s since last withdrawal"
    );
    c.withdraw(&stream_id, &ctx.recipient);
    assert_eq!(balance(&ctx, &ctx.recipient), flow_rate * 200);

    // Step 3: t=300 — final withdrawal drains the stream.
    ctx.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &ctx.recipient);
    assert_eq!(
        balance(&ctx, &ctx.recipient),
        amount,
        "final cumulative balance must equal total deposit"
    );
    assert!(c.try_get_stream(&stream_id).is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #5 — claimable at each step matches flow_rate × elapsed exactly
//            (validates the get_claimable view function in concert with time).
// ─────────────────────────────────────────────────────────────────────────────
//
// This is a read-only companion to Test #1 that asserts the *query* path
// (not the withdraw path) returns the right values at each ledger advance.
//
#[test]
fn lifecycle_get_claimable_tracks_flow_rate_times_elapsed() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 5_000;
    let duration: u64 = 500;
    let flow_rate: i128 = amount / duration as i128; // 10

    mint(&ctx, &ctx.sender, amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    // Check claimable at 5 evenly-spaced points without withdrawing.
    for step in 1u64..=5 {
        let now = step * 100;
        ctx.env.ledger().set_timestamp(now);
        let claimable = c.get_claimable(&stream_id);
        let expected = flow_rate * now as i128;
        assert_eq!(
            claimable, expected,
            "get_claimable at t={} expected {} got {}",
            now, expected, claimable
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #6 — cancel at the very first ledger after creation (t=0→1):
//            recipient gets flow_rate * 1 stroop, sender gets the rest.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn lifecycle_cancel_at_first_ledger_gives_minimal_recipient_payout() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 10_000;
    let duration: u64 = 1_000;
    let flow_rate: i128 = amount / duration as i128; // 10

    mint(&ctx, &ctx.sender, amount);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    ctx.env.ledger().set_timestamp(1);
    c.cancel_stream(&stream_id, &ctx.sender);

    let recipient_bal = balance(&ctx, &ctx.recipient);
    let sender_bal    = balance(&ctx, &ctx.sender);

    // 1 second elapsed → recipient earns flow_rate * 1
    assert_eq!(recipient_bal, flow_rate * 1);
    assert_eq!(sender_bal, amount - flow_rate * 1);
    assert_eq!(recipient_bal + sender_bal, amount);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test #7 — top_up at the very last second before end_time:
//            ensures top_up extends end_time even when called right at the
//            original end boundary.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn lifecycle_top_up_at_last_second_extends_end_time() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 1_000;
    let duration: u64 = 100;
    let top_up: i128 = 1_000;
    let flow_rate: i128 = amount / duration as i128; // 10

    mint(&ctx, &ctx.sender, amount + top_up);

    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    // Advance to 1 second before end.
    ctx.env.ledger().set_timestamp(duration - 1);
    c.top_up(&stream_id, &ctx.sender, &ctx.token, &top_up);

    let stream = c.get_stream(&stream_id);
    // New end_time = original 100 + top_up/flow_rate = 100 + 100 = 200
    let additional = (top_up / flow_rate) as u64;
    assert_eq!(stream.end_time, duration + additional);
    assert_eq!(stream.deposit, amount + top_up);
    assert_eq!(stream.status, StreamStatus::Active);
}
