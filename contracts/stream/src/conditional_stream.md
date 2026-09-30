# feat/31: Conditional Stream

Closes #632

## Summary

Enable outcome-dependent streams that release funds to the recipient only when
an on-chain oracle condition is met (e.g. a price threshold is reached).

## Acceptance Criteria

- [ ] `create_conditional_stream(recipient, oracle_address, condition)` entry point added
- [ ] On withdrawal, `conditional_stream` calls the oracle to check the condition
- [ ] If condition not met, withdrawal panics with `Error::ConditionNotMet`
- [ ] Unit test: conditional stream releases only when oracle condition is true

## Design Notes

- New `ConditionalStreamConfig` struct: `{ oracle: Address, condition_type: ConditionType, threshold: i128 }`
- `ConditionType` enum: `PriceAbove`, `PriceBelow`
- Stored in `Stream.options` or as a separate persistent key `(Symbol("csc"), stream_id)`
- On `withdraw`: call `IPriceOracle::get_price(token)`, compare to threshold
- If condition not met: return `StreamError::ConditionNotMet` (new error variant)
- Re-uses existing `oracle.rs` `PriceOracleClient` for cross-contract call
- Branch: `feat/31-conditional-stream`
