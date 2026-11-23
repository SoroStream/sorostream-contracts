//! Tests for step-vesting (tranche schedule) and oracle price-check integration.
//!
//! Required coverage:
//!   Step-vesting:
//!     - No tranches vested yet           → get_claimable returns 0, withdraw is a no-op
//!     - One tranche vested               → only that tranche's amount is claimable/paid
//!     - All tranches vested              → full deposit paid out, stream cleaned up
//!     - Cancel mid-schedule              → vested tranches go to recipient, rest refunded
//!   Oracle:
//!     - Oracle not set                   → behaves identically to today
//!     - Price within threshold           → withdraw succeeds, PriceCheckPassed emitted
//!     - Price above threshold            → withdraw reverts with InvalidSlippage

use super::*;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Mock oracle contract
// ---------------------------------------------------------------------------

/// Configurable price oracle for testing.
/// Stores a price in instance storage so tests can update it between calls.
#[contract]
pub struct MockOracle;

const MOCK_PRICE_KEY: &str = "price";

#[contractimpl]
impl MockOracle {
    /// Sets the price returned by `get_price`.
    pub fn set_price(env: Env, price: i128) {
        env.storage()
            .instance()
            .set(&soroban_sdk::Symbol::new(&env, MOCK_PRICE_KEY), &price);
    }

    /// Returns the configured price (IPriceOracle interface).
    pub fn get_price(env: Env, _token: Address) -> i128 {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, MOCK_PRICE_KEY))
            .unwrap_or(1_000_000i128)
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct TrancheTestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup_tranche() -> TrancheTestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint a generous supply to the sender.
    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    // Disable minimum duration so short test windows work.
    SoroStreamContractClient::new(&env, &contract_id)
        .set_min_duration(&sender, &0u64);

    TrancheTestEnv { env, contract_id, token_id, sender, recipient }
}

fn client(t: &TrancheTestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

/// Build a `Vec<VestingTranche>` from (unlock_offset_seconds, amount) pairs.
/// `unlock_offset` is added to `env.ledger().timestamp()` at call time.
fn make_tranches(env: &Env, now: u64, pairs: &[(u64, i128)]) -> Vec<VestingTranche> {
    let mut v = Vec::new(env);
    for (offset, amount) in pairs {
        v.push_back(VestingTranche {
            unlock_time: now + offset,
            amount: *amount,
        });
    }
    v
}

// ---------------------------------------------------------------------------
// Step-vesting tests
// ---------------------------------------------------------------------------

#[test]
fn test_tranche_no_tranches_vested_get_claimable_is_zero() {
    let t = setup_tranche();
    let c = client(&t);

    // Start time = 1000; tranches unlock at 1500 and 2000 — both in the future.
    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 200_000i128;
    let tranches = make_tranches(&t.env, now, &[(500, 100_000), (1000, 100_000)]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64,   // nonce
        &0u64,   // lock_until
        &false,  // allow_recipient_termination
        &None,   // oracle
        &0u32,   // max_price_deviation_bps
    );

    // No time has passed — nothing is claimable.
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 0, "nothing should be claimable before first unlock");

    // Stream is still active and untouched.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.options.tranches_claimed, 0);
    assert_eq!(stream.options.total_withdrawn, 0);
}

#[test]
fn test_tranche_no_tranches_vested_withdraw_transfers_nothing() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 200_000i128;
    let tranches = make_tranches(&t.env, now, &[(500, 100_000), (1000, 100_000)]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Withdraw before any tranche unlocks — recipient balance must not change.
    c.withdraw(&stream_id, &t.recipient);

    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance_after, balance_before, "recipient should receive nothing before unlock");

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.tranches_claimed, 0);
}

#[test]
fn test_tranche_one_tranche_vested() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    // 4 equal tranches of 25_000 each, every 1000 seconds.
    let tranche_amount = 25_000i128;
    let deposit = tranche_amount * 4;
    let tranches = make_tranches(&t.env, now, &[
        (1000, tranche_amount),
        (2000, tranche_amount),
        (3000, tranche_amount),
        (4000, tranche_amount),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    // Advance past first unlock only.
    t.env.ledger().set_timestamp(2001); // t=2001: tranche 0 unlocked at t=2000, tranche 1 not yet.

    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, tranche_amount, "exactly one tranche should be claimable");

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(balance_after - balance_before, tranche_amount, "recipient should receive first tranche");

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.tranches_claimed, 1, "cursor should have advanced by 1");
    assert_eq!(stream.options.total_withdrawn, tranche_amount);
    assert_eq!(stream.status, StreamStatus::Active, "stream still has 3 remaining tranches");
}

