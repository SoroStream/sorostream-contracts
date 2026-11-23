# SoroStream Contract Error Code Reference

This document is the authoritative reference for all `StreamError` variants returned by the SoroStream stream contract. Every entry includes the numeric code, a plain-English description, the conditions that trigger the error, and the recommended response for SDK and frontend callers.

---

## How Errors Are Returned

Soroban contracts return errors as `SCError` values. The SDK surfaces them as named variants of the `StreamError` enum. Each variant maps to a `u32` discriminant — this is the value you will see in raw XDR responses and on-chain transaction results.

**Reading the error in TypeScript (stellar-sdk):**

```typescript
import { Contract, rpc } from "@stellar/stellar-sdk";

try {
  await contract.call("withdraw", streamId, recipient);
} catch (err) {
  if (err instanceof rpc.AssembledTransaction.Errors.SimulationFailed) {
    // err.message will contain the StreamError variant name, e.g. "StreamNotFound"
    console.error("Contract error:", err.message);
  }
}
```

**Reading the error in Rust tests:**

```rust
let result = client.try_withdraw(&stream_id, &recipient);
assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
```

---

## Maintenance Rule

This file must be updated whenever a new `StreamError` variant is added to `contracts/stream/src/errors.rs`. The PR checklist in `CONTRIBUTING.md` includes this as a required step.

---

## Error Reference

### Code 1 — `StreamNotFound`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamNotFound` |
| Code | `1` |

**Description:** The requested `stream_id` does not exist in contract storage.

**Trigger:** Passing a `stream_id` that was never created, has already been removed (completed non-auto-renewing streams and cancelled streams are deleted from storage), or belongs to a different contract deployment.

**Remediation:** Verify the `stream_id` is correct and was obtained from a `StreamCreated` event or the return value of `create_stream`. If the stream was completed or cancelled, query your off-chain index rather than the contract.

---

### Code 2 — `NotRecipient`

| Field | Value |
|-------|-------|
| Variant | `StreamError::NotRecipient` |
| Code | `2` |

**Description:** The caller is not the designated recipient of the stream.

**Trigger:** Calling `withdraw` or `recipient_terminate` with an address that does not match `stream.recipient`.

**Remediation:** Ensure the transaction is signed by the recipient address stored in the stream. If the recipient was changed via `transfer_recipient`, use the new address.

---

### Code 3 — `NotSender`

| Field | Value |
|-------|-------|
| Variant | `StreamError::NotSender` |
| Code | `3` |

**Description:** The caller is not the sender (creator) of the stream.

**Trigger:** Calling `cancel_stream`, `top_up`, `pause_stream`, `resume_stream`, or `partial_cancel_stream` with an address that does not match `stream.sender`.

**Remediation:** Sign the transaction with the sender address that created the stream. Delegates authorized via `set_delegate` may also perform these operations.

---

### Code 4 — `StreamNotActive`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamNotActive` |
| Code | `4` |

**Description:** The operation requires an active stream but the stream is in a non-active state (paused, cancelled, completed, or expired).

**Trigger:** Calling `withdraw` on a paused or cancelled stream; calling `top_up` on a completed stream.

**Remediation:** Check `stream.status` before calling. Resume a paused stream with `resume_stream` before withdrawing.

---

### Code 5 — `ZeroAmount`

| Field | Value |
|-------|-------|
| Variant | `StreamError::ZeroAmount` |
| Code | `5` |

**Description:** A token amount of zero was provided where a positive amount is required.

**Trigger:** Passing `amount = 0` to `create_stream` or `top_up`.

**Remediation:** Provide a positive non-zero amount. Minimum practical amount is `flow_rate × 1` (enough for at least one second of streaming).

---

### Code 6 — `InvalidDuration`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidDuration` |
| Code | `6` |

**Description:** The stream duration is zero or otherwise invalid.

