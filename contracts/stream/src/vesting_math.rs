//! Pure vesting arithmetic functions extracted for formal verification.
//! These functions have zero Soroban dependencies and operate on primitive types only.

// ---------------------------------------------------------------------------
// Original linear helpers
// ---------------------------------------------------------------------------

/// Computes the claimable amount from milestones (all released milestones sum).
/// Returns the total amount from milestones marked as released.
pub fn compute_claimable_from_milestones(milestones: &[(i128, bool)]) -> i128 {
    let mut total = 0i128;
    for (amount, is_released) in milestones {
        if *is_released {
            total = total.saturating_add(*amount);
        }
    }
    total
}

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

#[cfg(test)]
mod issue_841_tests {
    use super::*;

    #[test]
    fn compute_refund_before_start_returns_full_deposit() {
        // now strictly before start_time: nothing streamed yet.
        let deposit = 1000i128;
        let duration = 100u64;
        let start_time = 1000u64;
        let now = 500u64; // before start
        let end_time = start_time + duration;
        let flow_rate = compute_flow_rate(deposit, duration);
        let refund = compute_refund(deposit, flow_rate, now, end_time, start_time).unwrap();
        assert_eq!(refund, deposit, "refund must be full deposit when now < start_time");
    }
}

pub fn compute_flow_rate(deposit: i128, duration_seconds: u64) -> i128 {
    deposit / duration_seconds as i128
}

// ---------------------------------------------------------------------------
// Cost comparison: Linear vs TimeDecay
// ---------------------------------------------------------------------------
//
// On-chain resource/gas implications for a stream creator choosing between the
// two curves (see [`VestingCurve`] in types.rs):
//
// | Curve     | On-chain work per `withdraw`/`compute_claimable` call           | Storage reads | Best for                            |
// |-----------|-----------------------------------------------------------------|---------------|-------------------------------------|
// | Linear    | Single multiply: `flow_rate × elapsed`                          | 0 (pure math) | Long-duration streams; lowest cost  |
// | TimeDecay | Iterative loop: one `checked_mul` + `checked_div` per 1 ks window | 0 (pure math) | Front-weighted vesting; higher cost |
//
// Linear is O(1) — one multiplication regardless of stream length, so it is
// the cheapest option and the safe default for multi-year streams.
//
// TimeDecay is O(windows) where `windows = elapsed / DECAY_WINDOW_SECS`.
// Each window iteration performs a fixed-point multiply and division, so cost
// grows linearly with stream duration. For a 1-year stream (~31.5M seconds)
// that is ~31 500 loop iterations — roughly 30 000× the arithmetic of Linear.
// Use TimeDecay only when front-weighted release is genuinely required, and
// prefer shorter durations or larger windows to keep the iteration count low.
//
// Both curves are pure arithmetic (zero storage reads) and `decay_factor == 0`
// degenerates to the linear formula, so a TimeDecay stream with a zero factor
// costs the same as Linear.
//
// # Convergence guarantee
//
// When `query_time >= end_time` both curves return exactly `deposit`, so the
// full amount is always reachable at stream end regardless of curve choice.

// ---------------------------------------------------------------------------
// Time-decay (exponential front-weighting) helpers
// ---------------------------------------------------------------------------

/// Window size for the discretised decay computation (1 000 seconds).
///
/// The decay factor is applied once per window. Smaller windows give a
/// smoother curve but cost more arithmetic; computation uses logarithmic
/// exponentiation, so long streams do not add per-window invocation cost.
pub const DECAY_WINDOW_SECS: u64 = 1_000;

/// Fixed-point scale factor used internally (10^9 = 1 billion).
///
/// All intermediate `remaining_weight` values are kept multiplied by this
/// scale to preserve precision through many iterations of integer division.
const SCALE: i128 = 1_000_000_000;

fn fixed_point_pow(base: i128, mut exponent: u64) -> Option<i128> {
    let mut result = SCALE;
    let mut factor = base;

    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result.checked_mul(factor)?.checked_div(SCALE)?;
        }
        exponent >>= 1;
        if exponent > 0 {
            factor = factor.checked_mul(factor)?.checked_div(SCALE)?;
        }
    }

    Some(result)
}

