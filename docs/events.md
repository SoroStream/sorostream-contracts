# SoroStream Contract Event Schema

**Schema version:** `1.0.0`

Consumers (SDKs, indexers, third-party tools) should check the contract version via `get_version()` before parsing events. Breaking field changes will increment the schema version.

---

## Event Format

All events are emitted via `env.events().publish(topics, data)`.

- **Topics** — a tuple that always starts with a `Symbol` event name, optionally followed by indexed fields (e.g. `stream_id`, `admin`).
- **Data** — a tuple (or scalar) of non-indexed fields.

Indexed fields appear in topics and are efficiently filterable. Non-indexed fields appear in data.

---

## Events

### `StreamCreated`

Emitted when a new stream is created by `create_stream` or `batch_create_stream`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamCreated"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Unique stream identifier |
| `sender` | data[0] | `Address` | Stream creator / payer |
| `recipient` | data[1] | `Address` | Stream beneficiary |
| `amount` | data[2] | `i128` | Total deposit in stroops |
| `flow_rate` | data[3] | `i128` | Tokens released per second (stroops/s) |
| `end_time` | data[4] | `u64` | Ledger timestamp when stream ends |

**Example:**
```
topics: ("StreamCreated", 12345678)
data:   (GABC...sender, GXYZ...recipient, 1000000, 1000, 1751000000)
```

---

### `StreamWithdrawn`

Emitted when a recipient withdraws claimable tokens via `withdraw` or `batch_withdraw`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamWithdrawn"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `recipient` | data[0] | `Address` | Recipient who withdrew |
| `amount` | data[1] | `i128` | Amount withdrawn in stroops |
| `timestamp` | data[2] | `u64` | Ledger timestamp of withdrawal |

---

### `StreamCancelled`

Emitted when a stream is cancelled via `cancel_stream` or `batch_cancel_stream`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamCancelled"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `sender` | data[0] | `Address` | Stream creator who cancelled |
| `refund_amount` | data[1] | `i128` | Tokens returned to sender |
| `recipient_amount` | data[2] | `i128` | Tokens sent to recipient (earned portion) |

---

### `StreamToppedUp`

Emitted when a sender adds tokens to an existing stream via `top_up`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamToppedUp"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `added_amount` | data[0] | `i128` | Additional tokens deposited (stroops) |
| `new_end_time` | data[1] | `u64` | Updated end timestamp after extension |

---

### `StreamCompleted`

Emitted when a stream reaches its natural end time.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamCompleted"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |

Data: `()` (no data payload)

---

### `StreamPaused`

Emitted when a sender pauses an active stream via `pause_stream`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamPaused"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `sender` | data | `Address` | Sender who paused the stream |

---

### `StreamResumed`

Emitted when a sender resumes a paused stream via `resume_stream`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamResumed"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `sender` | data | `Address` | Sender who resumed the stream |

---

### `StreamPartialCancelled`

Emitted when a sender partially cancels a stream via `partial_cancel_stream`, spawning a new smaller stream.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamPartialCancelled"` |
| `old_stream_id` | topics[1] | `u64` | **Indexed.** Original stream identifier |
| `new_stream_id` | data[0] | `u64` | Replacement stream identifier |
| `sender` | data[1] | `Address` | Stream creator |
| `refund_amount` | data[2] | `i128` | Tokens returned to sender |
| `new_deposit` | data[3] | `i128` | Deposit locked in the new stream |

---

### `StreamTerminatedByRecipient`

Emitted when a recipient terminates a stream early via `recipient_terminate`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamTerminatedByRecipient"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `recipient` | data[0] | `Address` | Recipient who terminated |
| `recipient_amount` | data[1] | `i128` | Tokens received by recipient |
| `refund_amount` | data[2] | `i128` | Tokens returned to sender |

---

### `RecipientTransferred`

Emitted when claim rights are transferred to a new recipient via `transfer_recipient`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"RecipientTransferred"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `old_recipient` | data[0] | `Address` | Previous recipient |
| `new_recipient` | data[1] | `Address` | New recipient |

---

### `StreamArchived`

