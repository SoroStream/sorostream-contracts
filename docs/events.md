# SoroStream Contract Event Schema Reference

This document is the authoritative reference for all events emitted by the SoroStream stream contract. It is intended for off-chain indexer developers, SDK authors, and frontend engineers who consume contract events from the Stellar network.

For XDR encoding details and base64 decoding examples, see [`docs/events.md`](./docs/events.md).

---

## Backward Compatibility Policy

- **Field names and positions are stable.** Existing event fields will not be removed or reordered in a backward-incompatible way without a major version bump.
- **New fields may be appended** to the data tuple in minor releases. Consumers must handle additional trailing fields gracefully.
- **Topic count for a given event name is fixed.** Events with 2 topics always have 2 topics; events with 1 topic always have 1 topic.
- **Event names are immutable.** A renamed event is treated as a new event; the old name continues to be emitted for one major version before removal.
- Contract version is emitted in the `ContractDeployed` event at initialization.

---

## Quick Reference Table

| # | Event | Topics | Data fields | Emitted by |
|---|-------|:------:|-------------|------------|
| 1 | `StreamCreated` | 2 | sender, recipient, amount, flow_rate, end_time | `create_stream`, `batch_create_stream` |
| 2 | `StreamWithdrawn` | 2 | recipient, amount, timestamp | `withdraw`, `batch_withdraw` |
| 3 | `StreamCancelled` | 2 | sender, refund_amount, recipient_amount | `cancel_stream`, `batch_cancel_stream` |
| 4 | `StreamToppedUp` | 2 | added_amount, new_end_time | `top_up` |
| 5 | `StreamCompleted` | 2 | _(none)_ | `withdraw` (at end_time) |
| 6 | `StreamPaused` | 2 | sender | `pause_stream` |
| 7 | `StreamResumed` | 2 | sender | `resume_stream` |
| 8 | `StreamPartialCancelled` | 2 | new_stream_id, sender, refund_amount, new_deposit | `partial_cancel_stream` |
| 9 | `StreamTerminatedByRecipient` | 2 | recipient, recipient_amount, refund_amount | `recipient_terminate` |
| 10 | `RecipientTransferred` | 2 | old_recipient, new_recipient | `transfer_recipient` |
| 11 | `StreamArchived` | 2 | sender, recipient, total_amount | `archive_stream` |
| 12 | `MetadataUpdated` | 2 | metadata | `update_metadata` |
| 13 | `MetadataUriUpdated` | 2 | metadata_uri | `update_metadata_uri` |
| 14 | `AutoRenewCancelled` | 2 | _(none)_ | `cancel_auto_renew` |
| 15 | `AutoRenewFailed` | 2 | sender, required | `withdraw` (auto-renew path) |
| 16 | `StreamRenewed` | 2 | new_stream_id | `withdraw` (auto-renew path) |
| 17 | `StreamExpired` | 2 | _(none)_ | `mark_expired` |
| 18 | `StreamSwept` | 2 | caller | `sweep_expired` |
| 19 | `TtlBumped` | 2 | new_expiry_ledger | `bump_ttl` |
| 20 | `MilestoneReleased` | 2 | milestone_index | `release_milestone` |
| 21 | `HoldbackReleased` | 2 | amount, recipient | `release_holdback` |
| 22 | `HoldbackClawedBack` | 2 | amount, sender | `claw_back_holdback` |
| 23 | `TrancheStreamCreated` | 2 | sender, tranche_count, total_amount | `create_stream` (step-vesting) |
| 24 | `TranchesWithdrawn` | 2 | recipient, tranches_claimed, amount | `withdraw` (step-vesting) |
| 25 | `TrancheStreamCancelled` | 2 | sender, unclaimed_tranche_refund, recipient_amount | `cancel_stream` (step-vesting) |
| 26 | `DelegateSet` | 2 | sender, delegate | `set_delegate` |
| 27 | `DelegateRevoked` | 2 | sender | `revoke_delegate` |
| 28 | `FeeCollected` | 2 | amount, treasury | `withdraw`, `transfer_recipient` |
| 29 | `CreationFeeCollected` | 1 | fee_amount, treasury | `create_stream` |
| 30 | `FeeChangeProposed` | 1 | new_fee, unlock_time | `propose_fee_change` |
| 31 | `FeeChangeExecuted` | 1 | new_fee | `execute_fee_change` |
| 32 | `FeeSwept` | 1 | token, amount, destination | `sweep_fees` |
| 33 | `PriceCheckPassed` | 2 | token, price, deviation_bps | `create_stream`, `withdraw` (oracle path) |
| 34 | `SlippageExceeded` | 2 | current_price, max_slippage_bps | `create_stream`, `withdraw` (oracle path) |
| 35 | `SlippageWarning` | 2 | current_deviation_bps, max_slippage_bps | `create_stream`, `withdraw` (oracle path) |
| 36 | `RateLimitExceeded` | 1 | sender | `create_stream` |
| 37 | `RateLimitUpdated` | 1 | window_seconds, max_creations | `set_rate_limit` |
| 38 | `TokenWhitelisted` | 1 | token | `whitelist_token` |
| 39 | `TokenDewhitelisted` | 1 | token | `remove_token` |
| 40 | `TokenWhitelistToggled` | 1 | enabled | `toggle_whitelist` |
| 41 | `FederationRegistered` | 1 | federation_name, stellar_address | `register_federation` |
| 42 | `FederationUnregistered` | 1 | federation_name | `unregister_federation` |
| 43 | `ContractDeployed` | 1 | version, admin | `initialize` |
| 44 | `ContractPaused` | 2 | timestamp | `emergency_pause` |
| 45 | `ContractResumed` | 2 | timestamp | `emergency_resume` |
| 46 | `ContractMigrated` | 1 | from_version, to_version, admin | `migrate` |
| 47 | `AdminAction` | 1 | instruction, admin, timestamp | `emergency_pause`, `emergency_resume`, `migrate` |