/// Computes the **cumulative** amount vested from `start_time` up to `query_time`
/// under an exponential time-decay curve.
///
/// # Formula
///
/// The remaining (un-vested) fraction after `k` completed 1 ks windows is:
///
/// ```text
/// remaining_fraction = (1 - decay_factor / 10_000) ^ k
/// ```
///
/// Expressed in fixed-point integer arithmetic (scaled by `SCALE`):
///
/// ```text
/// remaining_scaled[0] = SCALE
/// remaining_scaled[i] = remaining_scaled[i-1] × (10_000 - decay_factor) / 10_000
/// ```
///
/// Cumulative vested at time `t`:
///
/// ```text
/// vested(t) = deposit × (SCALE - remaining_scaled[windows(t)]) / SCALE
/// ```
///
/// **Convergence guarantee**: when `query_time >= end_time` the function returns
/// exactly `deposit`, so the full amount is always reachable at stream end.
///
/// **Linear fallback**: `decay_factor == 0` ⟹ `remaining_scaled` stays at
/// `SCALE` for every window, so `vested(t)` falls back to the linear formula
/// `deposit × elapsed / duration`.
///
/// # Parameters
/// - `deposit`      – total tokens locked (stroops)
/// - `start_time`   – stream start timestamp
/// - `end_time`     – stream end timestamp
/// - `query_time`   – timestamp to evaluate at (clamped to end_time internally)
/// - `decay_factor` – bps per 1 000-second window (0–9 999)
///
/// Returns `None` on arithmetic overflow.
pub fn compute_cumulative_decay(
    deposit: i128,
    start_time: u64,
    end_time: u64,
    query_time: u64,
    decay_factor: u32,
) -> Option<i128> {
    if decay_factor >= 10_000 {
        return None;
    }
    if deposit <= 0 || end_time <= start_time {
        return Some(0);
    }
    let duration = end_time - start_time;

    // Clamp to end_time — at or beyond end_time the full deposit is vested.
    let effective_now = query_time.min(end_time);

    if effective_now <= start_time {
        return Some(0);
    }

    // Linear fallback: decay_factor == 0.
    if decay_factor == 0 {
        let elapsed = effective_now - start_time;
        // deposit × elapsed / duration  (no overflow path for reasonable deposits)
        return deposit
            .checked_mul(elapsed as i128)?
            .checked_div(duration as i128);
    }

    let decay_factor = decay_factor as i128;
    let keep_bps: i128 = 10_000 - decay_factor; // how many bps of remainder survives each window

    let elapsed = effective_now - start_time;

    // Full windows completed so far.
    let full_windows = elapsed / DECAY_WINDOW_SECS;

    // Keep fixed-point work bounded to O(log(full_windows)).
    let keep_scaled = keep_bps.checked_mul(SCALE)?.checked_div(10_000)?;
    let remaining_scaled = fixed_point_pow(keep_scaled, full_windows)?;

    // Fraction vested = (SCALE - remaining_scaled) / SCALE
    // vested = deposit × (SCALE - remaining_scaled) / SCALE
    let vested_scaled = SCALE.checked_sub(remaining_scaled)?;
    let vested = deposit
        .checked_mul(vested_scaled)?
        .checked_div(SCALE)?;

    // Convergence guarantee: at end_time return full deposit.
    if effective_now >= end_time {
        return Some(deposit);
    }

    // Clamp to [0, deposit] to guard against any edge-case integer drift.
    Some(vested.max(0).min(deposit))
}

/// Computes the **incremental** claimable amount since `last_withdraw_time`
/// under a time-decay curve, with cliff enforcement.
///
/// Returns the difference:
///   `cumulative_decay(now) - cumulative_decay(last_withdraw_time)`
///
/// This is always ≥ 0 because `compute_cumulative_decay` is monotone.
/// Returns `None` on arithmetic overflow.
pub fn compute_claimable_decay(
    deposit: i128,
    start_time: u64,
    end_time: u64,
    now: u64,
    cliff_time: u64,
    last_withdraw_time: u64,
    decay_factor: u32,
) -> Option<i128> {
    if now < cliff_time {
        return Some(0);
    }

    let effective_now = now.min(end_time);

    // Convergence guarantee: at or after end_time the recipient may claim everything
    // that has not yet been withdrawn.
    if effective_now >= end_time {
        let cumulative_at_end = deposit; // full deposit
        let cumulative_at_last = compute_cumulative_decay(
            deposit, start_time, end_time, last_withdraw_time, decay_factor,
        )?;
        let claimable = cumulative_at_end.saturating_sub(cumulative_at_last);
        return Some(claimable.max(0));
    }

    let cumulative_now = compute_cumulative_decay(
        deposit, start_time, end_time, effective_now, decay_factor,
    )?;
    let cumulative_last = compute_cumulative_decay(
        deposit, start_time, end_time, last_withdraw_time, decay_factor,
    )?;

    Some(cumulative_now.saturating_sub(cumulative_last).max(0))
}

