#![cfg(test)]

extern crate std;

use crate::vesting_math;
use proptest::prelude::*;

/// Independent "SDK" reference implementation of the vesting claimable calculation.
/// Written with different control flow from the contract's version to maximize
/// the chance of catching implementation divergence.
fn sdk_compute_claimable(
    deposit: i128,
    duration_secs: u64,
    now: u64,
    cliff_time: u64,
    end_time: u64,
    last_withdraw_time: u64,
) -> i128 {
    if now < cliff_time {
        return 0;
    } 
    let rate = deposit / (duration_secs as i128);
    if rate == 0 {
        return 0;
    }
    let capped = core::cmp::min(now, end_time);
    if capped <= last_withdraw_time {
        return 0;
    }
    let secs = capped - last_withdraw_time;
    rate * (secs as i128)
}

/// Independent "SDK" reference for earned amount (no cliff).
fn sdk_compute_earned(
    deposit: i128,
    duration_secs: u64,
    now: u64,
    end_time: u64,
    last_withdraw_time: u64,
) -> i128 {
    let rate = deposit / (duration_secs as i128);
    if rate == 0 {
        return 0;
    }
    let capped = if now > end_time { end_time } else { now };
    if last_withdraw_time >= capped {
        return 0;
    }
    rate * ((capped - last_withdraw_time) as i128)
}

/// Independent "SDK" reference for refund on cancellation.
fn sdk_compute_refund(
    deposit: i128,
    duration_secs: u64,
    now: u64,
    end_time: u64,
    start_time: u64,
) -> i128 {
    let rate = deposit / (duration_secs as i128);
    if rate == 0 {
        return deposit;
    }
    let capped = if now > end_time { end_time } else { now };
    let streamed_secs = if capped > start_time {
        capped - start_time
    } else {
        0
    };
    let total_streamed = rate * (streamed_secs as i128);
    if deposit > total_streamed {
        deposit - total_streamed
    } else {
        0
    }
}

/// Independent "SDK" reference for compute_cumulative_decay using floating-point
/// closed-form arithmetic (higher precision than the contract's fixed-point loop).
///
/// remaining_fraction = (1 - decay_factor / 10_000) ^ k, where k is the number of
/// completed 1 000-second windows.  Vested = deposit × (1 - remaining_fraction).
fn sdk_compute_cumulative_decay(
    deposit: i128,
    start_time: u64,
    end_time: u64,
    query_time: u64,
    decay_factor: u32,
) -> i128 {
    if deposit <= 0 || end_time <= start_time {
        return 0;
    }
    let effective_now = query_time.min(end_time);
    if effective_now <= start_time {
        return 0;
    }
    let decay_factor = decay_factor.min(9_999);
    if decay_factor == 0 {
        // Linear fallback: deposit × elapsed / duration
        let elapsed = effective_now - start_time;
        let duration = end_time - start_time;
        return (deposit as f64 * elapsed as f64 / duration as f64) as i128;
    }
    let elapsed = effective_now - start_time;
    let full_windows = elapsed / 1_000;
    let keep = (10_000 - decay_factor) as f64 / 10_000.0;
    let remaining_fraction = keep.powf(full_windows as f64);
    let vested = deposit as f64 * (1.0 - remaining_fraction);
    // Convergence guarantee: at or past end_time the full deposit is vested.
    if effective_now >= end_time {
        return deposit;
    }
    vested as i128
}

