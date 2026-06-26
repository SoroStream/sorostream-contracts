# ADR-0001: Use `i128` for token amounts

## Status

Accepted

## Context

The SoroStream contract stores and transfers token amounts (deposits, flow rates, claimable balances). Soroban's `token::Client` interface defines `transfer`, `balance`, and related functions using `i128` as the amount type. We needed to choose the internal representation for amounts stored in the `Stream` struct and used in arithmetic (flow rate calculation, elapsed-time multiplication, refund computation).

The candidates were:

- `u128` — unsigned, matching the semantic that amounts are never negative.
- `i128` — signed, matching the Soroban token interface directly.
- `u64` / `i64` — smaller types that would require casting at every token interaction.

## Decision

Use `i128` for all amount fields (`deposit`, `flow_rate`, and intermediate calculations like `claimable`, `refund_amount`).

The primary reason is **interface alignment**: Soroban's SAC token standard uses `i128` for all amount parameters. Using a different type internally would require casting at every `transfer` and `balance` call, introducing conversion noise and potential truncation bugs. Since the token interface is the contract's most critical external boundary, matching its type eliminates an entire class of errors.

Negative amounts are rejected at the contract boundary (`amount <= 0` checks in `create_stream`, `top_up`, and `partial_cancel_stream`), so the signed range is never exercised for valid state.

## Alternatives considered

### `u128`

Would express the "amounts are non-negative" invariant in the type system. Rejected because every token SDK call would require `as i128` casts, and the Soroban SDK's `contracttype` serialization for `u128` is identical in size to `i128`. The safety benefit is marginal given the explicit `<= 0` guards already present.

### `u64`

Would cap amounts at ~18.4 × 10^18 stroops (~1.84 billion XLM). While sufficient for most current use cases, this creates an artificial ceiling that could break with high-value tokens or tokens with more decimal places. The Soroban VM handles `i128` natively with no extra cost compared to `i64`.

## Consequences

### Positive

- Zero-cost interop with `token::Client` — no casts, no truncation risk.
- Supports the full range of any Soroban token, including future tokens with large supplies.
- `flow_rate` arithmetic (`amount / duration as i128`, `flow_rate * elapsed as i128`) works without type conversion.

### Negative

- The signed type does not statically prevent negative values; this invariant is enforced by runtime checks at contract entry points.
- `i128` fields in the `Stream` struct are 16 bytes each, slightly larger than `i64` (8 bytes). With two `i128` fields (`deposit`, `flow_rate`), the per-stream overhead is 16 extra bytes — negligible relative to the three `Address` fields (~90 bytes total).

### Neutral

- Soroban's XDR encoding for `i128` and `u128` is the same size (16 bytes), so switching to `u128` would not save storage.