#[test]
fn test_tranche_all_tranches_vested_stream_completed() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 300_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (100, 100_000),
        (200, 100_000),
        (300, 100_000),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    // Advance past all unlocks.
    t.env.ledger().set_timestamp(1400); // all three tranches are past their unlock_time

    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, deposit, "entire deposit should be claimable");

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(balance_after - balance_before, deposit, "recipient should receive full deposit");

    // Stream should be gone from storage after all tranches are claimed.
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "stream should have been removed after all tranches claimed");
}

#[test]
fn test_tranche_partial_withdraw_then_remaining() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 400_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (100,  100_000),
        (200,  100_000),
        (300,  100_000),
        (400,  100_000),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    // Claim first two tranches.
    t.env.ledger().set_timestamp(1250); // tranches at offsets 100 and 200 have unlocked
    c.withdraw(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.tranches_claimed, 2);
    assert_eq!(stream.options.total_withdrawn, 200_000);

    // Claim remaining two tranches.
    t.env.ledger().set_timestamp(1500); // tranches at offsets 300 and 400 have unlocked
    c.withdraw(&stream_id, &t.recipient);

    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "stream should be gone after all tranches");
}

#[test]
fn test_tranche_cancel_mid_schedule_refunds_unvested() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    // 4 tranches × 25_000 = 100_000 total.
    let tranche_amount = 25_000i128;
    let deposit = tranche_amount * 4;
    let tranches = make_tranches(&t.env, now, &[
        (500,  tranche_amount),
        (1000, tranche_amount),
        (1500, tranche_amount),
        (2000, tranche_amount),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    // Advance past the first two unlock times only.
    t.env.ledger().set_timestamp(2100); // t=2100: unlocks at 1500 (=1000+500) and 2000 have passed

    let sender_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    c.cancel_stream(&stream_id, &t.sender);

    let sender_balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Two tranches vested → recipient gets 2 × tranche_amount.
    // Two tranches unvested → sender refunded 2 × tranche_amount.
    assert_eq!(
        recipient_balance_after - recipient_balance_before,
        tranche_amount * 2,
        "recipient should get the two vested tranches"
    );
    assert_eq!(
        sender_balance_after - sender_balance_before,
        tranche_amount * 2,
        "sender should be refunded the two unvested tranches"
    );

    // Stream must be removed.
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "cancelled stream should be removed");
}

#[test]
fn test_tranche_cancel_before_any_vesting_full_refund() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 120_000i128;
    // All unlock times are far in the future.
    let tranches = make_tranches(&t.env, now, &[
        (10_000, 40_000),
        (20_000, 40_000),
        (30_000, 40_000),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );

    // Cancel immediately — no time has passed since creation.
    let sender_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    c.cancel_stream(&stream_id, &t.sender);
    let sender_balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    assert_eq!(
        sender_balance_after - sender_balance_before,
        deposit,
        "sender should get full deposit back when no tranches have vested"
    );
}

#[test]
fn test_tranche_invalid_empty() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let empty: Vec<VestingTranche> = Vec::new(&t.env);

    let result = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &empty,
        &0u64, &0u64, &false, &None, &0u32,
    );
    assert!(result.is_err(), "empty tranche list should be rejected");
}

#[test]
fn test_tranche_invalid_sum_mismatch() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    // Tranche amounts sum to 90_000 but deposit is 100_000.
    let tranches = make_tranches(&t.env, now, &[(1000, 45_000), (2000, 45_000)]);

    let result = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );
    assert!(result.is_err(), "mismatched tranche sum should be rejected");
}

#[test]
fn test_tranche_invalid_unsorted() {
    let t = setup_tranche();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    // Second tranche has an earlier unlock time than the first.
    let mut tranches = Vec::new(&t.env);
    tranches.push_back(VestingTranche { unlock_time: now + 2000, amount: 50_000 });
    tranches.push_back(VestingTranche { unlock_time: now + 1000, amount: 50_000 }); // out of order

    let result = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches,
        &0u64, &0u64, &false, &None, &0u32,
    );
    assert!(result.is_err(), "unsorted tranches should be rejected");
}

// ---------------------------------------------------------------------------
// Oracle tests
// ---------------------------------------------------------------------------