---

## Type Reference

| Rust type | XDR ScVal variant | Description |
|-----------|------------------|-------------|
| `u32` | `ScvU32` | 32-bit unsigned integer |
| `u64` | `ScvU64` | 64-bit unsigned integer (timestamps, stream IDs) |
| `i128` | `ScvI128` | 128-bit signed integer (token amounts in stroops) |
| `bool` | `ScvBool` | Boolean flag |
| `Address` | `ScvAddress` | Stellar account or contract address |
| `String` | `ScvString` | UTF-8 string |
| `Bytes` | `ScvBytes` | Raw byte array |
| `Symbol` | `ScvSymbol` | Short ASCII identifier (topics[0] event name) |
| tuple `(A,B,…)` | `ScvVec` | One element per tuple field |
| `()` | `ScvVoid` | Empty / no data |

All token amounts are in **stroops** (1 XLM = 10,000,000 stroops).  
Timestamps are **Unix seconds** as returned by `env.ledger().timestamp()`.

---
## Stream Lifecycle Events

### 1. `StreamCreated`

**Trigger:** A new stream is successfully created by `create_stream` or `batch_create_stream`. Tokens are locked in the contract at this point.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamCreated"` |
| topics[1] | `stream_id` | `u64` | Unique stream identifier |
| data[0] | `sender` | `Address` | Stream creator / payer |
| data[1] | `recipient` | `Address` | Stream beneficiary |
| data[2] | `amount` | `i128` | Total deposit locked (stroops) |
| data[3] | `flow_rate` | `i128` | Tokens released per second (`amount / duration_seconds`) |
| data[4] | `end_time` | `u64` | Unix timestamp when the stream ends |

---

### 2. `StreamWithdrawn`

**Trigger:** A recipient calls `withdraw` or `batch_withdraw` to claim earned tokens. Emitted even for zero-amount calls that complete a stream.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamWithdrawn"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `recipient` | `Address` | Address that received the tokens |
| data[1] | `amount` | `i128` | Gross claimable amount before fee deduction (stroops) |
| data[2] | `timestamp` | `u64` | Ledger timestamp of the withdrawal |

> `amount` is the gross claimable. Net received = `amount − fee`. See `FeeCollected` for the fee breakdown.

---

### 3. `StreamCancelled`

**Trigger:** Sender calls `cancel_stream` or `batch_cancel_stream`. Earned tokens are sent to the recipient; the remainder is refunded to the sender.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamCancelled"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator who cancelled |
| data[1] | `refund_amount` | `i128` | Unstreamed tokens returned to sender (stroops) |
| data[2] | `recipient_amount` | `i128` | Earned tokens sent to recipient (stroops) |

> `refund_amount + recipient_amount == stream.deposit` (conservation invariant).

---

### 4. `StreamToppedUp`

**Trigger:** Sender calls `top_up` to add more tokens, extending the stream end time.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamToppedUp"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `added_amount` | `i128` | Actual tokens added (rounded down to nearest `flow_rate` multiple, stroops) |
| data[1] | `new_end_time` | `u64` | Updated stream end timestamp |

---

### 5. `StreamCompleted`

**Trigger:** A stream reaches its `end_time` naturally. Emitted during the final `withdraw` call that drains the stream, before storage cleanup.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamCompleted"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | _(none)_ | `()` | No data payload |

