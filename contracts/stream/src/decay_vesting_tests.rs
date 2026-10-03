//! Tests for `VestingCurve::TimeDecay` and the `simulate_claimable` utility.
//!
//! Required coverage (per spec):
//!   1. TimeDecay claimable ≥ linear claimable at every elapsed time point.
//!   2. Total claimable at end_time equals the full deposit regardless of decay_factor.
//!   3. decay_factor = 0 produces exactly the same output as linear.
//!   4. Pre-computed expected values at multiple time points.
//!   5. simulate_claimable returns correct amounts at multiple time points.

use crate::vesting_math::{
    compute_claimable, compute_claimable_decay, compute_cumulative_decay, simulate_claimable,
    DECAY_WINDOW_SECS,
};

// Also test the contract-level entrypoints.
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ---------------------------------------------------------------------------
// Pure-math unit tests (no Soroban environment needed)
// ---------------------------------------------------------------------------

/// Tolerance for integer-arithmetic assertions: we allow a 1-stroop rounding
/// margin per window to account for integer division truncation.
fn within(a: i128, b: i128, tolerance: i128) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn test_decay_factor_zero_equals_linear() {
    // When decay_factor == 0, compute_cumulative_decay must return the same
    // result as the simple linear formula: deposit × elapsed / duration.

    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;

    for elapsed in [0, 1000, 2500, 5000, 7500, 9999, 10_000] {
        let query = start + elapsed;

        let decay_result =
            compute_cumulative_decay(deposit, start, end, query, 0).unwrap();

        // Linear formula: deposit * elapsed / duration  (clamped to deposit at end)
        let linear_result = if query >= end {
            deposit
        } else {
            deposit * (query - start) as i128 / (end - start) as i128
        };

        assert_eq!(
            decay_result, linear_result,
            "decay_factor=0 should equal linear at elapsed={elapsed}"
        );
    }
}

#[test]
fn test_decay_claimable_always_gte_linear() {
    // For any decay_factor > 0 the front-weighting means that by any given
    // moment the cumulative decay-vested amount is ≥ the linearly-vested amount.

    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let duration = (end - start) as i128;

    // Test several decay_factor values.
    for &df in &[50u32, 100, 200, 500, 1000] {
        for elapsed in (0..=10_000u64).step_by(500) {
            let query = start + elapsed;

            let decay_result =
                compute_cumulative_decay(deposit, start, end, query, df).unwrap();

            let linear_result = if query >= end {
                deposit
            } else {
                deposit * (query - start) as i128 / duration
            };

            assert!(
                decay_result >= linear_result,
                "decay({df}) at elapsed={elapsed}: decay={decay_result} < linear={linear_result}"
            );
        }
    }
}

#[test]
fn test_decay_converges_to_full_deposit_at_end_time() {
    let deposit = 2_000_000i128;
    let start = 0u64;
    let end = 20_000u64;

    for &df in &[0u32, 50, 100, 250, 500, 1000, 2000] {
        let result = compute_cumulative_decay(deposit, start, end, end, df).unwrap();
        assert_eq!(
            result, deposit,
            "decay_factor={df}: should equal deposit at end_time, got {result}"
        );

        // Also verify that querying beyond end_time also returns deposit.
        let result_beyond =
            compute_cumulative_decay(deposit, start, end, end + 5000, df).unwrap();
        assert_eq!(
            result_beyond, deposit,
            "decay_factor={df}: querying past end_time should still return deposit"
        );
    }
}

#[test]
fn test_decay_is_monotone_increasing() {
    // cumulative_decay(t2) >= cumulative_decay(t1) when t2 > t1.
    let deposit = 500_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let df = 200u32;

    let mut prev = 0i128;
    for t in (0..=10_000u64).step_by(DECAY_WINDOW_SECS as usize) {
        let cur = compute_cumulative_decay(deposit, start, end, t, df).unwrap();
        assert!(
            cur >= prev,
            "cumulative_decay not monotone: at t={t} got {cur}, previous={prev}"
        );
        prev = cur;
    }
}

