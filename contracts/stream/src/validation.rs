//! Explicit parameter-boundary validation for contract entry points.
//!
//! Contract arguments arrive as XDR-decoded values. XDR guarantees the *wire
//! type* — a `u64` is never negative and an `i128` is always sixteen bytes —
//! but it says nothing about whether the decoded value is meaningful for the
//! operation being invoked. A zero duration, a zero flow rate against a
//! non-zero deposit, a basis-point rate above 100 %, or an over-long identifier
//! string all decode successfully yet are semantically invalid.
//!
//! Every guard in this module returns a typed [`StreamError`] the moment it
//! observes an invalid value, so malformed input is rejected at the entry point
//! and can never reach business logic — and no code path panics on it.
//!
//! Guards are deliberately tiny and side-effect free so entry points can call
//! them directly at their parameter boundary.

use soroban_sdk::String;

use crate::errors::StreamError;

/// Maximum accepted byte length for a federation name.
pub const MAX_FEDERATION_NAME_BYTES: u32 = 64;

/// Maximum accepted byte length for a stream tag.
pub const MAX_STREAM_TAG_BYTES: u32 = 64;

/// Rejects amounts that are zero or negative.
///
/// Used by every entry point that moves value into the contract, so that a
/// decoded `0` or negative `i128` is refused before any token transfer.
pub fn require_positive_amount(amount: i128) -> Result<(), StreamError> {
    if amount <= 0 {
        Err(StreamError::ZeroAmount)
    } else {
        Ok(())
    }
}

/// Rejects a non-positive flow rate.
pub fn require_positive_rate(rate: i128) -> Result<(), StreamError> {
    if rate <= 0 {
        Err(StreamError::ZeroFlowRate)
    } else {
        Ok(())
    }
}

/// Enforces the invariant "a non-zero deposit must accrue at a non-zero rate".
///
/// A stream with a positive deposit but a zero (or negative) flow rate would
/// never release anything while still locking the sender's funds.
pub fn require_rate_for_deposit(deposit: i128, flow_rate: i128) -> Result<(), StreamError> {
    if deposit > 0 && flow_rate <= 0 {
        Err(StreamError::ZeroFlowRate)
    } else {
        Ok(())
    }
}

/// Rejects a zero duration.
///
/// Durations decode as `u64`, so zero is the only degenerate value XDR can
/// deliver in this type; it would produce `end_time == start_time`.
pub fn require_positive_duration(duration_seconds: u64) -> Result<(), StreamError> {
    if duration_seconds == 0 {
        Err(StreamError::InvalidDuration)
    } else {
        Ok(())
    }
}

/// Rejects a duration longer than the configured maximum.
///
/// `cap_seconds == 0` means "no cap configured" and disables the check.
pub fn require_duration_within_cap(
    duration_seconds: u64,
    cap_seconds: u64,
) -> Result<(), StreamError> {
    if cap_seconds > 0 && duration_seconds > cap_seconds {
        Err(StreamError::DurationExceedsMax)
    } else {
        Ok(())
    }
}

/// Rejects a holdback that is negative or consumes the whole deposit.
///
/// A holdback must leave a strictly positive amount to stream.
pub fn require_holdback_in_range(
    amount: i128,
    holdback_amount: i128,
) -> Result<(), StreamError> {
    if holdback_amount < 0 || holdback_amount >= amount {
        Err(StreamError::ZeroAmount)
    } else {
        Ok(())
    }
}

/// Rejects a cliff that extends past the end of the stream.
pub fn require_cliff_within_duration(
    cliff_seconds: u64,
    duration_seconds: u64,
) -> Result<(), StreamError> {
    if cliff_seconds > duration_seconds {
        Err(StreamError::InvalidCliff)
    } else {
        Ok(())
    }
}

/// Rejects a withdrawal-step count of zero.
///
/// `None` means "no step gating"; `Some(0)` is nonsensical and is refused
/// rather than silently treated as free-form withdrawal.
pub fn require_withdrawal_steps(steps: Option<u32>) -> Result<(), StreamError> {
    match steps {
        Some(0) => Err(StreamError::InvalidDuration),
        _ => Ok(()),
    }
}

/// Rejects a minimum-withdrawal floor that is not strictly positive.
pub fn require_min_withdrawal_amount(floor: Option<i128>) -> Result<(), StreamError> {
    match floor {
        Some(f) if f <= 0 => Err(StreamError::ZeroAmount),
        _ => Ok(()),
    }
}

/// Rejects a basis-point rate above 100 % (`10_000` bps).
pub fn require_valid_fee_bps(fee_bps: u32) -> Result<(), StreamError> {
    require_bps_within(fee_bps, 10_000, StreamError::InvalidDuration)
}

/// Rejects a basis-point value above `max_bps`, reporting `err` on failure.
pub fn require_bps_within(
    bps: u32,
    max_bps: u32,
    err: StreamError,
) -> Result<(), StreamError> {
    if bps > max_bps {
        Err(err)
    } else {
        Ok(())
    }
}