**Trigger:** Passing `duration_seconds = 0` to `create_stream`.

**Remediation:** Provide a positive duration. Note that `StreamDurationTooShort` (code 22) applies when the duration is positive but below the configured minimum.

---

### Code 7 — `InsufficientBalance`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InsufficientBalance` |
| Code | `7` |

**Description:** The sender does not have enough token balance to fund the stream or top-up.

**Trigger:** Calling `create_stream` or `top_up` when the sender's on-chain balance is less than `amount`.

**Remediation:** Fund the sender's account with the required token amount before calling. Account for network fees when estimating the required balance.

---

### Code 8 — `InvalidCliff`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidCliff` |
| Code | `8` |

**Description:** The cliff period is longer than the total stream duration.

**Trigger:** Passing `cliff_seconds > duration_seconds` to `create_stream`.

**Remediation:** Ensure `cliff_seconds <= duration_seconds`. A cliff of zero is valid.

---

### Code 9 — `AlreadyInitialized`

| Field | Value |
|-------|-------|
| Variant | `StreamError::AlreadyInitialized` |
| Code | `9` |

**Description:** `initialize` was called on a contract that has already been initialized.

**Trigger:** Calling `initialize` more than once on the same deployed contract.

**Remediation:** `initialize` must be called exactly once after deployment. This error is not recoverable on the same deployment — deploy a fresh contract if re-initialization is required.

---

### Code 10 — `NotInitialized`

| Field | Value |
|-------|-------|
| Variant | `StreamError::NotInitialized` |
| Code | `10` |

**Description:** A contract function was called before `initialize` was invoked.

**Trigger:** Calling any instruction on a freshly deployed contract before calling `initialize`.

**Remediation:** Call `initialize` with the admin address and initial configuration immediately after deployment.

---

### Code 11 — `DuplicateStream`

| Field | Value |
|-------|-------|
| Variant | `StreamError::DuplicateStream` |
| Code | `11` |

**Description:** A stream creation was rejected because a stream with the same nonce already exists for this sender.

**Trigger:** Submitting two `create_stream` calls with the same `nonce` value from the same sender address.

**Remediation:** Use a unique nonce for each stream. A monotonically increasing counter or a hash of the creation parameters is a common pattern.

---

### Code 12 — `InvalidStartTime`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidStartTime` |
| Code | `12` |

**Description:** The provided start time is in the past or otherwise invalid.

**Trigger:** Passing a `start_time` that is earlier than the current ledger timestamp.

**Remediation:** Use the current ledger timestamp or a future timestamp. Query the ledger timestamp via `env.ledger().timestamp()` in tests or a recent Horizon response in production.

---

### Code 13 — `InvalidPartialCancel`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidPartialCancel` |
| Code | `13` |

**Description:** The partial cancellation parameters are invalid (e.g. the requested refund exceeds the unearned balance).

**Trigger:** Calling `partial_cancel_stream` with a `refund_amount` larger than the remaining unstreamed deposit.

**Remediation:** Query `get_claimable` to determine how much has already been earned, then compute the maximum refundable amount as `stream.deposit − earned`.

---

### Code 14 — `ContractPaused`

| Field | Value |
|-------|-------|
| Variant | `StreamError::ContractPaused` |
| Code | `14` |

**Description:** The contract is in an emergency paused state. All state-mutating operations are blocked.

**Trigger:** Calling any instruction (except admin resume functions) while `emergency_pause` is active.

**Remediation:** Monitor the `ContractPaused` and `ContractResumed` events to track pause state. Retry the operation after the `ContractResumed` event is observed.

---

### Code 15 — `Overflow`

| Field | Value |
|-------|-------|
| Variant | `StreamError::Overflow` |
| Code | `15` |

**Description:** An arithmetic operation overflowed the allowed range for the type.

**Trigger:** Providing extreme values for `amount` or `duration_seconds` that cause intermediate calculations (e.g. `flow_rate × elapsed`) to overflow `i128`.