/// Sets up a test environment with a deployed MockOracle.
struct OracleTestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    oracle_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup_oracle() -> OracleTestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let oracle_id = env.register(MockOracle, ());

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    // Set initial oracle price = 1_000_000.
    MockOracleClient::new(&env, &oracle_id).set_price(&1_000_000i128);

    SoroStreamContractClient::new(&env, &contract_id)
        .set_min_duration(&sender, &0u64);

    OracleTestEnv { env, contract_id, token_id, oracle_id, sender, recipient }
}

fn oracle_client(t: &OracleTestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

#[test]
fn test_oracle_not_set_stream_behaves_normally() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 300_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (100, 100_000),
        (200, 100_000),
        (300, 100_000),
    ]);

    // No oracle attached.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false,
        &None,   // no oracle
        &0u32,
    );

    t.env.ledger().set_timestamp(1400);

    // Withdraw should succeed without any price check.
    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(balance_after - balance_before, deposit,
        "withdrawal without oracle should work normally");
}

#[test]
fn test_oracle_price_within_threshold_succeeds() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 200_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (500,  100_000),
        (1000, 100_000),
    ]);

    // Creation price will be 1_000_000 (set in setup_oracle).
    // Allow 10 % deviation = 1000 bps.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false,
        &Some(t.oracle_id.clone()),
        &1000u32,  // 10 % max deviation
    );

    // Verify creation price was recorded.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.creation_price, 1_000_000);

    // At withdrawal time: price moves by 5 % (within threshold).
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&1_050_000i128); // +5 %

    t.env.ledger().set_timestamp(1600); // both tranches unlocked

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);  // must NOT revert
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(balance_after - balance_before, deposit,
        "withdrawal within oracle threshold should succeed");
}

#[test]
fn test_oracle_price_above_threshold_reverts() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 200_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (500,  100_000),
        (1000, 100_000),
    ]);

    // Creation price = 1_000_000; allow 10 % max deviation.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false,
        &Some(t.oracle_id.clone()),
        &1000u32,  // 10 % = 1000 bps
    );

    // Price crashes 50 % — way beyond the 10 % threshold.
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&500_000i128); // -50 %

    t.env.ledger().set_timestamp(1600); // tranches are past unlock

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_err(), "withdrawal should revert when price deviation exceeds threshold");

    // Verify the stream is still intact (nothing should have changed).
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.total_withdrawn, 0, "no tokens should have moved");
    assert_eq!(stream.options.tranches_claimed, 0, "cursor must be unchanged after failed withdraw");
}

#[test]
fn test_oracle_price_exactly_at_threshold_succeeds() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 100_000i128;
    let tranches = make_tranches(&t.env, now, &[(500, 100_000)]);

    // Allow 5 % (500 bps) deviation; creation price = 1_000_000.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false,
        &Some(t.oracle_id.clone()),
        &500u32, // exactly 5 %
    );

    // Move price up by exactly 5 %.
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&1_050_000i128); // +5 % = 500 bps

    t.env.ledger().set_timestamp(1600);

    // Should succeed (deviation == threshold, not strictly greater).
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok(), "price exactly at threshold should be accepted");
}

#[test]
fn test_oracle_linear_stream_price_check_on_withdraw() {
    // Verify oracle check also works for regular linear (non-step) streams.
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);

    // Linear stream with oracle — use create_stream (original entrypoint) which does NOT
    // accept oracle params, so we create it and then verify the oracle=None path is clean.
    // The oracle check for linear streams is triggered from withdraw when stream.options.oracle is Some.
    // Since create_stream doesn't accept oracle params, this test focuses on confirming the
    // no-oracle linear case remains unaffected.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128,
        &1000u64,  // duration
        &0u64,     // cliff
        &0u64,     // nonce
        &false,    // auto_renew
        &0u64,     // lock_until
        &false,    // allow_recipient_termination
    );

    t.env.ledger().set_timestamp(1500);

    // Should succeed with no oracle attached (oracle field = None on Stream).
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok(), "linear stream without oracle should always withdraw successfully");
}

#[test]
fn test_oracle_cancel_with_price_deviation_still_succeeds() {
    // cancel_stream does NOT check the oracle — only withdraw does.
    // A sender should always be able to cancel regardless of price.
    let t = setup_oracle();
    let c = oracle_client(&t);

    t.env.ledger().set_timestamp(1000);
    let now = t.env.ledger().timestamp();
    let deposit = 200_000i128;
    let tranches = make_tranches(&t.env, now, &[
        (500,  100_000),
        (1000, 100_000),
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64, &0u64, &false,
        &Some(t.oracle_id.clone()),
        &500u32,  // 5 % threshold
    );

    // Price crashes 90 % — withdrawal would fail.
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&100_000i128);

    // Advance past first tranche.
    t.env.ledger().set_timestamp(1600);

    // Cancel should still work — oracle is not checked on cancel.
    let result = c.try_cancel_stream(&stream_id, &t.sender);
    assert!(result.is_ok(), "cancel_stream should succeed regardless of oracle price");
}

