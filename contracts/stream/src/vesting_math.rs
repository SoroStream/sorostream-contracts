/// Pure vesting arithmetic functions extracted for formal verification.
/// These functions have zero Soroban dependencies and operate on primitive types only.

/// Computes the claimable amount with cliff enforcement (for withdrawals).
/// Returns `None` if the arithmetic overflows `i128`.
pub fn compute_claimable(
    flow_rate: i128,
    now: u64,
    cliff_time: u64,
    end_time: u64,
    last_withdraw_time: u64,
) -> Option<i128> {
    if now < cliff_time {
        return Some(0);
    }
    let effective_now = if now < end_time { now } else { end_time };
    let elapsed = effective_now.saturating_sub(last_withdraw_time);
    flow_rate.checked_mul(elapsed as i128)
}

/// Computes earned amount without cliff enforcement (for cancellation paths).
/// Returns `None` if the arithmetic overflows `i128`.
pub fn compute_earned(
    flow_rate: i128,
    now: u64,
    end_time: u64,
    last_withdraw_time: u64,
) -> Option<i128> {
    let effective_now = if now < end_time { now } else { end_time };
    let elapsed = effective_now.saturating_sub(last_withdraw_time);
    flow_rate.checked_mul(elapsed as i128)
}

/// Computes total tokens streamed from start until now (capped at end_time).
/// Returns `None` if the arithmetic overflows `i128`.
pub fn compute_total_streamed(
    flow_rate: i128,
    now: u64,
    end_time: u64,
    start_time: u64,
) -> Option<i128> {
    let effective_now = if now < end_time { now } else { end_time };
    flow_rate.checked_mul(effective_now.saturating_sub(start_time) as i128)
}

/// Computes the sender's refund on cancellation.
/// Returns `None` if the arithmetic overflows `i128`.
pub fn compute_refund(
    deposit: i128,
    flow_rate: i128,
    now: u64,
    end_time: u64,
    start_time: u64,
) -> Option<i128> {
    let total_streamed = compute_total_streamed(flow_rate, now, end_time, start_time)?;
    Some(deposit.saturating_sub(total_streamed))
}

/// Computes flow rate from deposit and duration (integer division, floors).
pub fn compute_flow_rate(deposit: i128, duration_seconds: u64) -> i128 {
    deposit / duration_seconds as i128
}

// ---------------------------------------------------------------------------
// Kani formal verification proofs
// ---------------------------------------------------------------------------
#[cfg(kani)]
mod proofs {
    use super::*;

