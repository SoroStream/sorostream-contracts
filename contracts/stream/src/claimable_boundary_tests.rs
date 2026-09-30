/// Claimable boundary tests — issue #38
///
/// Covers the edge cases where bugs most commonly appear:
///   • ledger before start_time  → claimable == 0
///   • ledger exactly at start_time → claimable == 0
///   • ledger exactly at end_time → claimable == flow_rate × (end - start)
///   • ledger past end_time → capped at full stream balance (no over-pay)
///   • paused stream → claimable does not grow during the pause window
///   • flow_rate == 0 edge → contract rejects at creation (ZeroFlowRate)
///
/// Branch: feat/38-claimable-boundary-tests

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
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

    // Disable minimum-duration guard so tests can use any duration.
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
// Helper: create a scheduled stream whose start_time is in the future.
//
// `create_stream_scheduled` takes explicit start_time and cliff_seconds
// separate from CreateStreamParams, so we call it directly here.
// ─────────────────────────────────────────────────────────────────────────────
fn create_scheduled(
    ctx: &Ctx,
    start_time: u64,
    duration: u64,
    amount: i128,
    nonce: u64,
) -> u64 {
    let c = client(ctx);
    c.create_stream_scheduled(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &start_time,
        &0u64,   // cliff_seconds
        &nonce,
        &false,  // auto_renew
        &None::<u32>,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: ledger strictly before start_time → claimable == 0
//
// A scheduled stream has start_time=100.  At t=99, no tokens should be
// claimable because the stream has not started yet.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_ledger_before_start_time_claimable_is_zero() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 10_000;
    let start_time: u64 = 100;
    let duration: u64 = 1_000;

    mint(&ctx, &ctx.sender, amount);
    let stream_id = create_scheduled(&ctx, start_time, duration, amount, 0);

    // Query claimable just before the stream starts.
    ctx.env.ledger().set_timestamp(start_time - 1); // t=99
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(
        claimable, 0,
        "claimable must be 0 when ledger is before start_time"
    );

    // Attempting to withdraw should transfer nothing to the recipient.
    c.withdraw(&stream_id, &ctx.recipient);
    assert_eq!(
        balance(&ctx, &ctx.recipient),
        0,
        "recipient balance must stay 0 when withdrawing before start_time"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: ledger exactly at start_time → claimable == 0
//
// At the precise instant the stream starts, elapsed time is 0.
// flow_rate × 0 == 0, so nothing is claimable yet.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_ledger_exactly_at_start_time_claimable_is_zero() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 10_000;
    let start_time: u64 = 100;
    let duration: u64 = 1_000;

    mint(&ctx, &ctx.sender, amount);
    let stream_id = create_scheduled(&ctx, start_time, duration, amount, 0);

    ctx.env.ledger().set_timestamp(start_time); // exactly t=100
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(
        claimable, 0,
        "claimable must be 0 at the exact start_time (elapsed == 0)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: ledger exactly at end_time → claimable == flow_rate × (end − start)
//
// The full streaming period has elapsed, so the entire deposit (minus any
// rounding dust returned to sender) should be claimable.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_ledger_exactly_at_end_time_claimable_equals_full_deposit() {
    let ctx = setup();
    let c = client(&ctx);

    // Use amounts that divide evenly to avoid dust in this assertion.
    let amount: i128 = 10_000;
    let start_time: u64 = 0;
    let duration: u64 = 1_000;
    let flow_rate: i128 = amount / duration as i128; // 10

    mint(&ctx, &ctx.sender, amount);

    // Plain (non-scheduled) stream starts at t=0.
    let stream_id = c.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    ctx.env.ledger().set_timestamp(start_time + duration); // t=1000
    let claimable = c.get_claimable(&stream_id);

    let expected = flow_rate * duration as i128; // 10 * 1000 = 10_000
    assert_eq!(
        claimable, expected,
        "claimable at end_time must equal flow_rate × (end − start)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: ledger past end_time → capped at full stream balance (no over-pay)
//
// At t = end_time + 500 the elapsed time would naively be 1500 s, but the
// contract must cap elapsed at (end_time − last_withdraw_time).
// The recipient must never receive more than the original deposit.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_ledger_past_end_time_claimable_capped_at_deposit() {
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

    // Advance well past end_time.
    ctx.env.ledger().set_timestamp(duration + 500); // t=1500
    let claimable = c.get_claimable(&stream_id);

    // Claimable must be capped at the full deposit, not flow_rate * 1500.
    let expected_max = flow_rate * duration as i128; // 10_000
    assert_eq!(
        claimable, expected_max,
        "claimable must be capped at the deposit when ledger is past end_time"
    );

    // Withdraw and verify no tokens beyond the deposit are transferred.
    c.withdraw(&stream_id, &ctx.recipient);
    assert_eq!(
        balance(&ctx, &ctx.recipient),
        amount,
        "recipient must receive exactly the deposit, not more"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: paused stream → claimable does not grow during the pause window
//
// Stream: flow_rate=10, paused at t=200.
// At t=500 (during pause) claimable must still be flow_rate × 200 = 2_000
// (frozen at the pause point), not flow_rate × 500 = 5_000.
// After resume at t=600, claimable resumes from the frozen point.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_paused_stream_claimable_does_not_grow_during_pause() {
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

    // ── Pause at t=200 ───────────────────────────────────────────────────────
    let pause_at: u64 = 200;
    ctx.env.ledger().set_timestamp(pause_at);
    c.pause_stream(&stream_id, &ctx.sender);

    let claimable_at_pause = c.get_claimable(&stream_id);
    let expected_at_pause = flow_rate * pause_at as i128; // 2_000
    assert_eq!(
        claimable_at_pause, expected_at_pause,
        "claimable at pause time should equal flow_rate × elapsed_before_pause"
    );

    // ── During pause: claimable must stay frozen ─────────────────────────────
    for t in [300u64, 400, 500] {
        ctx.env.ledger().set_timestamp(t);
        let claimable = c.get_claimable(&stream_id);
        assert_eq!(
            claimable, expected_at_pause,
            "claimable must not grow while stream is paused (checked at t={})", t
        );
    }

    // Confirm status is Paused.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);

    // ── Resume at t=600 ──────────────────────────────────────────────────────
    // Paused duration = 600 - 200 = 400 s.  All stream timestamps shift +400.
    // Effective resume point is still "200 s of flow" earned.
    let resume_at: u64 = 600;
    ctx.env.ledger().set_timestamp(resume_at);
    c.resume_stream(&stream_id, &ctx.sender);

    let stream_after_resume = c.get_stream(&stream_id);
    assert_eq!(stream_after_resume.status, StreamStatus::Active);

    // Immediately after resume, claimable should still reflect only the
    // 200 s of flow earned before the pause (the stream clock was frozen).
    let claimable_at_resume = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_at_resume, expected_at_pause,
        "claimable immediately after resume must equal pre-pause earned amount"
    );

    // ── Post-resume: claimable grows again ───────────────────────────────────
    // Advance 100 s past the resume point (the stream's shifted clock).
    // The stream's start_time was shifted by 400 s, so 100 s of new flow
    // should accrue = 10 * 100 = 1_000 more.
    let post_resume_elapsed: u64 = 100;
    ctx.env.ledger().set_timestamp(resume_at + post_resume_elapsed);
    let claimable_post = c.get_claimable(&stream_id);
    let expected_post = expected_at_pause + flow_rate * post_resume_elapsed as i128;
    assert_eq!(
        claimable_post, expected_post,
        "claimable must resume growing after stream is unpaused"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: flow_rate == 0 → contract must reject with ZeroFlowRate at creation
//
// A zero flow_rate means (amount / duration_seconds) truncates to 0.
// The contract validates this at creation and must return ZeroFlowRate.
// This boundary exists because 1 stroop / 1_000_000 seconds rounds to 0.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_zero_flow_rate_rejected_at_creation() {
    let ctx = setup();
    let c = client(&ctx);

    // 1 stroop over 1,000,000 seconds: floor(1 / 1_000_000) == 0
    let amount: i128 = 1;
    let duration: u64 = 1_000_000;

    mint(&ctx, &ctx.sender, amount);

    let result = c.try_create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    assert_eq!(
        result,
        Err(Ok(StreamError::ZeroFlowRate)),
        "creating a stream with effective flow_rate==0 must fail with ZeroFlowRate"
    );

    // No tokens must have left the sender's account.
    assert_eq!(
        balance(&ctx, &ctx.sender),
        amount,
        "sender balance must be unchanged when stream creation is rejected"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: additional zero-flow_rate variant — large duration, small amount
//
// Confirms the check applies regardless of which parameter causes the
// truncation, not just the minimum possible case.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_zero_flow_rate_rejected_large_duration_small_amount() {
    let ctx = setup();
    let c = client(&ctx);

    // 999 stroops / 1_000 s = floor(0.999) = 0
    let amount: i128 = 999;
    let duration: u64 = 1_000;

    mint(&ctx, &ctx.sender, amount);

    let result = c.try_create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token,
        &amount,
        &duration,
        &false,
        &bare_params(0),
    );

    assert_eq!(
        result,
        Err(Ok(StreamError::ZeroFlowRate)),
        "flow_rate 0.999 truncates to 0 and must be rejected"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8: boundary — withdraw at exactly end_time yields full deposit.
//
// This is the withdrawal-side complement to boundary test 3 (which checks
// the query path).  The transfer must equal the full deposit.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_withdraw_at_exact_end_time_yields_full_deposit() {
    let ctx = setup();
    let c = client(&ctx);

    let amount: i128 = 5_000;
    let duration: u64 = 500;

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

    ctx.env.ledger().set_timestamp(duration); // exactly at end_time
    c.withdraw(&stream_id, &ctx.recipient);

    assert_eq!(
        balance(&ctx, &ctx.recipient),
        amount,
        "withdrawing at exact end_time must transfer the full deposit"
    );
    // Contract must clean up the stream after full settlement.
    assert!(c.try_get_stream(&stream_id).is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9: multiple pauses in a row — claimable must remain frozen for
//          the entire combined pause period.
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_repeated_pause_resume_claimable_stays_correct() {
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

    // Pause #1 at t=100 (earned 1_000), resume at t=200 (pause duration=100).
    ctx.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &ctx.sender);
    ctx.env.ledger().set_timestamp(200);
    c.resume_stream(&stream_id, &ctx.sender);

    // Pause #2 at t=300 (active for 100 s since resume → earned another 1_000),
    // resume at t=400.
    ctx.env.ledger().set_timestamp(300);
    c.pause_stream(&stream_id, &ctx.sender);
    ctx.env.ledger().set_timestamp(400);
    c.resume_stream(&stream_id, &ctx.sender);

    // After two pause/resume cycles, total active time = 200 s.
    // Expected claimable = flow_rate * 200 = 2_000.
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(
        claimable,
        flow_rate * 200,
        "after two pause cycles, claimable must equal flow_rate × total_active_seconds"
    );
}
