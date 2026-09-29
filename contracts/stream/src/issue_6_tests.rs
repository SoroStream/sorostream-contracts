/// Issue #6 — resume_stream must clear `paused_at` after adjusting `claimed_at`
///
/// Root cause: after a resume, `last_pause_time` still held the ledger of the
/// *first* pause. When the stream was paused a second time the claimable
/// calculation used `last_pause_time` as its "current time" and subtracted
/// `last_withdraw_time`, which still pointed into the first pause window,
/// causing ledgers that were genuinely paused to be counted as claimable.
///
/// Fix: `resume_stream` sets `last_pause_time = 0` after it has shifted all
/// time fields forward by the paused duration.
///
/// Tests
/// ─────
/// 1. `test_resume_clears_paused_at`          — `last_pause_time` is 0 after resume.
/// 2. `test_pause_resume_pause_claimable`      — pause→resume→pause cycle yields
///                                               the correct claimable at each step.
/// 3. `test_claimable_zero_during_pause`       — advancing the ledger while a stream
///                                               is paused does not increase claimable.
/// 4. `test_second_pause_after_resume_correct` — a second pause after a resume does
///                                               not double-count the first pause gap.
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ── helpers ──────────────────────────────────────────────────────────────────

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint a generous supply so transfers never fail.
    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    // Disable the minimum-duration gate for unit tests.
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv { env, contract_id, token_id, sender, recipient }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

/// Minimal `CreateStreamParams` for these tests.
fn params_with_nonce(nonce: u64) -> crate::types::CreateStreamParams {
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

// ── Test 1 ────────────────────────────────────────────────────────────────────
/// After `resume_stream`, `last_pause_time` must be 0 (cleared).
#[test]
fn test_resume_clears_paused_at() {
    let t = setup();
    let c = client(&t);

    // Create stream: 1 token/s for 1000 s.
    t.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000, &1_000, &false,
        &params_with_nonce(0),
    );

    // Pause at t=100.
    t.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &t.sender);

    {
        let s = c.get_stream(&stream_id);
        assert_eq!(s.options.last_pause_time, 100, "last_pause_time must be set to pause ledger");
    }

    // Resume at t=200.
    t.env.ledger().set_timestamp(200);
    c.resume_stream(&stream_id, &t.sender);

    let s = c.get_stream(&stream_id);
    assert_eq!(
        s.options.last_pause_time, 0,
        "last_pause_time must be cleared (0) after resume"
    );
}

// ── Test 2 ────────────────────────────────────────────────────────────────────
/// pause → resume → pause cycle yields the correct claimable at each step.
///
/// Timeline (flow_rate = 1 token/s, deposit = 1000):
///   t=0    stream created
///   t=100  pause            → claimable = 100
///   t=200  resume           → stream shifted +100 s; end_time = 1100
///   t=300  claimable        → 100 s active since resume = 100  (not 200!)
///   t=300  pause again
///   t=400  claimable        → frozen at pause time 300 → 100 claimable
#[test]
fn test_pause_resume_pause_claimable() {
    let t = setup();
    let c = client(&t);

    // flow_rate = 1_000 / 1_000 = 1 token/s
    t.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000, &1_000, &false,
        &params_with_nonce(0),
    );

    // ── Pause at t=100 ──────────────────────────────────────────────────────
    t.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &t.sender);

    let claimable_at_pause = c.get_claimable(&stream_id);
    // Paused stream reports claimable frozen at pause time (100 - 0 = 100 tokens).
    assert_eq!(claimable_at_pause, 100, "claimable at first pause must be 100");

    // ── Resume at t=200 ──────────────────────────────────────────────────────
    // Paused for 100 s. All time fields shift +100.
    //   start_time       = 0 + 100 = 100  (not directly visible but implied)
    //   last_withdraw_time = 0 + 100 = 100
    //   end_time         = 1000 + 100 = 1100
    //   last_pause_time  = 0  (cleared!)
    t.env.ledger().set_timestamp(200);
    c.resume_stream(&stream_id, &t.sender);

    {
        let s = c.get_stream(&stream_id);
        assert_eq!(s.options.last_pause_time, 0, "last_pause_time cleared after resume");
        assert_eq!(s.end_time, 1100, "end_time extended by pause duration");
    }

    // ── Check claimable at t=300 (100 s of active flow since resume at t=200) ─
    t.env.ledger().set_timestamp(300);
    let claimable_mid = c.get_claimable(&stream_id);
    // last_withdraw_time = 100 (shifted). effective now = 300. elapsed = 200 - 100 = wait...
    // After resume: last_withdraw_time was 0, shifted by 100 → 100.
    // At t=300: elapsed since last_withdraw = 300 - 100 = 200 → claimable = 200.
    // Note: the 100 s between t=100..t=200 were paused, so only 100 s (t=200..t=300) are
    // genuinely new, but last_withdraw_time was also shifted, so: 300 - 100 = 200 is the
    // mathematical result because start was shifted to 100.
    // The key invariant is that the PAUSED window (t=100..t=200) is NOT double-counted.
    assert!(claimable_mid > 0, "claimable after resume must be positive");
    // Exactly: last_withdraw_time = 100, now = 300 → elapsed = 200 → claimable = 200.
    assert_eq!(claimable_mid, 200, "claimable at t=300 must be 200 (100..300 elapsed since shifted LWT)");

    // ── Pause again at t=300 ─────────────────────────────────────────────────
    t.env.ledger().set_timestamp(300);
    c.pause_stream(&stream_id, &t.sender);

    {
        let s = c.get_stream(&stream_id);
        assert_eq!(s.options.last_pause_time, 300, "second pause records correct timestamp");
    }

    // Advance ledger to t=400 — claimable must remain frozen at the second-pause value.
    t.env.ledger().set_timestamp(400);
    let claimable_frozen = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_frozen, claimable_mid,
        "claimable must not increase while stream is paused (second pause)"
    );
}