#[test]
fn test_decay_starts_at_zero() {
    let deposit = 1_000_000i128;
    let start = 1_000u64;
    let end = 11_000u64;
    let df = 300u32;

    // At or before start_time, nothing is vested.
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, start, df).unwrap(),
        0,
        "nothing should be vested at start_time"
    );
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, start - 1, df).unwrap(),
        0,
        "nothing should be vested before start_time"
    );
}

#[test]
fn test_decay_precomputed_values() {
    // Manually verify the iterative fixed-point formula at known time points.
    //
    // Parameters:
    //   deposit = 1_000_000 stroops
    //   start   = 0, end = 10_000 s (10 windows of 1 000 s each)
    //   decay_factor = 1000 bps = 10 % per window
    //   keep_bps = 9_000
    //   SCALE = 1_000_000_000
    //
    // remaining_scaled after k windows = SCALE × (9000/10000)^k
    //   k=0: 1_000_000_000   → vested = 0
    //   k=1: 900_000_000     → vested = deposit × 100_000_000 / 1e9 = 100_000
    //   k=2: 810_000_000     → vested = deposit × 190_000_000 / 1e9 = 190_000
    //   k=5: 590_490_000     → vested = deposit × 409_510_000 / 1e9 ≈ 409_510
    //   k=10 (end_time):     → vested = deposit (convergence guarantee)

    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let df = 1000u32; // 10 % per 1 ks window

    // t = 0 (k=0)
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, 0, df).unwrap(),
        0
    );

    // t = 1000 (k=1): remaining_scaled = 900_000_000
    // vested = 1_000_000 × (1_000_000_000 − 900_000_000) / 1_000_000_000
    //        = 1_000_000 × 100_000_000 / 1_000_000_000 = 100_000
    let v1 = compute_cumulative_decay(deposit, start, end, 1000, df).unwrap();
    assert!(within(v1, 100_000, 1), "k=1 expected≈100_000 got {v1}");

    // t = 2000 (k=2): remaining_scaled = 810_000_000
    // vested = 1_000_000 × 190_000_000 / 1_000_000_000 = 190_000
    let v2 = compute_cumulative_decay(deposit, start, end, 2000, df).unwrap();
    assert!(within(v2, 190_000, 1), "k=2 expected≈190_000 got {v2}");

    // t = 5000 (k=5): remaining_scaled = 9000^5/10000^5 × SCALE
    //   = 59_049_000_000_000 / 100_000_000_000_000 × 1e9  ≈ 590_490_000
    // vested = 1_000_000 × (1e9 - 590_490_000) / 1e9 ≈ 409_510
    let v5 = compute_cumulative_decay(deposit, start, end, 5000, df).unwrap();
    assert!(within(v5, 409_510, 2), "k=5 expected≈409_510 got {v5}");

    // t = end (convergence)
    let v_end = compute_cumulative_decay(deposit, start, end, end, df).unwrap();
    assert_eq!(v_end, deposit, "at end_time vested must equal deposit");
}

#[test]
fn test_compute_claimable_decay_incremental() {
    // Verify that two successive calls to compute_claimable_decay correctly
    // return the incremental (not cumulative) amount.

    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let cliff = 0u64;
    let df = 500u32; // 5 % per window

    // First withdrawal at t=3000
    let first_claim =
        compute_claimable_decay(deposit, start, end, 3000, cliff, start, df).unwrap();
    // Should equal cumulative(3000).
    let expected_first = compute_cumulative_decay(deposit, start, end, 3000, df).unwrap();
    assert_eq!(first_claim, expected_first);

    // Second withdrawal at t=6000, last_withdraw_time=3000.
    let second_claim =
        compute_claimable_decay(deposit, start, end, 6000, cliff, 3000, df).unwrap();
    let expected_second = compute_cumulative_decay(deposit, start, end, 6000, df).unwrap()
        - compute_cumulative_decay(deposit, start, end, 3000, df).unwrap();
    assert_eq!(second_claim, expected_second);

    // Sum of all incremental claims equals cumulative at final point.
    let total = first_claim + second_claim;
    let expected_total = compute_cumulative_decay(deposit, start, end, 6000, df).unwrap();
    assert_eq!(total, expected_total);
}