---

### 6. `StreamPaused`

**Trigger:** Sender calls `pause_stream`. Token accrual stops while paused.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamPaused"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `sender` | `Address` | Address that paused the stream |

---

### 7. `StreamResumed`

**Trigger:** Sender calls `resume_stream`. Token accrual resumes from the current timestamp.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamResumed"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `sender` | `Address` | Address that resumed the stream |

---

### 8. `StreamPartialCancelled`

**Trigger:** Sender calls `partial_cancel_stream` to reduce the stream amount. The original stream is cancelled and a new stream is created with the remaining deposit.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamPartialCancelled"` |
| topics[1] | `old_stream_id` | `u64` | Identifier of the cancelled stream |
| data[0] | `new_stream_id` | `u64` | Identifier of the replacement stream |
| data[1] | `sender` | `Address` | Stream creator |
| data[2] | `refund_amount` | `i128` | Tokens immediately returned to sender (stroops) |
| data[3] | `new_deposit` | `i128` | Deposit locked in the replacement stream (stroops) |

---

### 9. `StreamTerminatedByRecipient`

**Trigger:** Recipient calls `recipient_terminate` on a stream where `allow_recipient_termination = true`.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamTerminatedByRecipient"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `recipient` | `Address` | Recipient who terminated |
| data[1] | `recipient_amount` | `i128` | Earned tokens sent to recipient (stroops) |
| data[2] | `refund_amount` | `i128` | Remainder returned to sender (stroops) |

---

### 10. `RecipientTransferred`

**Trigger:** Sender calls `transfer_recipient`. Any earned tokens are auto-swept to the old recipient before ownership changes.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"RecipientTransferred"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `old_recipient` | `Address` | Previous recipient |
| data[1] | `new_recipient` | `Address` | New recipient |

---

### 11. `StreamArchived`

**Trigger:** `archive_stream` is called after the stream is fully settled. Storage entry is deleted on-chain.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamArchived"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator |
| data[1] | `recipient` | `Address` | Stream beneficiary |
| data[2] | `total_amount` | `i128` | Original deposit (stroops) |

---

### 12. `MetadataUpdated`

**Trigger:** `update_metadata` is called with a new temporary binary metadata blob (max 256 bytes).

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"MetadataUpdated"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `metadata` | `Bytes` | New metadata blob (raw bytes, max 256 bytes) |

---

### 13. `MetadataUriUpdated`

**Trigger:** `update_metadata_uri` is called with a new URI string. Empty string indicates URI cleared.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"MetadataUriUpdated"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `metadata_uri` | `String` | New URI (IPFS or HTTPS, max 128 bytes; empty string if cleared) |

---

### 14. `AutoRenewCancelled`

**Trigger:** `cancel_auto_renew` is called to disable auto-renewal on a stream.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"AutoRenewCancelled"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | _(none)_ | `()` | No data payload |

---

### 15. `AutoRenewFailed`

**Trigger:** Auto-renewal attempted but sender has insufficient token balance to fund the next cycle.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"AutoRenewFailed"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator with insufficient balance |
| data[1] | `required` | `i128` | Full deposit needed for renewal (stroops) |

---

### 16. `StreamRenewed`

**Trigger:** Auto-renewal succeeds. A new stream is created for the next cycle.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamRenewed"` |
| topics[1] | `old_stream_id` | `u64` | Identifier of the completed stream |
| data | `new_stream_id` | `u64` | Identifier of the newly created renewal stream |

---

### 17. `StreamExpired`

**Trigger:** `mark_expired` transitions a stream past its `end_time` into the `Expired` state.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamExpired"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | _(none)_ | `()` | No data payload |

---

### 18. `StreamSwept`

**Trigger:** `sweep_expired` removes an expired stream from storage.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"StreamSwept"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `caller` | `Address` | Address that triggered the sweep |

---

### 19. `TtlBumped`

**Trigger:** `bump_ttl` extends the on-chain ledger lifetime of a stream's storage entry.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TtlBumped"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `new_expiry_ledger` | `u32` | Ledger number at which the entry will next expire |

---
## Milestone, Holdback & Step-Vesting Events

### 20. `MilestoneReleased`

**Trigger:** Sender calls `release_milestone` to unlock a gated milestone tranche.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"MilestoneReleased"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `milestone_index` | `u32` | Zero-based index of the released milestone |

---

### 21. `HoldbackReleased`

**Trigger:** Sender calls `release_holdback` to send the escrowed holdback amount to the recipient.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"HoldbackReleased"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `amount` | `i128` | Holdback amount released (stroops) |
| data[1] | `recipient` | `Address` | Recipient of the holdback |