**Remediation:** Use amounts within the practical range. Maximum safe deposit is approximately `i128::MAX / duration_seconds`. Validate inputs client-side before submitting.

---

### Code 16 — `ZeroFlowRate`

| Field | Value |
|-------|-------|
| Variant | `StreamError::ZeroFlowRate` |
| Code | `16` |

**Description:** The computed flow rate is zero, meaning the stream would never pay out any tokens.

**Trigger:** Providing an `amount` smaller than `duration_seconds`, causing `amount / duration_seconds` to floor to zero.

**Remediation:** Ensure `amount >= duration_seconds`. For long-duration streams (e.g. multi-year vesting), use larger amounts or shorter duration units.

---

### Code 17 — `BatchLengthMismatch`

| Field | Value |
|-------|-------|
| Variant | `StreamError::BatchLengthMismatch` |
| Code | `17` |

**Description:** The `recipients` and `amounts` arrays passed to `batch_create_stream` have different lengths.

**Trigger:** Calling `batch_create_stream` with mismatched array lengths.

**Remediation:** Ensure every element in `recipients` has a corresponding element in `amounts` at the same index.

---

### Code 18 — `TokenMismatch`

| Field | Value |
|-------|-------|
| Variant | `StreamError::TokenMismatch` |
| Code | `18` |

**Description:** The token address provided does not match the token stored in the stream.

**Trigger:** Calling `top_up` with a different token address than the one used when the stream was created.

**Remediation:** Always use the same token address as stored in `stream.token`. Query it with `get_stream` before calling `top_up`.

---

### Code 19 — `StreamLocked`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamLocked` |
| Code | `19` |

**Description:** The stream's reentrancy guard is active, indicating a concurrent call is already being processed.

**Trigger:** A reentrant call to `withdraw` while a previous `withdraw` is still executing. This should not occur in normal operation.

**Remediation:** Do not call contract functions recursively or from within a callback triggered by the same contract. This error indicates a potentially malicious or misconfigured integration.

---

### Code 20 — `NotAuthorized`

| Field | Value |
|-------|-------|
| Variant | `StreamError::NotAuthorized` |
| Code | `20` |

**Description:** The caller does not have the required authorization to perform this operation.

**Trigger:** Calling an admin-only function (e.g. `emergency_pause`, `propose_fee_change`) without being the configured admin address.

**Remediation:** Only the admin address set during `initialize` (or updated via governance) can call admin functions. Check the current admin with the appropriate getter.

---

### Code 21 — `StreamNotPaused`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamNotPaused` |
| Code | `21` |

**Description:** `resume_stream` was called on a stream that is not currently paused.

**Trigger:** Calling `resume_stream` when `stream.status != Paused`.

**Remediation:** Check `stream.status` before calling `resume_stream`. This operation is a no-op if the stream is already active.

---

### Code 22 — `StreamDurationTooShort`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamDurationTooShort` |
| Code | `22` |

**Description:** The stream duration is less than the configured minimum duration.

**Trigger:** Calling `create_stream` with a `duration_seconds` below the value set by `set_min_duration`.

**Remediation:** Use a duration at or above the minimum. Query the current minimum via the contract configuration getter.

---

### Code 23 — `StreamIdConflict`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamIdConflict` |
| Code | `23` |

**Description:** The generated stream ID collides with an existing stream ID.

**Trigger:** An extremely rare SHA-256 truncation collision during stream ID generation. Should not occur in practice.

**Remediation:** Retry the operation with a different nonce value.

---

### Code 24 — `SenderStreamLimitExceeded`

| Field | Value |
|-------|-------|
| Variant | `StreamError::SenderStreamLimitExceeded` |
| Code | `24` |

**Description:** The sender has reached the maximum number of concurrent active streams allowed per address.

**Trigger:** Calling `create_stream` when the sender already has the maximum number of streams stored in their sender index.

