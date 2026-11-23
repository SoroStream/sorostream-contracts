#![cfg(test)]
//! Issue #763: fuzz/unit coverage for `compute_flow_rate` with
//! `duration_seconds` far outside the range the Kani proofs bound
//! (≤ 315_360_000 s ≈ 10 years), including values near `u64::MAX`.
//!
//! The Kani proofs in `vesting_math.rs` assume `duration > 0 && duration <=
//! 315_360_000`. Nothing in the pure math layer itself enforces that bound, so
//! these tests exercise the unbounded domain directly and pin down the
//! observed behaviour: integer division degrades gracefully to a flow rate of
//! 0 (or a small bounded quotient for huge deposits) instead of panicking or
//! producing an out-of-range value.

use crate::vesting_math::compute_flow_rate;
use proptest::prelude::*;

/// Upper bound assumed by every Kani proof in `vesting_math.rs`.
const KANI_MAX_DURATION: u64 = 315_360_000;

/// A duration at/next to `u64::MAX` is ~584 billion years — no realistic
/// deposit can sustain a non-zero per-second flow rate over it.
#[test]
fn zero_flow_rate_for_duration_near_u64_max() {
    let durations = [
        u64::MAX,
        u64::MAX - 1,
        u64::MAX - 1_000,
        u64::MAX / 2,
        i64::MAX as u64,
    ];
    // Deposits up to ~1e18 stroops are far smaller than every duration above.
    let deposits = [
        0i128,
        1,
        1_000_000,
        1_000_000_000_000,
        1_000_000_000_000_000_000,
    ];

    for deposit in deposits {
        for duration in durations {
            assert_eq!(
                compute_flow_rate(deposit, duration),
                0,
                "deposit {deposit} over duration {duration} must floor to 0"
            );
        }
    }
}

/// The only way `compute_flow_rate` can leave the "degrades to 0" regime is a
/// deposit larger than the duration itself (only possible for deposits ≥
/// ~1.8e19, i.e. approaching `i128::MAX`). Even then the quotient must stay a
/// finite `i128` with `|quotient × duration| ≤ |deposit|`.
#[test]
fn huge_deposit_over_u64_max_duration_stays_bounded() {
    let duration = u64::MAX;
    for deposit in [
        i128::MAX,
        i128::MAX - 1,
        u64::MAX as i128 + 1,
        u64::MAX as i128 * 2,
    ] {
        let rate = compute_flow_rate(deposit, duration);
        let product = rate
            .checked_mul(duration as i128)
            .expect("rate × duration must not overflow i128");
        assert!(
            product.unsigned_abs() <= deposit.unsigned_abs(),
            "|rate × duration| must not exceed |deposit| (deposit {deposit}, rate {rate})"
        );
        assert_eq!(
            rate.signum(),
            deposit.signum(),
            "sign of the flow rate must match the sign of the deposit"
        );
    }
}

/// Just beyond the Kani bound the arithmetic must behave exactly like any
/// other in-range duration (no cliff/edge behaviour at the bound itself).
#[test]
fn durations_just_above_kani_bound_behave_normally() {
    let deposit = 1_000_000_000_000i128;

    let at_bound = compute_flow_rate(deposit, KANI_MAX_DURATION);
    assert_eq!(at_bound, deposit / KANI_MAX_DURATION as i128);

    let above_bound = compute_flow_rate(deposit, KANI_MAX_DURATION + 1);
    assert_eq!(above_bound, deposit / (KANI_MAX_DURATION + 1) as i128);

    // Larger duration → smaller (or equal) floor quotient.
    assert!(above_bound <= at_bound);
}

