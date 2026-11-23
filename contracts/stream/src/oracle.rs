//! Oracle integration for SoroStream.
//!
//! Defines the `IPriceOracle` cross-contract interface and the `check_price_deviation`
//! helper used on `create_stream` and `withdraw` when a stream has an oracle attached.

use soroban_sdk::{contractclient, Address, Env};

use crate::errors::StreamError;

/// Minimal cross-contract interface every price oracle must expose.
///
/// Implementors return the current price of `token` as a raw `i128` value.
/// The unit and scaling are oracle-specific but must be consistent across calls
/// (e.g. price in USD with 7 decimal places of precision).
#[contractclient(name = "PriceOracleClient")]
pub trait IPriceOracle {
    /// Returns the current price of `token`.
    ///
    /// # Returns
    /// A positive `i128` representing the token price.
    /// Zero and negative values are both treated as oracle faults and produce
    /// `StreamError::InvalidSlippage`; the caller does not distinguish them.
    fn get_price(env: Env, token: Address) -> i128;
}

/// Fetches the current price from the oracle contract.
///
/// Returns `StreamError::InvalidSlippage` if the call fails or the returned price is zero
/// or negative. Both nonpositive values produce the same error before deviation is checked.
pub fn fetch_price(env: &Env, oracle: &Address, token: &Address) -> Result<i128, StreamError> {
    let client = PriceOracleClient::new(env, oracle);
    let price = client.try_get_price(token).map_err(|_| StreamError::InvalidSlippage)?;
    let price = price.map_err(|_| StreamError::InvalidSlippage)?;
    if price <= 0 {
        return Err(StreamError::InvalidSlippage);
    }
    Ok(price)
}

/// Validates that `current_price` is within `max_deviation_bps` of `creation_price`.
///
/// Deviation is computed as:
///   `deviation_bps = abs(current - creation) * 10_000 / creation`
///
/// Returns `StreamError::InvalidSlippage` when the threshold is breached or either price is
/// nonpositive. In `check_oracle`, `fetch_price` rejects a zero or negative current price
/// before this deviation check, using the same error for both.
pub fn assert_price_within_bounds(
    creation_price: i128,
    current_price: i128,
    max_deviation_bps: u32,
) -> Result<u32, StreamError> {
    if creation_price <= 0 || current_price <= 0 {
        return Err(StreamError::InvalidSlippage);
    }
    let diff = (current_price - creation_price).abs();
    // deviation_bps = diff * 10_000 / creation_price  (integer arithmetic, floors)
    let deviation_bps = diff
        .checked_mul(10_000)
        .ok_or(StreamError::Overflow)?
        .checked_div(creation_price)
        .ok_or(StreamError::InvalidSlippage)? as u32;

    if deviation_bps > max_deviation_bps {
        return Err(StreamError::InvalidSlippage);
    }
    Ok(deviation_bps)
}

/// Convenience: fetch price then assert it is within bounds.
///
/// Used by `create_stream` (pass `creation_price = current` so it always passes,
/// returning the price to store) and `withdraw` (pass stored `creation_price`).
pub fn check_oracle(
    env: &Env,
    oracle: &Address,
    token: &Address,
    creation_price: i128,
    max_deviation_bps: u32,
) -> Result<(i128, u32), StreamError> {
    let current_price = fetch_price(env, oracle, token)?;
    let deviation_bps = assert_price_within_bounds(creation_price, current_price, max_deviation_bps)?;
    Ok((current_price, deviation_bps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonpositive_prices_are_rejected_with_the_same_error() {
        assert_eq!(
            assert_price_within_bounds(100, 0, 10_000),
            Err(StreamError::InvalidSlippage)
        );
        assert_eq!(
            assert_price_within_bounds(100, -1, 10_000),
            Err(StreamError::InvalidSlippage)
        );
    }
}