**Remediation:** Cancel or complete some existing streams before creating new ones. Query active streams with `get_streams_by_sender`.

---

### Code 25 — `InvalidNonce`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidNonce` |
| Code | `25` |

**Description:** The nonce value provided for stream creation is invalid.

**Trigger:** Passing a nonce that fails validation (e.g. a nonce that has already been consumed or is out of the valid range).

**Remediation:** Use a fresh nonce for each stream creation. A simple counter or timestamp-derived value is recommended.

---

### Code 26 — `MigrationAlreadyApplied`

| Field | Value |
|-------|-------|
| Variant | `StreamError::MigrationAlreadyApplied` |
| Code | `26` |

**Description:** The migration script has already been applied to this contract deployment.

**Trigger:** Calling `migrate` with the same target version more than once.

**Remediation:** Each migration is idempotent — this error prevents double-application. No action is needed; the migration has already succeeded.

---

### Code 27 — `StreamNotSettled`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamNotSettled` |
| Code | `27` |

**Description:** An archive or cleanup operation was attempted on a stream that still has unsettled balances.

**Trigger:** Calling `archive_stream` before the stream is fully withdrawn or cancelled.

**Remediation:** Call `withdraw` to drain remaining claimable amounts, or `cancel_stream` to settle the stream before archiving.

---

### Code 28 — `WithdrawalCooldownActive`

| Field | Value |
|-------|-------|
| Variant | `StreamError::WithdrawalCooldownActive` |
| Code | `28` |

**Description:** A withdrawal was attempted before the minimum cooldown period between withdrawals has elapsed.

**Trigger:** Calling `withdraw` again too soon after a previous withdrawal when a per-stream cooldown is configured.

**Remediation:** Wait for the cooldown period to pass. The cooldown is stored in `stream.lock_until` — check this field before calling.

---

### Code 29 — `RecipientNotWhitelisted`

| Field | Value |
|-------|-------|
| Variant | `StreamError::RecipientNotWhitelisted` |
| Code | `29` |

**Description:** The recipient address is not on the recipient whitelist.

**Trigger:** Calling `create_stream` with a recipient that is not whitelisted when recipient whitelisting is enforced.

**Remediation:** Add the recipient to the whitelist via the admin function, or disable recipient whitelisting if it is not required.

---

### Code 30 — `MetadataTooLong`

| Field | Value |
|-------|-------|
| Variant | `StreamError::MetadataTooLong` |
| Code | `30` |

**Description:** The temporary metadata blob exceeds the 256-byte maximum.

**Trigger:** Passing a `metadata` field longer than 256 bytes to `update_metadata`.

**Remediation:** Truncate or compress the metadata to 256 bytes or fewer. For larger metadata, store the content off-chain (IPFS or HTTPS) and use `metadata_uri` to reference it.

---

### Code 31 — `InvalidEndTime`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidEndTime` |
| Code | `31` |

**Description:** The computed or provided end time is invalid (e.g. before the start time).

**Trigger:** Providing start/duration combinations that result in an end time in the past, or explicitly setting an `end_time` before `start_time`.

**Remediation:** Ensure `start_time + duration_seconds > current_ledger_timestamp`.

---

### Code 32 — `InsufficientXlmForFee`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InsufficientXlmForFee` |
| Code | `32` |

**Description:** The sender does not have enough XLM to pay the flat stream creation fee.

**Trigger:** Calling `create_stream` when `cf_xlm > 0` and the sender's XLM balance is insufficient to cover the fee.

**Remediation:** Fund the sender's account with additional XLM before calling `create_stream`. The required fee is available from the contract configuration.

---

### Code 33 — `DuplicateStreamId`

| Field | Value |
|-------|-------|
| Variant | `StreamError::DuplicateStreamId` |
| Code | `33` |

**Description:** A stream with the generated ID already exists in storage.