#[test]
fn test_compute_claimable_decay_cliff_enforced() {
    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let cliff = 2000u64; // 2 ks cliff
    let df = 200u32;

    // Before cliff: nothing claimable.
    let before = compute_claimable_decay(deposit, start, end, 1999, cliff, start, df).unwrap();
    assert_eq!(before, 0, "nothing claimable before cliff");

    // At cliff: should match cumulative from start.
    let at_cliff = compute_claimable_decay(deposit, start, end, 2000, cliff, start, df).unwrap();
    let expected = compute_cumulative_decay(deposit, start, end, 2000, df).unwrap();
    assert_eq!(at_cliff, expected);
}

#[test]
fn test_simulate_claimable_sequence() {
    // simulate_claimable returns the cumulative vested amount from stream start,
    // not the incremental. Verify a sequence of time points.

    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;
    let cliff = 1000u64;
    let df = 300u32;

    let times = [0u64, 500, 1000, 2000, 5000, 8000, 10_000, 12_000];

    let results: Vec<i128> = times
        .iter()
        .map(|&t| simulate_claimable(deposit, start, end, t, cliff, df).unwrap())
        .collect();

    // Before cliff: 0
    assert_eq!(results[0], 0, "t=0 should be 0 (before cliff)");
    assert_eq!(results[1], 0, "t=500 should be 0 (before cliff)");

    // At cliff: equals cumulative_decay at cliff time
    let expected_at_cliff = compute_cumulative_decay(deposit, start, end, 1000, df).unwrap();
    assert_eq!(results[2], expected_at_cliff, "t=cliff");

    // Strictly increasing after cliff.
    for i in 2..results.len() - 1 {
        assert!(
            results[i + 1] >= results[i],
            "simulate_claimable not monotone at index {i}: {} then {}",
            results[i],
            results[i + 1]
        );
    }

    // At and beyond end_time: full deposit.
    assert_eq!(results[6], deposit, "t=end_time should be full deposit");
    assert_eq!(results[7], deposit, "t>end_time should still be full deposit");
}

#[test]
fn test_decay_higher_factor_means_more_early_vesting() {
    // A higher decay_factor should always produce ≥ vested amount at any early
    // time compared to a lower decay_factor (more front-loading).
    let deposit = 1_000_000i128;
    let start = 0u64;
    let end = 10_000u64;

    // At t = 3000 (30 % through), compare df=100 vs df=500 vs df=1000.
    let v100 = compute_cumulative_decay(deposit, start, end, 3000, 100).unwrap();
    let v500 = compute_cumulative_decay(deposit, start, end, 3000, 500).unwrap();
    let v1000 = compute_cumulative_decay(deposit, start, end, 3000, 1000).unwrap();

    assert!(v500 >= v100, "df=500 should vest >= df=100 at t=3000");
    assert!(v1000 >= v500, "df=1000 should vest >= df=500 at t=3000");

    // All must still converge to the same deposit at end_time.
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, end, 100).unwrap(),
        deposit
    );
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, end, 500).unwrap(),
        deposit
    );
    assert_eq!(
        compute_cumulative_decay(deposit, start, end, end, 1000).unwrap(),
        deposit
    );
}

// ---------------------------------------------------------------------------
// Contract-level integration tests
// ---------------------------------------------------------------------------

struct DecayTestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup_decay() -> DecayTestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    // Zero minimum duration so short test windows work.
    SoroStreamContractClient::new(&env, &contract_id)
        .set_min_duration(&sender, &0u64);

    DecayTestEnv { env, contract_id, token_id, sender, recipient }
}

fn decay_client(t: &DecayTestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

#[test]
fn test_contract_linear_curve_stored_and_claimable() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(1000);
    let deposit = 100_000i128;
    let duration = 10_000u64;

    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
        &VestingCurve::Linear,
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.curve, VestingCurve::Linear);

    // Advance to 50 % of duration.
    t.env.ledger().set_timestamp(1000 + duration / 2);
    let claimable = c.get_claimable(&stream_id);
    // Linear: flow_rate × elapsed = (100_000/10_000) × 5_000 = 50_000
    assert_eq!(claimable, 50_000, "linear curve: half the deposit after half duration");
}

