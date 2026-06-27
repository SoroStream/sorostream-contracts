# SoroStream Contract Specification

This document is the authoritative reference for the `SoroStreamContract` Soroban smart contract. It describes every public instruction, the storage data model, all error variants, emitted events, and the `StreamStatus` state machine. It is intended for contributors, integrators, and auditors.

---

## Table of Contents

1. [Data Model](#data-model)
2. [StreamStatus State Machine](#streamstatus-state-machine)
3. [Error Reference](#error-reference)
4. [Event Reference](#event-reference)
5. [Instruction Reference](#instruction-reference)
6. [Invariants](#invariants)

---

## Data Model

### `Stream`

| Field | Type | Description |
|---|---|---|
| `id` | `u64` | Unique stream identifier, assigned at creation. |
| `sender` | `Address` | Stream creator and payer. Must authorise all sender-gated instructions. |
| `recipient` | `Address` | Stream beneficiary. Must authorise all recipient-gated instructions. |
| `token` | `Address` | SAC-compatible token contract address (e.g. USDC). |
| `deposit` | `i128` | Total token amount locked in the contract (in stroops). |
| `flow_rate` | `i128` | Tokens released per second (`deposit / duration_seconds`, stroops/s). |
| `start_time` | `u64` | Ledger timestamp when the stream was created. |
| `cliff_time` | `u64` | Ledger timestamp before which no tokens are claimable. `cliff_time >= start_time`. |
| `end_time` | `u64` | Ledger timestamp when the stream ends. `end_time > start_time`. |
| `last_withdraw_time` | `u64` | Ledger timestamp of the most recent withdrawal. Initialised to `start_time`. |
| `status` | `StreamStatus` | Current lifecycle status (see state machine below). |
| `auto_renew` | `bool` | If `true`, the stream re-locks `deposit` tokens from the sender on natural completion. |

### `Stats`

Aggregate statistics returned by `get_stats`.

| Field | Type | Description |
|---|---|---|
| `total_streams` | `u64` | Total streams ever created (including Cancelled and Completed). |
| `active_streams` | `u64` | Streams currently in `Active` status. |
| `total_volume` | `i128` | Sum of all `deposit` values across all known streams (in stroops). |

---

## StreamStatus State Machine

```
                      ┌──────────────────────────┐
                      │         Active           │
                      │  (tokens are flowing)    │
                      └──────────────────────────┘
                         │                    │
           cancel_stream │                    │ now >= end_time
        partial_cancel   │                    │
                         ▼                    ▼
                  ┌───────────┐      ┌─────────────────┐
                  │ Cancelled │      │   end_time hit  │
                  │           │      └────────┬────────┘
                  └───────────┘               │
                                    ┌─────────┴──────────┐
                              auto_renew?              auto_renew?
                               = true                   = false
                                    │                       │
                         sender has enough              ┌───────────┐
                          balance?                      │ Completed │
                         ┌────┴─────┐                  └───────────┘
                       yes          no
                         │          │
                         ▼          ▼
                     ┌──────┐  ┌──────────┐
                     │      │  │Completed │
                     │Active│  │(auto-    │
                     │(reset│  │renew     │
                     │ to   │  │failed)   │
                     │start)│  └──────────┘
                     └──────┘
```

### Transitions

| From | To | Trigger | Instruction |
|---|---|---|---|
| `Active` | `Cancelled` | Sender calls cancel before `end_time` | `cancel_stream`, `partial_cancel_stream` |
| `Active` | `Completed` | `now >= end_time` and `auto_renew = false` | `withdraw` (on natural completion) |
| `Active` | `Active` (reset) | `now >= end_time`, `auto_renew = true`, sender has sufficient balance | `withdraw` (auto-renew path) |
| `Active` | `Completed` | `now >= end_time`, `auto_renew = true`, sender has **insufficient** balance | `withdraw` (auto-renew failure path) |

Terminal states: `Cancelled` and `Completed`. No instruction transitions out of either.

---

## Error Reference

| Code | Variant | Meaning |
|---|---|---|
| 1 | `StreamNotFound` | No stream exists with the given ID. |
| 2 | `NotRecipient` | Caller is not the stream's recipient. |
| 3 | `NotSender` | Caller is not the stream's sender. |
| 4 | `StreamNotActive` | Instruction requires `Active` status; stream is `Cancelled` or `Completed`. |
| 5 | `ZeroAmount` | `amount` must be greater than zero. |
| 6 | `InvalidDuration` | Duration or fee value is out of range. |
| 7 | `InsufficientBalance` | Contract has insufficient token balance to fulfil the transfer. |
| 8 | `InvalidCliff` | `cliff_seconds > duration_seconds`. |
| 9 | `AlreadyInitialized` | `initialize` was called after the contract was already set up. |
| 10 | `NotInitialized` | An instruction requiring admin access was called before `initialize`. |
| 11 | `DuplicateStream` | A stream with this `(sender, nonce)` pair already exists. |
| 12 | `InvalidStartTime` | Reserved; `start_time` is in the past. |
| 13 | `InvalidPartialCancel` | `cancel_amount >= remaining` or leaves less than one second of flow. |
| 14 | `ContractPaused` | `create_stream` rejected because the contract is paused. |
| 15 | `ZeroFlowRate` | `amount / duration_seconds == 0`; stream would never flow. |

---

## Event Reference

All events are published as `(topic, data)` pairs on the Soroban event log.

| Symbol | Topic | Data | Emitted by |
|---|---|---|---|
| `StreamCreated` | `(StreamCreated, stream_id)` | `(sender, recipient, amount, flow_rate, end_time)` | `create_stream`, `batch_create_stream` |
| `StreamWithdrawn` | `(StreamWithdrawn, stream_id)` | `(recipient, amount, timestamp)` | `withdraw`, `batch_withdraw` |
| `StreamCancelled` | `(StreamCancelled, stream_id)` | `(sender, refund_amount, recipient_amount)` | `cancel_stream`, `partial_cancel_stream` |
| `StreamPartialCancelled` | `(StreamPartialCancelled, old_stream_id)` | `(new_stream_id, sender, refund_amount, new_deposit)` | `partial_cancel_stream` |
| `StreamToppedUp` | `(StreamToppedUp, stream_id)` | `(added_amount, new_end_time)` | `top_up` |
| `StreamCompleted` | `(StreamCompleted, stream_id)` | `()` | `withdraw`, `batch_withdraw` |
| `AutoRenewFailed` | `(AutoRenewFailed, stream_id)` | `(sender, required_amount)` | `withdraw` |

---

## Instruction Reference

### Admin Instructions

#### `initialize(env, admin: Address) → Result<(), StreamError>`

Sets the contract admin. Reverts with `AlreadyInitialized` if called more than once.

#### `get_admin(env) → Result<Address, StreamError>`

Returns the current admin address. Reverts with `NotInitialized` if not set.

#### `set_admin(env, new_admin: Address) → Result<(), StreamError>`

Transfers the admin role. Requires auth from the current admin.

#### `pause(env) → Result<(), StreamError>`

Pauses the contract. Only `create_stream` is blocked while paused; existing streams continue to accrue and can be withdrawn or cancelled. Requires admin auth.

#### `unpause(env) → Result<(), StreamError>`

Resumes normal operation. Requires admin auth.

#### `is_paused(env) → bool`

Returns the current pause state.

#### `upgrade(env, new_wasm_hash: BytesN<32>) → Result<(), StreamError>`

Replaces the contract WASM. All existing storage is preserved. Requires admin auth.

#### `set_protocol_fee(env, fee_bps: u32) → Result<(), StreamError>`

Sets the protocol fee in basis points (max 10 000 = 100%). Fee is deducted from withdrawn amounts in `batch_withdraw`. Reverts with `InvalidDuration` if `fee_bps > 10_000`.

#### `set_treasury_address(env, treasury: Address) → Result<(), StreamError>`

Sets the address that receives protocol fee transfers.

#### `get_protocol_fee_info(env) → (u32, Option<Address>)`

Returns `(fee_bps, Option<treasury_address>)`.

---

### Stream Lifecycle Instructions

#### `create_stream(env, sender, recipient, token, amount, duration_seconds, cliff_seconds, nonce, auto_renew) → Result<u64, StreamError>`

Creates a single payment stream.

**Validations (in order):**
1. Contract is not paused → `ContractPaused`
2. `(sender, nonce)` not already used → `DuplicateStream`
3. `amount > 0` → `ZeroAmount`
4. `cliff_seconds <= duration_seconds` → `InvalidCliff`
5. `amount / duration_seconds > 0` → `ZeroFlowRate`

**Side effects:**
- Transfers `amount` tokens from `sender` to the contract.
- Emits `StreamCreated`.
- Returns the new stream ID.

**Flow rate computation:** `flow_rate = amount / duration_seconds` (integer division). The remainder `amount % duration_seconds` is kept in `deposit` but will never be paid out via normal flow — it is returned to the sender on cancellation because `cancel_stream` refunds `deposit - flow_rate * elapsed_since_start`.

#### `withdraw(env, stream_id, recipient) → Result<(), StreamError>`

Transfers all currently claimable tokens to the recipient.

**Claimable calculation:**
```
effective_now = min(now, end_time)
elapsed       = if now < cliff_time { 0 } else { effective_now - last_withdraw_time }
claimable     = flow_rate * elapsed
```

**Post-withdrawal behaviour:**
- If `now < end_time`: saves updated `last_withdraw_time`, stream remains `Active`.
- If `now >= end_time` and `auto_renew = false`: emits `StreamCompleted`, removes stream from storage.
- If `now >= end_time` and `auto_renew = true` and sender has ≥ `deposit` balance: re-locks `deposit`, resets `start_time`/`end_time`/`last_withdraw_time`, stream stays `Active`.
- If `now >= end_time` and `auto_renew = true` and sender has < `deposit` balance: emits `AutoRenewFailed`, sets status to `Completed`, emits `StreamCompleted`.

#### `cancel_stream(env, stream_id, sender) → Result<(), StreamError>`

Cancels an active stream before its natural end.

**Amounts transferred:**
```
recipient_amount = flow_rate * (min(now, end_time) - last_withdraw_time)
refund_amount    = deposit - flow_rate * (min(now, end_time) - start_time)
```

Stream status is set to `Cancelled`. Emits `StreamCancelled`.

#### `partial_cancel_stream(env, stream_id, sender, cancel_amount) → Result<u64, StreamError>`

Reclaims part of the unstreamed deposit while preserving a smaller continuing stream.

**Validation:** `cancel_amount < remaining` and `remaining - cancel_amount >= flow_rate`.

**Steps:**
1. Transfers earned tokens to recipient.
2. Transfers `cancel_amount` to sender.
3. Sets original stream to `Cancelled`.
4. Creates a new stream with `deposit = remaining - cancel_amount`, same `flow_rate`, no cliff.
5. Emits `StreamCancelled` (for original) and `StreamPartialCancelled`.

Returns the new stream ID.

#### `top_up(env, stream_id, sender, token, amount) → Result<(), StreamError>`

Extends stream duration by adding more tokens.

**Effective amount:** only the portion divisible by `flow_rate` is accepted; dust stays with the sender:
```
effective_amount = amount - (amount % flow_rate)
extra_seconds    = effective_amount / flow_rate
```

`end_time` and `deposit` are increased by `extra_seconds` and `effective_amount` respectively. Emits `StreamToppedUp`.

---

### Query Instructions

#### `get_stream(env, stream_id) → Result<Stream, StreamError>`

Returns the full `Stream` struct.

#### `get_claimable(env, stream_id) → Result<i128, StreamError>`

Returns tokens currently claimable by the recipient. Returns `0` for non-Active streams or before cliff.

#### `is_participant(env, stream_id, address) → Result<bool, StreamError>`

Returns `true` if `address` is either the sender or recipient.

#### `get_all_stream_ids(env, start, limit) → Vec<u64>`

Paginated list of all stream IDs (max 20 per call).

#### `get_streams_by_sender(env, sender, start, limit) → Vec<Stream>`

Paginated streams created by `sender` (max 20).

#### `get_streams_by_recipient(env, recipient, start, limit) → Vec<Stream>`

Paginated streams targeting `recipient` (max 20).

#### `get_active_streams_by_sender(env, sender) → Vec<Stream>`

All `Active` streams created by `sender` (unbounded — avoid on large sender histories).

#### `get_active_streams_by_recipient(env, recipient) → Vec<Stream>`

All `Active` streams targeting `recipient` (unbounded).

#### `get_stats(env) → Stats`

Scans all known streams and returns aggregate statistics. O(n) in total stream count — use sparingly on-chain.

---

### Batch Instructions

#### `batch_create_stream(env, sender, recipients, amounts, token, duration_seconds, auto_renew) → Result<Vec<u64>, StreamError>`

Creates multiple streams in one transaction. `recipients.len()` must equal `amounts.len()`. Transfers the sum of all amounts in a single token transfer before creating each stream.

#### `batch_withdraw(env, stream_ids, recipient) → Result<Vec<i128>, StreamError>`

Withdraws from multiple streams in one transaction. All streams must have the same `recipient`. Protocol fee is deducted from each withdrawal if configured.

---

## Invariants

The following properties must hold at all times after `initialize`:

1. **Non-negative balances.** `deposit >= 0` for all stored streams.
2. **Time ordering.** For every stream: `start_time <= cliff_time <= end_time` and `start_time <= last_withdraw_time <= end_time`.
3. **Flow rate consistency.** `flow_rate = deposit / (end_time - start_time)` (approximately; dust may cause slight under-accounting).
4. **Active-only withdrawals.** `withdraw`, `cancel_stream`, and `partial_cancel_stream` all revert with `StreamNotActive` if the stream status is not `Active`.
5. **Auth gating.** `cancel_stream` and `top_up` require auth from `stream.sender`. `withdraw` requires auth from `stream.recipient`. Admin instructions require auth from the stored admin address.
6. **Idempotency via nonce.** Each `(sender, nonce)` pair may create at most one stream. `DuplicateStream` is returned on re-use.
7. **Monotonic stream IDs.** Stream IDs are assigned from a monotonically increasing counter; they are never reused, even after cancellation or completion.
8. **Cliff before claimable.** Before `ledger.timestamp() >= cliff_time`, `get_claimable` returns 0 and `withdraw` transfers 0 tokens.
9. **Pause scope.** Only `create_stream` is blocked by the paused flag. Existing-stream instructions (`withdraw`, `cancel_stream`, etc.) proceed regardless.
10. **Protocol fee bounds.** `fee_bps` is enforced to be `<= 10 000` (100%).