---

### 22. `HoldbackClawedBack`

**Trigger:** Sender calls `claw_back_holdback` before the recipient claims it.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"HoldbackClawedBack"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `amount` | `i128` | Holdback amount returned to sender (stroops) |
| data[1] | `sender` | `Address` | Sender who clawed back |

---

### 23. `TrancheStreamCreated`

**Trigger:** A step-vesting stream is created (`is_step_vesting = true`). Emitted alongside `StreamCreated`.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TrancheStreamCreated"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator |
| data[1] | `tranche_count` | `u32` | Number of tranches in the schedule |
| data[2] | `total_amount` | `i128` | Total deposit (stroops) |

---

### 24. `TranchesWithdrawn`

**Trigger:** One or more tranches are claimed during a `withdraw` call on a step-vesting stream.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TranchesWithdrawn"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `recipient` | `Address` | Recipient of the tranches |
| data[1] | `tranches_claimed` | `u32` | Number of tranches claimed in this call |
| data[2] | `amount` | `i128` | Total tokens released in this call (stroops) |

---

### 25. `TrancheStreamCancelled`

**Trigger:** A step-vesting stream is cancelled. Unclaimed tranches are refunded to the sender.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TrancheStreamCancelled"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator |
| data[1] | `unclaimed_tranche_refund` | `i128` | Unlocked tranche amounts returned to sender (stroops) |
| data[2] | `recipient_amount` | `i128` | Previously unlocked tranche amounts sent to recipient (stroops) |

---

### 26. `DelegateSet`

**Trigger:** Sender calls `set_delegate` to authorize an address to act on their behalf.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"DelegateSet"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `sender` | `Address` | Stream creator |
| data[1] | `delegate` | `Address` | Newly authorized delegate |

---

### 27. `DelegateRevoked`

**Trigger:** Sender calls `revoke_delegate` to remove a previously set delegate.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"DelegateRevoked"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data | `sender` | `Address` | Stream creator who revoked |

---

## Fee Events

### 28. `FeeCollected`

**Trigger:** A non-zero protocol fee is deducted during `withdraw` or `transfer_recipient`. Only emitted when the recipient is not fee-exempt.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FeeCollected"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `amount` | `i128` | Fee in stroops: `claimable × fee_bps / 10_000` |
| data[1] | `treasury` | `Address` | Destination of the fee |

---

### 29. `CreationFeeCollected`

**Trigger:** A flat XLM creation fee is charged when `create_stream` is called and `cf_xlm > 0`.

> This event has **one topic** (no `stream_id`).

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"CreationFeeCollected"` |
| data[0] | `fee_amount` | `i128` | Fee in XLM stroops (1 XLM = 10,000,000 stroops) |
| data[1] | `treasury` | `Address` | Destination of the fee |

---

### 30. `FeeChangeProposed`

**Trigger:** Admin calls `propose_fee_change`, starting the 7-day timelock before the new rate takes effect.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FeeChangeProposed"` |
| data[0] | `new_fee` | `u32` | Proposed fee in basis points (100 bps = 1%) |
| data[1] | `unlock_time` | `u64` | Unix timestamp after which the change can be executed (`now + 604_800`) |

---

### 31. `FeeChangeExecuted`

**Trigger:** Admin calls `execute_fee_change` after the timelock expires.

> This event has **one topic**. `data` is a single-element tuple (`ScvVec` with one element), not a scalar.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FeeChangeExecuted"` |
| data[0] | `new_fee` | `u32` | New active fee in basis points |

---

### 32. `FeeSwept`

**Trigger:** Admin calls `sweep_fees` to transfer accumulated protocol fees to a destination address.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FeeSwept"` |
| data[0] | `token` | `Address` | Token contract swept |
| data[1] | `amount` | `i128` | Amount swept (stroops) |
| data[2] | `destination` | `Address` | Destination address |

---

## Oracle Events

### 33. `PriceCheckPassed`

**Trigger:** An oracle price check succeeds within the allowed deviation during `create_stream` or `withdraw`.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"PriceCheckPassed"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `token` | `Address` | Token whose price was checked |
| data[1] | `price` | `i128` | Current oracle price |
| data[2] | `deviation_bps` | `u32` | Actual deviation from creation price in basis points |

---

### 34. `SlippageExceeded`

**Trigger:** Oracle price deviates beyond `max_price_deviation_bps` causing the operation to revert.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"SlippageExceeded"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `current_price` | `i128` | Current oracle price at time of check |
| data[1] | `max_slippage_bps` | `u32` | Configured maximum deviation in basis points |

---

### 35. `SlippageWarning`