/// Off-chain preview utility: returns the **cumulative** amount that would be
/// claimable (from the start of the stream) at `query_time` under a time-decay
/// curve.
///
/// Does not require a `last_withdraw_time` — it gives the total vested amount
/// accumulated since stream inception, useful for building unlock schedule UIs.
///
/// Returns `None` on arithmetic overflow.
pub fn simulate_claimable(
    deposit: i128,
    start_time: u64,
    end_time: u64,
    query_time: u64,
    cliff_time: u64,
    decay_factor: u32,
) -> Option<i128> {
    if query_time < cliff_time {
        return Some(0);
    }
    compute_cumulative_decay(deposit, start_time, end_time, query_time, decay_factor)
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
        kani::assume(cliff_seconds < duration);
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

    /// Time-decay cumulative vesting remains within the deposited amount.
    #[kani::proof]
    #[kani::unwind(12)]
    fn verify_decay_cumulative_bounds() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let query_time: u64 = kani::any();
        let decay_factor: u32 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 1_000_000_u64);
        kani::assume(start_time <= u64::MAX / 2);
        kani::assume(decay_factor <= 9_999);

        let end_time = start_time + duration;
        let vested = compute_cumulative_decay(
            deposit, start_time, end_time, query_time, decay_factor,
        )
        .unwrap();

        assert!(vested >= 0 && vested <= deposit);
    }

    /// Time-decay cumulative vesting is monotone for valid factors.
    #[kani::proof]
    #[kani::unwind(12)]
    fn verify_decay_cumulative_monotonic() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let first_time: u64 = kani::any();
        let second_time: u64 = kani::any();
        let decay_factor: u32 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 1_000_000_u64);
        kani::assume(start_time <= u64::MAX / 2);
        kani::assume(second_time >= first_time);
        kani::assume(decay_factor <= 9_999);

        let end_time = start_time + duration;
        let first = compute_cumulative_decay(
            deposit, start_time, end_time, first_time, decay_factor,
        )
        .unwrap();
        let second = compute_cumulative_decay(
            deposit, start_time, end_time, second_time, decay_factor,
        )
        .unwrap();

        assert!(second >= first);
    }

    /// Incremental time-decay claims remain non-negative and deposit-bounded.
    #[kani::proof]
    #[kani::unwind(12)]
    fn verify_decay_claimable_bounds() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let now: u64 = kani::any();
        let cliff_offset: u64 = kani::any();
        let decay_factor: u32 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 1_000_000_u64);
        kani::assume(start_time <= u64::MAX / 2);
        kani::assume(cliff_offset <= duration);
        kani::assume(now >= start_time + cliff_offset);
        kani::assume(decay_factor <= 9_999);

        let end_time = start_time + duration;
        let claimable = compute_claimable_decay(
            deposit,
            start_time,
            end_time,
            now,
            start_time + cliff_offset,
            start_time,
            decay_factor,
        )
        .unwrap();

        assert!(claimable >= 0 && claimable <= deposit);
    }

    /// The preview helper enforces the cliff and matches cumulative vesting after it.
    #[kani::proof]
    #[kani::unwind(12)]
    fn verify_decay_simulation_cliff_and_cumulative() {
        let deposit: i128 = kani::any();
        let duration: u64 = kani::any();
        let start_time: u64 = kani::any();
        let query_time: u64 = kani::any();
        let cliff_offset: u64 = kani::any();
        let decay_factor: u32 = kani::any();

        kani::assume(deposit > 0 && deposit <= 1_000_000_000_000_i128);
        kani::assume(duration > 0 && duration <= 1_000_000_u64);
        kani::assume(start_time <= u64::MAX / 2);
        kani::assume(cliff_offset <= duration);
        kani::assume(decay_factor <= 9_999);

        let end_time = start_time + duration;
        let cliff_time = start_time + cliff_offset;
        let preview = simulate_claimable(
            deposit, start_time, end_time, query_time, cliff_time, decay_factor,
        )
        .unwrap();

        if query_time < cliff_time {
            assert!(preview == 0);
        } else {
            let cumulative = compute_cumulative_decay(
                deposit, start_time, end_time, query_time, decay_factor,
            )
            .unwrap();
            assert!(preview == cumulative);
        }
    }
}