// ─── Issue #324: dynamic oracle price updates during a stream ────────────────

/// Integration test: oracle price updates three times (up, down, back to original)
/// during a four-tranche stream's lifetime. A partial withdraw is performed at
/// each price interval boundary. The test verifies:
///
/// 1. Each individual withdraw succeeds while the price is within the 20 % threshold.
/// 2. `get_claimable` returns the correct per-period amount at each boundary.
/// 3. The running `total_withdrawn` after every period equals the sum of tranche
///    amounts for all tranches vested so far — i.e. the accumulator is never
///    reset by a price change.
/// 4. The final `total_withdrawn` equals the full deposit (all four tranches).
///
/// Price schedule (creation price = 1_000_000, threshold = 2000 bps / 20 %):
///   Period 0 → 1: price stays at 1_000_000 (0 % deviation)
///   Period 1 → 2: price rises to 1_150_000 (+15 %, within threshold)
///   Period 2 → 3: price falls to  870_000 (-13 %, within threshold)
///   Period 3 → 4: price returns to 1_000_000 (0 % deviation)
#[test]
fn test_oracle_three_price_updates_accumulator_correct() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    // ── Stream setup ────────────────────────────────────────────────────────
    // Four equal tranches of 50_000 each, unlocking every 1000 seconds.
    // Total deposit = 200_000.
    let tranche_amount = 50_000i128;
    let deposit        = tranche_amount * 4;
    let t0: u64        = 1_000; // creation timestamp

    t.env.ledger().set_timestamp(t0);

    let tranches = {
        let mut v = soroban_sdk::Vec::new(&t.env);
        v.push_back(VestingTranche { unlock_time: t0 + 1_000, amount: tranche_amount });
        v.push_back(VestingTranche { unlock_time: t0 + 2_000, amount: tranche_amount });
        v.push_back(VestingTranche { unlock_time: t0 + 3_000, amount: tranche_amount });
        v.push_back(VestingTranche { unlock_time: t0 + 4_000, amount: tranche_amount });
        v
    };

    // creation_price = 1_000_000 (set in setup_oracle).
    // Allow 20 % (2000 bps) deviation so all three price moves stay inside the band.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &0u64,                         // nonce
        &0u64,                         // lock_until
        &false,                        // allow_recipient_termination
        &Some(t.oracle_id.clone()),    // oracle
        &2000u32,                      // max_price_deviation_bps = 20 %
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.creation_price, 1_000_000, "creation price must be stored");
    assert_eq!(stream.deposit, deposit);

    let oracle = MockOracleClient::new(&t.env, &t.oracle_id);
    let token  = soroban_sdk::token::Client::new(&t.env, &t.token_id);

    // ── Period 1: price unchanged (0 % deviation) — tranche 0 unlocks ──────
    // Price: 1_000_000 (same as creation)
    oracle.set_price(&1_000_000i128);

    t.env.ledger().set_timestamp(t0 + 1_001); // just past first unlock

    let claimable_p1 = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_p1, tranche_amount,
        "period 1: exactly one tranche must be claimable"
    );

    let bal_before_p1 = token.balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient); // must NOT revert
    let bal_after_p1 = token.balance(&t.recipient);
    assert_eq!(
        bal_after_p1 - bal_before_p1, tranche_amount,
        "period 1: recipient receives first tranche"
    );

    let s = c.get_stream(&stream_id);
    assert_eq!(s.options.total_withdrawn, tranche_amount, "accumulator after period 1");
    assert_eq!(s.options.tranches_claimed, 1, "cursor after period 1");

    // ── Period 2: price UP to 1_150_000 (+15 %, within 20 % band) ──────────
    // Price: +15 % from creation (deviation = 1500 bps ≤ 2000 bps threshold)
    oracle.set_price(&1_150_000i128);

    t.env.ledger().set_timestamp(t0 + 2_001); // just past second unlock

    let claimable_p2 = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_p2, tranche_amount,
        "period 2: only the newly unlocked tranche must be claimable"
    );

    let bal_before_p2 = token.balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient); // price within threshold — must succeed
    let bal_after_p2 = token.balance(&t.recipient);
    assert_eq!(
        bal_after_p2 - bal_before_p2, tranche_amount,
        "period 2: recipient receives second tranche despite price move up"
    );

    let s = c.get_stream(&stream_id);
    assert_eq!(s.options.total_withdrawn, tranche_amount * 2, "accumulator after period 2");
    assert_eq!(s.options.tranches_claimed, 2, "cursor after period 2");

    // ── Period 3: price DOWN to 870_000 (−13 %, within 20 % band) ──────────
    // Price: -13 % from creation (deviation = 1300 bps ≤ 2000 bps threshold)
    oracle.set_price(&870_000i128);

    t.env.ledger().set_timestamp(t0 + 3_001); // just past third unlock

    let claimable_p3 = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_p3, tranche_amount,
        "period 3: only the newly unlocked tranche must be claimable"
    );

    let bal_before_p3 = token.balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient); // price within threshold — must succeed
    let bal_after_p3 = token.balance(&t.recipient);
    assert_eq!(
        bal_after_p3 - bal_before_p3, tranche_amount,
        "period 3: recipient receives third tranche despite price drop"
    );

    let s = c.get_stream(&stream_id);
    assert_eq!(s.options.total_withdrawn, tranche_amount * 3, "accumulator after period 3");
    assert_eq!(s.options.tranches_claimed, 3, "cursor after period 3");

    // ── Period 4: price back to 1_000_000 (0 % deviation) — final tranche ──
    oracle.set_price(&1_000_000i128);

    t.env.ledger().set_timestamp(t0 + 4_001); // past final unlock

    let claimable_p4 = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_p4, tranche_amount,
        "period 4: final tranche must be claimable"
    );

    let bal_before_p4 = token.balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let bal_after_p4 = token.balance(&t.recipient);
    assert_eq!(
        bal_after_p4 - bal_before_p4, tranche_amount,
        "period 4: recipient receives final tranche"
    );

    // ── Final invariant: total payout == deposit ────────────────────────────
    // After all four tranches the stream should have been removed from storage.
    let total_received =
        (bal_after_p1 - bal_before_p1) +
        (bal_after_p2 - bal_before_p2) +
        (bal_after_p3 - bal_before_p3) +
        (bal_after_p4 - bal_before_p4);

    assert_eq!(
        total_received, deposit,
        "sum of all period payouts must equal the original deposit"
    );

    // Stream must have been cleaned up after the last tranche.
    assert!(
        c.try_get_stream(&stream_id).is_err(),
        "stream must be removed after all tranches are claimed"
    );
}