**Trigger:** Oracle price deviation reaches 80% of the configured maximum (warning threshold).

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"SlippageWarning"` |
| topics[1] | `stream_id` | `u64` | Stream identifier |
| data[0] | `current_deviation_bps` | `u32` | Current deviation in basis points |
| data[1] | `max_slippage_bps` | `u32` | Configured maximum in basis points |

---

## Rate Limiting & Token Whitelist Events

### 36. `RateLimitExceeded`

**Trigger:** `create_stream` is rejected because the sender has exceeded the configured stream-creation rate limit.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"RateLimitExceeded"` |
| data | `sender` | `Address` | Address that hit the rate limit |

---

### 37. `RateLimitUpdated`

**Trigger:** Admin updates the rate-limit parameters via `set_rate_limit`.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"RateLimitUpdated"` |
| data[0] | `window_seconds` | `u64` | Rolling window size in seconds |
| data[1] | `max_creations` | `u32` | Max stream creations allowed per window |

---

### 38. `TokenWhitelisted`

**Trigger:** Admin calls `whitelist_token` to allow a token to be used in streams.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TokenWhitelisted"` |
| data | `token` | `Address` | Token contract address added |

---

### 39. `TokenDewhitelisted`

**Trigger:** Admin calls `remove_token` to disallow a previously approved token.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TokenDewhitelisted"` |
| data | `token` | `Address` | Token contract address removed |

---

### 40. `TokenWhitelistToggled`

**Trigger:** Admin enables token whitelist enforcement globally. Enforcement cannot be disabled because token calls are an external trust boundary.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"TokenWhitelistToggled"` |
| data | `enabled` | `bool` | `true` = enforcement on, `false` = enforcement off |

---

## Federation Events

### 41. `FederationRegistered`

**Trigger:** `register_federation` maps a Stellar federation name to an on-chain address.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FederationRegistered"` |
| data[0] | `federation_name` | `String` | Federation name (e.g. `user*sorostream.io`) |
| data[1] | `stellar_address` | `Address` | Resolved on-chain address |

---

### 42. `FederationUnregistered`

**Trigger:** `unregister_federation` removes a federation name mapping.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"FederationUnregistered"` |
| data | `federation_name` | `String` | Federation name that was removed |

---

## Contract Admin Events

### 43. `ContractDeployed`

**Trigger:** Emitted once during `initialize`. Records the contract version and initial admin.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"ContractDeployed"` |
| data[0] | `version` | `String` | Contract version string (e.g. `"1.0.0"`) |
| data[1] | `admin` | `Address` | Initial admin address |

---

### 44. `ContractPaused`

**Trigger:** Admin calls `emergency_pause`. All state-mutating instructions are blocked until resumed.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"ContractPaused"` |
| topics[1] | `admin` | `Address` | Admin who triggered the pause (indexed) |
| data | `timestamp` | `u64` | Ledger timestamp of the pause |

---

### 45. `ContractResumed`

**Trigger:** Admin calls `emergency_resume`. Normal operations are restored.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"ContractResumed"` |
| topics[1] | `admin` | `Address` | Admin who triggered the resume (indexed) |
| data | `timestamp` | `u64` | Ledger timestamp of the resume |

---

### 46. `ContractMigrated`

**Trigger:** Admin calls `migrate` to upgrade contract state to a new schema version.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"ContractMigrated"` |
| data[0] | `from_version` | `String` | Previous contract version |
| data[1] | `to_version` | `String` | New contract version |
| data[2] | `admin` | `Address` | Admin who performed the migration |

---

### 47. `AdminAction`

**Trigger:** Emitted alongside `emergency_pause`, `emergency_resume`, and `migrate` as a structured audit log entry.

> This event has **one topic**.

| Position | Field | Type | Description |
|----------|-------|------|-------------|
| topics[0] | event name | `Symbol` | `"AdminAction"` |
| data[0] | `instruction` | `String` | Name of the instruction (e.g. `"emergency_pause"`) |
| data[1] | `admin` | `Address` | Admin address |
| data[2] | `timestamp` | `u64` | Ledger timestamp |

---

## Filtering Events

All SoroStream events carry `type = "contract"` and the `contract_id` of the stream contract. Filter by event name by matching `topics[0]` after decoding from base64 XDR.

### stellar-cli

```bash
stellar events \
  --network testnet \
  --contract-id <CONTRACT_ID> \
  --start-ledger 0
```

### Horizon REST

```bash
curl "https://horizon-testnet.stellar.org/events?contract_id=<CONTRACT_ID>&limit=200&order=desc"
```

> Closes [#317](https://github.com/SoroStream/sorostream-contracts/issues/317).