**Trigger:** ID generation produced a value that collides with an existing stream. Functionally similar to `StreamIdConflict` (code 23).

**Remediation:** Retry with a different nonce to produce a different stream ID.

---

### Code 34 — `ReentrancyDetected`

| Field | Value |
|-------|-------|
| Variant | `StreamError::ReentrancyDetected` |
| Code | `34` |

**Description:** A reentrancy attempt was detected and blocked.

**Trigger:** A cross-contract callback or exploit attempt that re-enters the stream contract while a state-mutating call is in progress.

**Remediation:** Legitimate callers should never trigger this. If encountered in a non-malicious context, review the call chain for unexpected cross-contract calls.

---

### Code 35 — `InvalidMetadataUri`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidMetadataUri` |
| Code | `35` |

**Description:** The metadata URI does not conform to the allowed format (must be HTTPS or IPFS, max 128 bytes).

**Trigger:** Passing a `metadata_uri` that is not a valid HTTPS URL or IPFS CID path, or exceeds 128 bytes.

**Remediation:** Use a valid HTTPS URL or IPFS URI. Keep it under 128 bytes. Example: `https://example.com/stream/1.json` or `ipfs://Qm...`.

---

### Code 36 — `StreamNotComplete`

| Field | Value |
|-------|-------|
| Variant | `StreamError::StreamNotComplete` |
| Code | `36` |

**Description:** An operation requires the stream to be in the `Completed` state, but it is not.

**Trigger:** Calling `archive_stream` or a post-completion cleanup function before the stream has naturally ended.

**Remediation:** Wait for the stream to reach its `end_time` and be marked as completed via a final `withdraw` call.

---

### Code 37 — `TokenNotWhitelisted` / `InvalidTranches` / `RateLimitExceeded`

| Field | Value |
|-------|-------|
| Variant | `StreamError::TokenNotWhitelisted`, `StreamError::InvalidTranches`, `StreamError::RateLimitExceeded` |
| Code | `37` |

> **Note:** Code `37` is shared by three variants in the current enum definition due to a known duplicate-discriminant issue. All three are returned as code `37` at the ABI level.

**`TokenNotWhitelisted`** — The token used in `create_stream` is not on the mandatory token whitelist.
- *Trigger:* Token whitelist is active and the provided token address is not approved.
- *Remediation:* Use an approved token. Query the whitelist or request the admin to add the token.

**`InvalidTranches`** — The tranche schedule in a step-vesting stream is invalid (zero amount, unsorted unlock times, total mismatch, or empty list).
- *Trigger:* Passing malformed `tranches` to `create_stream` when `is_step_vesting = true`.
- *Remediation:* Ensure tranches are sorted by `unlock_time` in ascending order, all amounts are positive, and the sum of tranche amounts equals the total deposit.

**`RateLimitExceeded`** — The sender has exceeded the stream-creation rate limit within the current rolling window.
- *Trigger:* Creating too many streams in a short time period when rate limiting is configured.
- *Remediation:* Wait for the rate-limit window to roll over before creating additional streams. Monitor the `RateLimitExceeded` event.

---

### Code 38 — `PriceDeviationTooHigh` / `TokenNotWhitelisted`

| Field | Value |
|-------|-------|
| Variant | `StreamError::PriceDeviationTooHigh`, `StreamError::TokenNotWhitelisted` |
| Code | `38` |

> **Note:** Code `38` is shared by two variants due to a duplicate-discriminant issue.

**`PriceDeviationTooHigh`** — The oracle price at the time of the call deviates from the creation price by more than `max_price_deviation_bps`.
- *Trigger:* Oracle price has moved beyond the allowed tolerance since the stream was created.
- *Remediation:* Retry the operation when the price stabilizes. Consider increasing `max_price_deviation_bps` when creating streams for volatile tokens.

**`TokenNotWhitelisted`** — See code 37 entry above.

---