/// Complementary test: a withdrawal is blocked when the oracle price moves OUTSIDE
/// the threshold between two tranche unlock events. The accumulator must not advance.
#[test]
fn test_oracle_price_out_of_band_blocks_mid_stream_withdraw() {
    let t = setup_oracle();
    let c = oracle_client(&t);

    let t0: u64 = 1_000;
    t.env.ledger().set_timestamp(t0);

    // Two tranches of 100_000 each.
    let tranche_amount = 100_000i128;
    let deposit        = tranche_amount * 2;

    let tranches = {
        let mut v = soroban_sdk::Vec::new(&t.env);
        v.push_back(VestingTranche { unlock_time: t0 + 1_000, amount: tranche_amount });
        v.push_back(VestingTranche { unlock_time: t0 + 2_000, amount: tranche_amount });
        v
    };

    // 10 % threshold (1000 bps).
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &tranches,
        &1u64, &0u64, &false,
        &Some(t.oracle_id.clone()),
        &1000u32,
    );

    // Price crashes 50 % before the first withdrawal — exceeds 10 % threshold.
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&500_000i128);

    t.env.ledger().set_timestamp(t0 + 1_001);

    // Withdraw must be blocked.
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(
        result.is_err(),
        "withdraw must fail when price deviation exceeds threshold"
    );

    // Accumulator and cursor must be unchanged.
    let s = c.get_stream(&stream_id);
    assert_eq!(s.options.total_withdrawn, 0, "total_withdrawn must not advance on failed withdraw");
    assert_eq!(s.options.tranches_claimed, 0, "tranche cursor must not advance on failed withdraw");

    // Oracle recovers; withdrawal now succeeds.
    MockOracleClient::new(&t.env, &t.oracle_id).set_price(&1_050_000i128); // +5 %, within 10 %
    c.withdraw(&stream_id, &t.recipient);

    let s = c.get_stream(&stream_id);
    assert_eq!(s.options.total_withdrawn, tranche_amount, "accumulator must advance after recovery");
    assert_eq!(s.options.tranches_claimed, 1);
}