/// Rejects an empty or over-long identifier string.
///
/// Strings arrive with a declared byte length, so this is the boundary at which
/// an over-long "asset code"/federation-style identifier must be refused — it
/// would otherwise be stored and echoed back to clients indefinitely.
pub fn require_bounded_string(value: &String, max_bytes: u32) -> Result<(), StreamError> {
    let len = value.len();
    if len == 0 || len > max_bytes {
        Err(StreamError::InvalidParameter)
    } else {
        Ok(())
    }
}

/// Rejects vectors whose lengths must agree.
pub fn require_matching_lengths(expected: u32, actual: u32) -> Result<(), StreamError> {
    if expected != actual {
        Err(StreamError::BatchLengthMismatch)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn positive_amount_rejects_zero_and_negative() {
        assert!(require_positive_amount(1).is_ok());
        assert_eq!(require_positive_amount(0), Err(StreamError::ZeroAmount));
        assert_eq!(require_positive_amount(-1), Err(StreamError::ZeroAmount));
    }

    #[test]
    fn positive_rate_rejects_zero_and_negative() {
        assert!(require_positive_rate(1).is_ok());
        assert_eq!(require_positive_rate(0), Err(StreamError::ZeroFlowRate));
        assert_eq!(require_positive_rate(-5), Err(StreamError::ZeroFlowRate));
    }

    #[test]
    fn rate_for_deposit_only_guards_non_zero_deposits() {
        // A zero deposit with a zero rate is harmless (e.g. milestone streams).
        assert!(require_rate_for_deposit(0, 0).is_ok());
        assert!(require_rate_for_deposit(100, 1).is_ok());
        assert_eq!(
            require_rate_for_deposit(100, 0),
            Err(StreamError::ZeroFlowRate)
        );
    }

    #[test]
    fn positive_duration_rejects_zero() {
        assert!(require_positive_duration(1).is_ok());
        assert_eq!(
            require_positive_duration(0),
            Err(StreamError::InvalidDuration)
        );
    }

    #[test]
    fn duration_cap_is_only_enforced_when_configured() {
        assert!(require_duration_within_cap(10, 0).is_ok());
        assert!(require_duration_within_cap(10, 10).is_ok());
        assert_eq!(
            require_duration_within_cap(11, 10),
            Err(StreamError::DurationExceedsMax)
        );
    }

    #[test]
    fn holdback_must_leave_something_to_stream() {
        assert!(require_holdback_in_range(100, 0).is_ok());
        assert!(require_holdback_in_range(100, 99).is_ok());
        assert_eq!(
            require_holdback_in_range(100, 100),
            Err(StreamError::ZeroAmount)
        );
        assert_eq!(
            require_holdback_in_range(100, -1),
            Err(StreamError::ZeroAmount)
        );
    }

    #[test]
    fn cliff_must_not_exceed_duration() {
        assert!(require_cliff_within_duration(10, 10).is_ok());
        assert_eq!(
            require_cliff_within_duration(11, 10),
            Err(StreamError::InvalidCliff)
        );
    }

    #[test]
    fn withdrawal_steps_and_floor_boundaries() {
        assert!(require_withdrawal_steps(None).is_ok());
        assert!(require_withdrawal_steps(Some(1)).is_ok());
        assert_eq!(
            require_withdrawal_steps(Some(0)),
            Err(StreamError::InvalidDuration)
        );

        assert!(require_min_withdrawal_amount(None).is_ok());
        assert!(require_min_withdrawal_amount(Some(1)).is_ok());
        assert_eq!(
            require_min_withdrawal_amount(Some(0)),
            Err(StreamError::ZeroAmount)
        );
    }

    #[test]
    fn fee_bps_cannot_exceed_one_hundred_percent() {
        assert!(require_valid_fee_bps(0).is_ok());
        assert!(require_valid_fee_bps(10_000).is_ok());
        assert_eq!(
            require_valid_fee_bps(10_001),
            Err(StreamError::InvalidDuration)
        );
    }

    #[test]
    fn bounded_string_rejects_empty_and_over_long_values() {
        let env = Env::default();
        let ok = String::from_str(&env, "USDC");
        let over = String::from_str(
            &env,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        );
        let empty = String::from_str(&env, "");

        assert!(require_bounded_string(&ok, 64).is_ok());
        assert_eq!(
            require_bounded_string(&empty, 64),
            Err(StreamError::InvalidParameter)
        );
        assert_eq!(
            require_bounded_string(&over, 64),
            Err(StreamError::InvalidParameter)
        );
    }

    #[test]
    fn matching_lengths_rejects_mismatch() {
        assert!(require_matching_lengths(3, 3).is_ok());
        assert_eq!(
            require_matching_lengths(3, 2),
            Err(StreamError::BatchLengthMismatch)
        );
    }
}