Emitted when a fully settled stream is archived via `archive_stream`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamArchived"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `sender` | data[0] | `Address` | Stream creator |
| `recipient` | data[1] | `Address` | Stream beneficiary |
| `total_amount` | data[2] | `i128` | Total deposit that was streamed |

---

### `MetadataUpdated`

Emitted when the metadata blob of a stream is updated via `update_metadata`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"MetadataUpdated"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `metadata` | data | `Bytes` | New metadata blob (max 64 bytes) |

---

### `AutoRenewCancelled`

Emitted when auto-renewal is disabled for a stream via `cancel_auto_renew`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"AutoRenewCancelled"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |

Data: `()` (no data payload)

---

### `AutoRenewFailed`

Emitted when an auto-renew cycle cannot start because the sender has insufficient balance.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"AutoRenewFailed"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream identifier |
| `sender` | data[0] | `Address` | Sender whose balance was insufficient |
| `required` | data[1] | `i128` | Amount required for renewal (stroops) |

---

### `StreamRenewed`

Emitted when an auto-renew cycle successfully starts.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"StreamRenewed"` |
| `old_stream_id` | topics[1] | `u64` | **Indexed.** Previous cycle stream identifier |
| `new_stream_id` | data | `u64` | New cycle stream identifier |

---

### `FeeCollected`

Emitted when a protocol fee is deducted from a withdrawal and sent to the treasury.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"FeeCollected"` |
| `stream_id` | topics[1] | `u64` | **Indexed.** Stream the fee was collected from |
| `amount` | data[0] | `i128` | Fee amount in stroops |
| `treasury` | data[1] | `Address` | Treasury address that received the fee |

---

### `CreationFeeCollected`

Emitted when the flat XLM creation fee is deducted from the sender at stream creation time.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"CreationFeeCollected"` |
| `fee_amount` | data[0] | `i128` | XLM fee in stroops |
| `treasury` | data[1] | `Address` | Treasury address that received the fee |

---

### `FeeChangeProposed`

Emitted when an admin proposes a protocol fee change, starting the 7-day timelock.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"FeeChangeProposed"` |
| `new_fee` | data[0] | `u32` | Proposed fee in basis points |
| `unlock_time` | data[1] | `u64` | Ledger timestamp when the change can be executed |

---

### `FeeChangeExecuted`

Emitted when a timelocked fee change is applied via `execute_fee_change`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"FeeChangeExecuted"` |
| `new_fee` | data[0] | `u32` | New protocol fee in basis points |

---

### `ContractDeployed`

Emitted once during `initialize`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"ContractDeployed"` |
| `version` | data[0] | `String` | Contract version string |
| `admin` | data[1] | `Address` | Initial admin address |

---

### `ContractPaused`

Emitted when the contract is paused via `emergency_pause`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"ContractPaused"` |
| `admin` | topics[1] | `Address` | **Indexed.** Admin who triggered the pause |
| `timestamp` | data | `u64` | Ledger timestamp of the pause |

---

### `ContractResumed`

Emitted when the contract is unpaused via `emergency_resume`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"ContractResumed"` |
| `admin` | topics[1] | `Address` | **Indexed.** Admin who triggered the resume |
| `timestamp` | data | `u64` | Ledger timestamp of the resume |

---

### `ContractMigrated`

Emitted when a WASM migration step is applied via `migrate`.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"ContractMigrated"` |
| `from_version` | data[0] | `String` | Previous contract version |
| `to_version` | data[1] | `String` | New contract version |
| `admin` | data[2] | `Address` | Admin who applied the migration |

---

### `AdminAction`

Emitted alongside specific admin operations (`emergency_pause`, `emergency_resume`, `migrate`) and written to the circular audit log.

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `event_name` | topics[0] | `Symbol` | `"AdminAction"` |
| `instruction` | data[0] | `String` | Name of the admin instruction |
| `admin` | data[1] | `Address` | Admin address that acted |
| `timestamp` | data[2] | `u64` | Ledger timestamp |

---

## Version History

| Schema Version | Contract Version | Changes |
|----------------|-----------------|---------|
| `1.0.0` | `1.0.0` | Initial schema — all events listed above |