    /// INVARIANT 1: claimable ≤ total_amount (deposit)
    ///
    /// For any valid stream parameters where flow_rate = deposit / duration,
    /// the claimable amount at any point in time never exceeds the deposit.
    ///
    /// Proof sketch: flow_rate = deposit / duration (integer floor), so
    /// flow_rate * duration ≤ deposit. Since elapsed ≤ duration,
    /// flow_rate * elapsed ≤ flow_rate * duration ≤ deposit.
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_claimable_leq_deposit() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let cliff_seconds: u64 = kani::any();
        let now: u64 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 315_360_000_u64);
        kani::assume(start_time <= u64::MAX / 2);
        kani::assume(cliff_seconds <= duration);
        kani::assume(now >= start_time);

        let flow_rate = compute_flow_rate(deposit, duration);
        kani::assume(flow_rate > 0);

        let end_time = start_time + duration;
        let cliff_time = start_time + cliff_seconds;
        let last_withdraw_time = start_time;

        let claimable =
            compute_claimable(flow_rate, now, cliff_time, end_time, last_withdraw_time).unwrap();

        assert!(claimable <= deposit, "claimable must not exceed deposit");
    }

    /// INVARIANT 1b: claimable ≤ deposit even after partial withdrawals.
    ///
    /// When last_withdraw_time is between start_time and end_time (simulating
    /// prior withdrawals), the claimable amount still cannot exceed deposit.
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_claimable_leq_deposit_after_withdrawal() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let last_withdraw_time: u64 = kani::any();
        let now: u64 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 315_360_000_u64);
        kani::assume(start_time <= u64::MAX / 4);
        kani::assume(last_withdraw_time >= start_time);
        kani::assume(now >= last_withdraw_time);

        let end_time = start_time + duration;
        kani::assume(last_withdraw_time <= end_time);

        let flow_rate = compute_flow_rate(deposit, duration);
        kani::assume(flow_rate > 0);

        let claimable = compute_claimable(flow_rate, now, start_time, end_time, last_withdraw_time).unwrap();

        assert!(claimable <= deposit, "claimable must not exceed deposit after partial withdrawal");
    }

    /// INVARIANT 2: claimable is non-decreasing over time.
    ///
    /// For any two timestamps t1 ≤ t2 with the same stream parameters,
    /// compute_claimable(t2) ≥ compute_claimable(t1).
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_claimable_monotonic() {
        let flow_rate: i128 = kani::any();
        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        let cliff_time: u64 = kani::any();
        let end_time: u64 = kani::any();
        let last_withdraw_time: u64 = kani::any();

        kani::assume(flow_rate > 0 && flow_rate <= 1_000_000_000_i128);
        kani::assume(t2 >= t1);
        kani::assume(end_time <= u64::MAX / 2);
        kani::assume(last_withdraw_time <= end_time);

        let c1 = compute_claimable(flow_rate, t1, cliff_time, end_time, last_withdraw_time).unwrap();
        let c2 = compute_claimable(flow_rate, t2, cliff_time, end_time, last_withdraw_time).unwrap();

        assert!(c2 >= c1, "claimable must be non-decreasing over time");
    }

    /// INVARIANT 3: claimable = 0 before cliff.
    ///
    /// For any timestamp strictly before cliff_time, the claimable amount is zero
    /// regardless of flow_rate, end_time, or last_withdraw_time.
    #[kani::proof]
    fn verify_claimable_zero_before_cliff() {
        let flow_rate: i128 = kani::any();
        let now: u64 = kani::any();
        let cliff_time: u64 = kani::any();
        let end_time: u64 = kani::any();
        let last_withdraw_time: u64 = kani::any();

        kani::assume(flow_rate > 0);
        kani::assume(now < cliff_time);

        let claimable =
            compute_claimable(flow_rate, now, cliff_time, end_time, last_withdraw_time).unwrap();

        assert!(claimable == 0, "claimable must be zero before cliff");
    }

    /// INVARIANT 4: refund + total_streamed = deposit (balance conservation).
    ///
    /// The refund amount plus the total streamed amount equals the deposit,
    /// proving no tokens are created or destroyed during cancellation.
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_cancel_balance_conservation() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let now: u64 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 315_360_000_u64);
        kani::assume(start_time <= u64::MAX / 4);
        kani::assume(now >= start_time);

        let end_time = start_time + duration;
        let flow_rate = compute_flow_rate(deposit, duration);
        kani::assume(flow_rate > 0);

        let total_streamed = compute_total_streamed(flow_rate, now, end_time, start_time).unwrap();
        let refund = compute_refund(deposit, flow_rate, now, end_time, start_time).unwrap();

        assert!(
            total_streamed + refund == deposit,
            "total_streamed + refund must equal deposit"
        );
    }

    /// INVARIANT 5: earned amount is non-negative.
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_earned_non_negative() {
        let flow_rate: i128 = kani::any();
        let now: u64 = kani::any();
        let end_time: u64 = kani::any();
        let last_withdraw_time: u64 = kani::any();

        kani::assume(flow_rate > 0 && flow_rate <= 1_000_000_000_i128);
        kani::assume(end_time <= u64::MAX / 2);

        let earned = compute_earned(flow_rate, now, end_time, last_withdraw_time).unwrap();

        assert!(earned >= 0, "earned must be non-negative");
    }

    /// INVARIANT 6: refund is non-negative.
    #[kani::proof]
    #[kani::unwind(1)]
    fn verify_refund_non_negative() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let now: u64 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 315_360_000_u64);
        kani::assume(start_time <= u64::MAX / 4);
        kani::assume(now >= start_time);

        let end_time = start_time + duration;
        let flow_rate = compute_flow_rate(deposit, duration);
        kani::assume(flow_rate > 0);

        let refund = compute_refund(deposit, flow_rate, now, end_time, start_time).unwrap();

        assert!(refund >= 0, "refund must be non-negative");
    }
}