### Code 39 — `OracleError` / `SlippageExceeded`

| Field | Value |
|-------|-------|
| Variant | `StreamError::OracleError`, `StreamError::SlippageExceeded` |
| Code | `39` |

> **Note:** Code `39` is shared by two variants due to a duplicate-discriminant issue.

**`OracleError`** — The oracle contract call failed or returned an unexpected value.
- *Trigger:* Oracle contract is unavailable, returns a non-positive price, or the cross-contract call panics.
- *Remediation:* Check the oracle contract status. If the oracle is down, streams with oracle validation will be blocked until it recovers.

**`SlippageExceeded`** — The current token price exceeds the slippage tolerance at time of operation.
- *Trigger:* Token price slippage check failed during `create_stream` or `withdraw` on an oracle-enabled stream.
- *Remediation:* Retry when price stabilizes or increase the `max_slippage_bps` parameter.

---

### Code 40 — `InvalidSlippage`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidSlippage` |
| Code | `40` |

**Description:** The `max_slippage_bps` value is out of the valid range.

**Trigger:** Passing a `max_slippage_bps` value of `0` (disabling slippage protection is not allowed) or above `10_000` (100%).

**Remediation:** Use a value between `1` and `10_000` basis points. Common values: `50` (0.5%), `100` (1%), `500` (5%).

---

### Code 41 — `DurationExceedsMax`

| Field | Value |
|-------|-------|
| Variant | `StreamError::DurationExceedsMax` |
| Code | `41` |

**Description:** The stream duration exceeds the configured maximum allowed duration.

**Trigger:** Calling `create_stream` with a `duration_seconds` above the maximum set by the admin.

**Remediation:** Use a shorter duration. If long-duration streams are needed, request the admin to increase the maximum, or use `auto_renew` with a shorter per-cycle duration.

---

### Code 42 — `InvalidTokenAddress`

| Field | Value |
|-------|-------|
| Variant | `StreamError::InvalidTokenAddress` |
| Code | `42` |

**Description:** The token address provided is not a valid SAC-compatible token contract.

**Trigger:** Passing a zero address, a non-contract address, or a contract that does not implement the SEP-41 token interface.

**Remediation:** Use a valid Stellar Asset Contract (SAC) address or a token that implements the SEP-41 interface. On testnet, USDC is available at the address listed in `deployments/testnet.json`.

---

## SDK ABI Name Export

The `StreamError` enum is exported in the contract's WASM ABI under the type name `StreamError`. SDKs generated via `stellar contract bindings` will expose typed error variants by name, allowing you to match errors without hardcoding numeric codes:

```typescript
// Generated SDK usage
import { StreamError } from "./contracts/stream";

try {
  await contract.withdraw({ stream_id, recipient });
} catch (e) {
  if (e instanceof StreamError.StreamNotFound) {
    // stream was already completed or cancelled
  } else if (e instanceof StreamError.NotRecipient) {
    // wrong signer
  }
}
```

```rust
// Rust integration test
assert_eq!(
    client.try_withdraw(&stream_id, &other_address),
    Err(Ok(StreamError::NotRecipient))
);
```

---

## Duplicate Code Notes

The current `errors.rs` contains several variants that share the same `u32` discriminant due to enum conflicts introduced across multiple feature branches. The table below tracks the known duplicates:

| Code | Variants sharing the code |
|------|--------------------------|
| 37 | `TokenNotWhitelisted`, `InvalidTranches`, `RateLimitExceeded` |
| 38 | `PriceDeviationTooHigh`, `TokenNotWhitelisted` |
| 39 | `OracleError`, `SlippageExceeded` |

These will be resolved in a future minor release by assigning unique codes to each variant. SDKs should match errors by variant name (via ABI bindings) rather than by raw numeric code to remain forward-compatible.

> Closes [#319](https://github.com/SoroStream/sorostream-contracts/issues/319).