// ── Differential fuzz tests: 1,000,000 iterations ───────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_000_000))]

    /// Contract vs SDK: compute_claimable must agree within 1 stroop.
    #[test]
    fn differential_fuzz_claimable(
        deposit in 1_i128..=1_000_000_000_i128,
        duration in 1_u64..=10_000_000_u64,
        start_time in 0_u64..=1_000_000_u64,
        cliff_offset in 0_u64..=10_000_000_u64,
        elapsed_offset in 0_u64..=20_000_000_u64,
    ) {
        let flow_rate = deposit / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let cliff_offset = cliff_offset.min(duration);
        let end_time = start_time.saturating_add(duration);
        let cliff_time = start_time.saturating_add(cliff_offset);
        let last_withdraw_time = start_time;
        let now = start_time.saturating_add(elapsed_offset);

        let contract_result = vesting_math::compute_claimable(
            flow_rate, now, cliff_time, end_time, last_withdraw_time,
        ).unwrap();
        let sdk_result = sdk_compute_claimable(
            deposit, duration, now, cliff_time, end_time, last_withdraw_time,
        );

        let diff = (contract_result - sdk_result).abs();
        prop_assert!(
            diff <= 1,
            "claimable divergence {} > 1 stroop: contract={}, sdk={}, deposit={}, duration={}, now={}, cliff={}, end={}, lwt={}",
            diff, contract_result, sdk_result, deposit, duration, now, cliff_time, end_time, last_withdraw_time
        );
    }

    /// Contract vs SDK: compute_earned must agree within 1 stroop.
    #[test]
    fn differential_fuzz_earned(
        deposit in 1_i128..=1_000_000_000_i128,
        duration in 1_u64..=10_000_000_u64,
        start_time in 0_u64..=1_000_000_u64,
        elapsed_offset in 0_u64..=20_000_000_u64,
        lwt_offset in 0_u64..=10_000_000_u64,
    ) {
        let flow_rate = deposit / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let end_time = start_time.saturating_add(duration);
        let lwt_offset = lwt_offset.min(duration);
        let last_withdraw_time = start_time.saturating_add(lwt_offset);
        let now = start_time.saturating_add(elapsed_offset);

        let contract_result = vesting_math::compute_earned(
            flow_rate, now, end_time, last_withdraw_time,
        ).unwrap();
        let sdk_result = sdk_compute_earned(
            deposit, duration, now, end_time, last_withdraw_time,
        );

        let diff = (contract_result - sdk_result).abs();
        prop_assert!(
            diff <= 1,
            "earned divergence {} > 1 stroop: contract={}, sdk={}, deposit={}, duration={}, now={}, end={}, lwt={}",
            diff, contract_result, sdk_result, deposit, duration, now, end_time, last_withdraw_time
        );
    }

    /// Contract vs SDK: compute_refund must agree within 1 stroop.
    #[test]
    fn differential_fuzz_refund(
        deposit in 1_i128..=1_000_000_000_i128,
        duration in 1_u64..=10_000_000_u64,
        start_time in 0_u64..=1_000_000_u64,
        elapsed_offset in 0_u64..=20_000_000_u64,
    ) {
        let flow_rate = deposit / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let end_time = start_time.saturating_add(duration);
        let now = start_time.saturating_add(elapsed_offset);

        let contract_result = vesting_math::compute_refund(
            deposit, flow_rate, now, end_time, start_time,
        ).unwrap();
        let sdk_result = sdk_compute_refund(
            deposit, duration, now, end_time, start_time,
        );

        let diff = (contract_result - sdk_result).abs();
        prop_assert!(
            diff <= 1,
            "refund divergence {} > 1 stroop: contract={}, sdk={}, deposit={}, duration={}, now={}, end={}, start={}",
            diff, contract_result, sdk_result, deposit, duration, now, end_time, start_time
        );
    }

    /// Contract vs SDK (f64 closed-form): compute_cumulative_decay must agree
    /// within a tolerance that scales with deposit to absorb fixed-point drift
    /// accumulated over many decay windows.
    #[test]
    fn differential_fuzz_cumulative_decay(
        deposit in 1_i128..=1_000_000_000_i128,
        duration in 1_000_u64..=31_536_000_u64,
        start_time in 0_u64..=1_000_000_u64,
        elapsed_offset in 0_u64..=31_536_000_u64,
        decay_factor in 1_u32..=9_999_u32,
    ) {
        let end_time = start_time.saturating_add(duration);
        let query = start_time.saturating_add(elapsed_offset);

        let contract_result = vesting_math::compute_cumulative_decay(
            deposit, start_time, end_time, query, decay_factor,
        ).unwrap();
        let sdk_result = sdk_compute_cumulative_decay(
            deposit, start_time, end_time, query, decay_factor,
        );

        // Tolerance: measured divergence is a near-constant ~1.1-1.2 ppm of
        // deposit regardless of duration or window count (fixed-point
        // truncation in the contract's `keep_bps^windows` exponentiation vs.
        // the SDK's float reference), so use 3 ppm for safety margin.
        let tolerance = (deposit.abs() * 3 / 1_000_000).max(15);
        let diff = (contract_result - sdk_result).abs();
        prop_assert!(
            diff <= tolerance,
            "cumulative_decay divergence {} > tolerance {}: contract={}, sdk={}, deposit={}, duration={}, start={}, query={}, decay_factor={}",
            diff, tolerance, contract_result, sdk_result, deposit, duration, start_time, query, decay_factor
        );
    }
}

// ── Cross-function consistency: 1,000,000 iterations ────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_000_000))]

    /// earned + refund = deposit for any valid stream parameters.
    #[test]
    fn differential_fuzz_earned_plus_refund_eq_deposit(
        deposit in 1_i128..=1_000_000_000_i128,
        duration in 1_u64..=10_000_000_u64,
        start_time in 0_u64..=1_000_000_u64,
        elapsed_offset in 0_u64..=20_000_000_u64,
    ) {
        let flow_rate = deposit / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let end_time = start_time.saturating_add(duration);
        let now = start_time.saturating_add(elapsed_offset);

        let total_streamed = vesting_math::compute_total_streamed(
            flow_rate, now, end_time, start_time,
        ).unwrap();
        let refund = vesting_math::compute_refund(
            deposit, flow_rate, now, end_time, start_time,
        ).unwrap();

        prop_assert_eq!(
            total_streamed + refund, deposit,
            "total_streamed + refund must equal deposit"
        );
    }
}