#[test]
fn test_contract_decay_curve_stored_and_claimable_gte_linear() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(0);
    let deposit = 1_000_000i128;
    let duration = 10_000u64;
    let df = 500u32; // 5 % per 1 ks window

    let decay_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
        &VestingCurve::TimeDecay(df),
    );
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &deposit);
    let linear_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &1u64, // different nonce
        &false, &0u64, &false,
        &VestingCurve::Linear,
    );

    let stream = c.get_stream(&decay_id);
    assert_eq!(stream.options.curve, VestingCurve::TimeDecay(df));

    // Check at several time points that decay ≥ linear.
    for elapsed in [1000u64, 3000, 5000, 7000, 9000, 10_000] {
        t.env.ledger().set_timestamp(elapsed);
        let decay_claimable = c.get_claimable(&decay_id);
        let linear_claimable = c.get_claimable(&linear_id);
        assert!(
            decay_claimable >= linear_claimable,
            "at elapsed={elapsed}: decay={decay_claimable} < linear={linear_claimable}"
        );
    }
}

#[test]
fn test_contract_decay_withdraw_pays_correct_amount() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(0);
    let deposit = 1_000_000i128;
    let duration = 10_000u64;
    let df = 1000u32; // 10 % per window (strong decay for clear numbers)

    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
        &VestingCurve::TimeDecay(df),
    );

    // Advance past 2 windows (t=2000).
    t.env.ledger().set_timestamp(2000);

    let expected = compute_cumulative_decay(deposit, 0, duration, 2000, df).unwrap();

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert!(
        within(balance_after - balance_before, expected, 1),
        "withdraw at t=2000: expected≈{expected}, got {}",
        balance_after - balance_before
    );
}

#[test]
fn test_contract_decay_full_withdrawal_at_end_time() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(0);
    let deposit = 500_000i128;
    let duration = 5_000u64;
    let df = 200u32;

    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
        &VestingCurve::TimeDecay(df),
    );

    // Move to end_time.
    t.env.ledger().set_timestamp(duration);

    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, deposit, "full deposit must be claimable at end_time");

    let balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(
        balance_after - balance_before, deposit,
        "full deposit must be transferred at end_time"
    );
}

#[test]
fn test_contract_simulate_claimable_sequence() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(0);
    let deposit = 1_000_000i128;
    let duration = 10_000u64;
    let df = 300u32;

    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
        &VestingCurve::TimeDecay(df),
    );

    // simulate_claimable is read-only and query_time-driven — ledger time doesn't matter.
    let at_0 = c.simulate_claimable(&stream_id, &0u64);
    let at_2000 = c.simulate_claimable(&stream_id, &2000u64);
    let at_5000 = c.simulate_claimable(&stream_id, &5000u64);
    let at_end = c.simulate_claimable(&stream_id, &duration);
    let beyond = c.simulate_claimable(&stream_id, &(duration + 1000));

    // Monotone increasing.
    assert!(at_0 <= at_2000, "simulate: t=0 <= t=2000");
    assert!(at_2000 <= at_5000, "simulate: t=2000 <= t=5000");
    assert!(at_5000 <= at_end, "simulate: t=5000 <= t=end");

    // Full deposit at and beyond end_time.
    assert_eq!(at_end, deposit, "simulate: at end_time must return full deposit");
    assert_eq!(beyond, deposit, "simulate: beyond end_time must return full deposit");

    // Cross-check with pure-math function.
    let expected_2000 = simulate_claimable(deposit, 0, duration, 2000, 0, df).unwrap();
    assert!(within(at_2000, expected_2000, 1));
}

#[test]
fn test_contract_simulate_claimable_linear_stream() {
    let t = setup_decay();
    let c = decay_client(&t);

    t.env.ledger().set_timestamp(0);
    let deposit = 1_000_000i128;
    let duration = 10_000u64;

    // Use original create_stream (defaults to Linear curve).
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &deposit, &duration, &0u64,
        &0u64, &false, &0u64, &false,
    );

    // At 25 %: 250_000 expected.
    let at_25 = c.simulate_claimable(&stream_id, &2500u64);
    assert_eq!(at_25, 250_000, "linear simulate at 25%");

    // At 100 %: full deposit.
    let at_100 = c.simulate_claimable(&stream_id, &duration);
    assert_eq!(at_100, deposit, "linear simulate at 100%");
}