/// Floor division is non-increasing in the divisor for non-negative deposits —
/// must hold across the whole unbounded duration range, including near
/// `u64::MAX`.
#[test]
fn flow_rate_non_increasing_in_duration_near_u64_max() {
    let deposit = 5_000_000_000_000i128;
    let durations = [
        u64::MAX,
        u64::MAX - 1,
        u64::MAX - 2,
        u64::MAX - 1_000_000,
        u64::MAX / 2 + 1,
        u64::MAX / 2,
        u64::from(u32::MAX),
        KANI_MAX_DURATION,
    ];
    for window in durations.windows(2) {
        let (longer, shorter) = (window[0], window[1]);
        assert!(shorter >= longer, "test durations must be sorted descending");
        assert!(
            compute_flow_rate(deposit, longer) <= compute_flow_rate(deposit, shorter),
            "flow rate must not increase when duration grows ({longer} vs {shorter})"
        );
    }
}

/// `duration_seconds == 0` is a divide-by-zero panic by design. It is rejected
/// long before this layer by `validation::require_positive_duration` (issue
/// #620), so the panic is unreachable from contract entry points — this test
/// pins down that contract so a future "graceful" change is a conscious one.
#[test]
#[should_panic(expected = "attempt to divide by zero")]
fn zero_duration_panics_and_is_rejected_upstream() {
    let _ = compute_flow_rate(1_000_000i128, 0);
}

// ---------------------------------------------------------------------------
// Proptest: whole-domain coverage
// ---------------------------------------------------------------------------

proptest! {
    /// No `(deposit, duration)` pair with a non-zero duration may panic, and
    /// the result must always satisfy `rate × duration ≈ deposit` with the
    /// truncation error bounded by one flow-rate unit.
    #[test]
    fn prop_compute_flow_rate_never_panics_and_stays_bounded(
        deposit: i128,
        duration in 1u64..=u64::MAX,
    ) {
        let rate = compute_flow_rate(deposit, duration);

        let product = rate
            .checked_mul(duration as i128)
            .expect("rate × duration must not overflow i128");
        prop_assert!(
            product.unsigned_abs() <= deposit.unsigned_abs(),
            "|rate × duration| exceeded |deposit|: deposit={deposit} duration={duration} rate={rate}"
        );

        if rate != 0 {
            prop_assert_eq!(rate.signum(), deposit.signum());
        }

        // Truncation residue: deposit - rate × duration is smaller in magnitude
        // than the duration (one whole unit of flow rate).
        let residue = deposit - product;
        prop_assert!(residue.unsigned_abs() < duration as u128);
    }

    /// A deposit smaller than a near-`u64::MAX` duration must always floor to
    /// a flow rate of exactly 0 — this is the "graceful degradation" the
    /// issue asks to confirm. The duration is pinned to the top of the `u64`
    /// range so shrinking keeps any failure at the extreme the issue cares
    /// about; the deposit range (≤ 1e18) is far smaller than every such
    /// duration, so this property must hold for every generated case.
    #[test]
    fn prop_flow_rate_is_zero_when_duration_near_u64_max(
        deposit in 0i128..=1_000_000_000_000_000_000i128,
        offset in 0u64..=1_000_000,
    ) {
        let duration = u64::MAX - offset;
        prop_assert!(i128::from(duration) > deposit);
        prop_assert_eq!(compute_flow_rate(deposit, duration), 0);
    }

    /// Same bound as `prop_compute_flow_rate_never_panics_and_stays_bounded`
    /// but with the duration forced into the last 1 000 000 values of the
    /// `u64` range, so shrinking keeps failures near `u64::MAX`.
    #[test]
    fn prop_compute_flow_rate_bounded_for_duration_near_u64_max(
        deposit: i128,
        offset in 0u64..=1_000_000,
    ) {
        let duration = u64::MAX - offset;
        let rate = compute_flow_rate(deposit, duration);

        let product = rate
            .checked_mul(duration as i128)
            .expect("rate × duration must not overflow i128");
        prop_assert!(product.unsigned_abs() <= deposit.unsigned_abs());

        if rate != 0 {
            prop_assert_eq!(rate.signum(), deposit.signum());
        }

        // Whenever the deposit fits inside the duration, the floor is 0.
        if deposit >= 0 && (deposit as u128) < duration as u128 {
            prop_assert_eq!(rate, 0);
        }
    }
}