// ── Test 3 ────────────────────────────────────────────────────────────────────
/// Advancing the ledger while a stream is paused must not increase claimable.
#[test]
fn test_claimable_zero_during_pause() {
    let t = setup();
    let c = client(&t);

    // flow_rate = 500 / 1000 = 0 (below 1 token/s); use larger amount.
    // Use 10_000 tokens / 1_000 s = 10 tokens/s.
    t.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params_with_nonce(0),
    );

    // Let 50 s pass, then pause.
    t.env.ledger().set_timestamp(50);
    c.pause_stream(&stream_id, &t.sender);

    let claimable_at_pause = c.get_claimable(&stream_id);

    // Advance the clock by 500 s while still paused.
    t.env.ledger().set_timestamp(550);
    let claimable_after_wait = c.get_claimable(&stream_id);

    assert_eq!(
        claimable_at_pause, claimable_after_wait,
        "claimable must not grow while stream is paused"
    );
    assert!(claimable_at_pause > 0, "sanity: some tokens were earned before pause");
}

// ── Test 4 ────────────────────────────────────────────────────────────────────
/// A second pause after a resume must not double-count the first pause gap.
///
/// Specifically: if we pause at t=100, resume at t=200, then immediately pause
/// again at t=200, the claimable must be 0 new tokens (none earned in the
/// zero-width window between resume and second pause), not 100 (the stale
/// first-pause window, which would happen with the old bug).
#[test]
fn test_second_pause_after_resume_no_stale_paused_at() {
    let t = setup();
    let c = client(&t);

    // 10 tokens/s
    t.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &1_000, &false,
        &params_with_nonce(0),
    );

    // Pause at t=100 (1000 tokens earned so far, frozen).
    t.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &t.sender);

    // Resume at t=300 (paused for 200 s).
    t.env.ledger().set_timestamp(300);
    c.resume_stream(&stream_id, &t.sender);

    // Verify paused_at is cleared.
    assert_eq!(c.get_stream(&stream_id).options.last_pause_time, 0);

    // Pause again immediately at t=300.
    t.env.ledger().set_timestamp(300);
    c.pause_stream(&stream_id, &t.sender);

    // Claimable at second pause: from the stream's perspective time has shifted.
    // last_withdraw_time was 0, shifted by 200 → 200.
    // last_pause_time = 300. elapsed = 300 - 200 = 100 → 1000 tokens claimable.
    // (These are the 100 s that were genuinely active from t=0..t=100 before the first pause.)
    let claimable = c.get_claimable(&stream_id);

    // Advance the clock a LOT while still paused — claimable must stay frozen.
    t.env.ledger().set_timestamp(10_000);
    let claimable_later = c.get_claimable(&stream_id);

    assert_eq!(
        claimable, claimable_later,
        "claimable must remain frozen during second pause (stale paused_at bug would grow it)"
    );

    // The stale-paused_at bug would have made claimable = 300 * 10 = 3000 at t=10_000
    // (because last_pause_time was still 100 and flow_rate * (100 - last_withdraw_time)
    // used the first pause window again). After the fix it is correctly frozen.
    // Double check: it equals the properly computed 1000 tokens (100 active seconds).
    assert_eq!(
        claimable, 1_000,
        "only 100 genuinely active seconds = 1000 tokens should be claimable"
    );
}
