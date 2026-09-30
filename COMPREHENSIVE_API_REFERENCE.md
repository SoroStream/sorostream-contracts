# SoroStream Contract - Comprehensive API Reference

## Overview

SoroStream is a Soroban smart contract enabling real-time payment streaming. This document covers all public entry points with parameter types, return values, error codes, and usage examples.

**Table of Contents:**
1. [Core Concepts](#core-concepts)
2. [Error Codes](#error-codes)
3. [Data Types](#data-types)
4. [Stream Management](#stream-management)
5. [Withdrawal & Settlement](#withdrawal--settlement)
6. [Query & Retrieval](#query--retrieval)
7. [Administrative](#administrative)
8. [Advanced Features](#advanced-features)

---

## Core Concepts

### Stroops vs Tokens

All amounts in SoroStream are specified in **stroops** (1 stroop = 10^-7 tokens). This allows integer-only arithmetic to avoid floating-point rounding errors.

### Stream ID

Each stream is identified by a unique 64-bit ID generated deterministically from:
- Sender address
- Recipient address  
- Timestamp
- Nonce (sender-provided)

### Flow Rate

Computed at stream creation: `flow_rate = deposit / duration_seconds` (integer division, floors).

Example: `deposit=1,000,003 stroops, duration=1000 seconds` → `flow_rate=1,000 stroops/sec`

The remaining 3 stroops are refunded to the sender when the stream completes.

### Stream Status

- **Active**: Tokens are flowing from sender to recipient
- **Paused**: Streaming is temporarily halted; can be resumed
- **Completed**: Stream has ended; no further withdrawals possible
- **Cancelled**: Stream was terminated early by sender
- **PendingApproval**: Recipient must approve before streaming begins
- **EscrowHold**: Funds locked, awaiting sender activation
- **Expired**: Stream's TTL has expired; recovery period may apply

---

## Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 1 | `StreamNotFound` | Stream ID does not exist |
| 2 | `NotRecipient` | Caller is not the stream recipient |
| 3 | `NotSender` | Caller is not the stream sender |
| 4 | `StreamNotActive` | Stream is not in Active state (may be Paused, Completed, Cancelled, etc.) |
| 5 | `ZeroAmount` | Amount or holdback is zero or negative |
| 6 | `InvalidDuration` | Duration is invalid (zero, too short, or too long) |
| 8 | `InvalidCliff` | Cliff time is greater than stream duration |
| 9 | `AlreadyInitialized` | Contract already initialized; cannot reinitialize |
| 10 | `NotInitialized` | Contract not initialized |
| 11 | `DuplicateStream` | Sender+recipient+nonce combination already used |
| 14 | `ContractPaused` | Contract is under emergency pause; operations blocked |
| 15 | `Overflow` | Arithmetic overflow during calculation |
| 16 | `ZeroFlowRate` | Deposit too small relative to duration (flow_rate rounds to zero) |
| 17 | `BatchLengthMismatch` | Batch arrays (recipients, amounts, tokens) have different lengths |
| 18 | `TokenMismatch` | Token address mismatch (e.g., cancel with different token) |
| 19 | `StreamLocked` | Sender has irrevocably locked the stream via `lock_stream` |
| 20 | `NotAuthorized` | Caller lacks required authorization |
| 21 | `StreamNotPaused` | Stream is not paused (required for resume) |
| 22 | `StreamDurationTooShort` | Duration is below contract's `min_duration` |
| 25 | `InvalidNonce` | Nonce does not match expected value |
| 26 | `MigrationAlreadyApplied` | Migration has already been applied |
| 27 | `StreamNotSettled` | Stream end reached but not fully withdrawn |
| 28 | `WithdrawalCooldown` | Withdrawal cooldown period is active |
| 29 | `RecipientNotWhitelisted` | Recipient not on whitelist (if enabled) |
| 31 | `InvalidEndTime` | End time is invalid (not > start time) |
| 34 | `ReentrancyDetected` | Reentrancy protection triggered |
| 35 | `InvalidMetadataUri` | Metadata URI exceeds 128 bytes |
| 36 | `StreamNotComplete` | Stream is not complete (required for recovery) |
| 37 | `TokenNotWhitelisted` | Token not on whitelist (if enabled) |
| 38 | `InvalidTranches` | Tranches are invalid (negative amount, unsorted, sum ≠ deposit) |
| 41 | `RateLimitExceeded` | Sender exceeded rate limit for stream creation |
| 43 | `InvalidSlippage` | Slippage deviation exceeds max allowed |
| 44 | `DurationExceedsMax` | Duration exceeds contract's `max_duration` |
| 45 | `InvalidTokenAddress` | Token address is not a valid Stellar asset contract |
| 46 | `StartTimeTooFar` | Scheduled start_time exceeds `max_future_start_offset` |
| 47 | `IDCollision` | Stream ID collision (extremely rare) |
| 48 | `NextStepNotReached` | Withdrawal step boundary not yet reached |
| 49 | `AmountBelowMinimum` | Claimable amount below configured minimum |
| 50 | `InvalidExpiryWindow` | Expiry warning window is zero |
| 51 | `NewSenderStreamCapExceeded` | New sender exceeded max concurrent streams cap |
| 52 | `InvalidRedirectTarget` | Redirect target stream does not exist |
| 53 | `CircularRedirect` | Setting redirect would create a cycle |
| 54 | `RedirectRecipientMismatch` | Target stream's recipient differs |
| 55 | `DuplicateTokenInDualStream` | Dual stream uses same token twice |
| 56 | `NotDualStream` | Operation requires dual-token stream but stream is single-token |
| 57 | `IsDualStream` | Operation requires single-token stream but stream is dual-token |
| 58 | `StreamNonTransferable` | Stream marked non-transferable; can't change recipient |
| 59 | `AwaitingApproval` | Stream awaiting recipient approval before flowing |
| 60 | `StreamIsLocked` | Sender locked stream; cannot cancel |
| 61 | `RecipientNotAllowed` | Recipient not on allowlist (if enforcement enabled) |
| 63 | `InvalidPartialCancel` | Partial cancel amount invalid |

---

## Data Types

### Stream

Complete stream record:

```rust
pub struct Stream {
    pub id: u64,                                    // Unique stream ID
    pub sender: Address,                            // Stream creator (payer)
    pub recipient: Address,                         // Stream beneficiary
    pub token: Address,                             // SAC token contract
    pub deposit: i128,                              // Total deposit (stroops)
    pub flow_rate: i128,                            // Tokens/second
    pub start_time: u64,                            // Unix timestamp (seconds)
    pub cliff_time: u64,                            // Cliff unlock time
    pub lock_until: u64,                            // Withdrawal lock time
    pub end_time: u64,                              // Stream end time
    pub last_withdraw_time: u64,                    // Last withdrawal timestamp
    pub status: StreamStatus,                       // Active, Paused, Completed, etc.
    pub auto_renew: bool,                           // Auto-renew enabled
    pub renew_count: Option<u32>,                   // Max auto-renewals (None=unlimited)
    pub renewals_used: u32,                         // Renewals completed so far
    pub allow_recipient_termination: bool,          // Recipient can terminate early
    pub total_withdrawn: i128,                      // Total amount withdrawn
    pub holdback_amount: i128,                      // Escrow holdback
    pub holdback_claimed: bool,                     // Holdback released
    pub metadata: Bytes,                            // Arbitrary metadata blob
    pub metadata_uri: Option<String>,               // Off-chain metadata URI
    pub non_transferable: bool,                     // Recipient locked at creation
    pub sender_locked: bool,                        // Sender irrevocably locked
    pub is_dual_stream: bool,                       // Dual-token stream flag
    pub redirect_to_stream_id: Option<u64>,        // Redirect target (if set)
    pub tag: Option<String>,                        // Custom tag for grouping
    // ... additional fields for vesting curves, milestones, etc.
}
```

### StreamStatus (Enum)

```rust
pub enum StreamStatus {
    Active,                 // Currently streaming
    Paused,                 // Temporarily halted
    Completed,              // Natural completion
    Cancelled,              // Terminated by sender
    PendingApproval,        // Awaiting recipient approval
    EscrowHold,             // Awaiting sender activation
    Expired,                // TTL expired
}
```

### VestingCurve (Enum)

```rust
pub enum VestingCurve {
    Linear,                 // Constant rate: claimable = flow_rate × elapsed
    TimeDecay(u32),        // Exponential front-weighting (decay_factor in bps)
}
```

### VestingTranche (Step-vesting)

```rust
pub struct VestingTranche {
    pub unlock_time: u64,   // Timestamp when tokens unlock
    pub amount: i128,       // Amount unlocking (stroops)
}
```

### StreamHealth

```rust
pub struct StreamHealth {
    pub current_ledger: u32,            // Current ledger number
    pub end_time: u64,                  // Stream end timestamp
    pub ttl_remaining_ledgers: u32,    // Ledgers until storage eviction
    pub status: HealthStatus,           // Healthy | TTLWarning | AtRisk
}
```

### Stats (Contract Statistics)

```rust
pub struct Stats {
    pub total_streams: u64,             // Lifetime streams created
    pub active_streams: u64,            // Currently active streams
    pub total_volume: i128,             // Total deposit across all streams (stroops)
}
```

### StreamQueryFilter

```rust
pub struct StreamQueryFilter {
    pub status: Option<StreamStatus>,   // Filter by status
    pub asset: Option<Address>,         // Filter by token
    pub sender: Option<Address>,        // Filter by sender
    pub recipient: Option<Address>,     // Filter by recipient
}
```

---

## Stream Management

### create_stream

Creates a basic linear payment stream.

**Parameters:**
```rust
env: Env,                                   // Soroban environment
sender: Address,                            // Stream creator (must auth)
recipient: Address,                         // Stream beneficiary
token: Address,                             // SAC token address
amount: i128,                               // Total deposit (stroops)
duration_seconds: u64,                      // Duration (seconds)
cliff_seconds: u64,                         // Cliff duration (≤ duration)
nonce: u64,                                 // Unique per sender (prevents duplicate creation)
auto_renew: bool,                           // Auto-renew on completion
renew_count: Option<u32>,                   // Max renewals (None=unlimited)
lock_until: u64,                            // Withdrawal lock time (Unix timestamp)
allow_recipient_termination: bool,          // Recipient can terminate early
holdback_amount: i128,                      // Amount held in escrow
withdrawal_steps: Option<u32>,              // Step-withdrawal boundaries
min_withdrawal_amount: Option<i128>,        // Minimum claimable per withdrawal
non_transferable: bool,                     // Lock recipient at creation
requires_recipient_approval: bool,          // Recipient must approve first
enforce_recipient_allowlist: bool,          // Check recipient allowlist
```

**Returns:** `Result<u64, StreamError>`
- Success: Stream ID
- Errors: `ZeroAmount`, `InvalidDuration`, `InvalidCliff`, `ZeroFlowRate`, `DuplicateStream`, `RateLimitExceeded`, `RecipientNotWhitelisted`, `TokenNotWhitelisted`, etc.

**Example:**
```rust
let stream_id = client.create_stream(
    &sender,                    // Who pays
    &recipient,                 // Who receives
    &usdc_token,               // Token contract
    &1_000_000,                // 0.1 USDC (100M stroops)
    &2_592_000,                // 30 days in seconds
    &0,                        // No cliff
    &0u64,                     // Unique nonce
    &false,                    // Don't auto-renew
    &None::<u32>,              // Unlimited renewals (if auto-renew enabled)
    &0u64,                     // No lock period
    &false,                    // Recipient can't terminate
    &0i128,                    // No holdback
    &None::<u32>,              // No step withdrawals
    &None::<i128>,             // No minimum withdrawal
    &false,                    // Transferable recipient
    &false,                    // No approval required
    &false,                    // No allowlist enforcement
)?;
```

### create_stream_with_federation

Creates a stream using a federation address name instead of Stellar address.

**Parameters:**
```rust
env: Env,
sender: Address,
federation_name: String,                    // e.g., "alice*example.com"
token: Address,
amount: i128,
duration_seconds: u64,
cliff_seconds: u64,
nonce: u64,
auto_renew: bool,
renew_count: Option<u32>,
lock_until: u64,
allow_recipient_termination: bool,
```

**Returns:** `Result<u64, StreamError>`

**Errors:** All `create_stream` errors plus `StreamNotFound` if federation name not registered.

### create_stream_with_curve

Creates a stream with a custom vesting curve (linear or time-decay).

**Parameters:**
```rust
env: Env,
sender: Address,
recipient: Address,
token: Address,
amount: i128,
duration_seconds: u64,
cliff_seconds: u64,
nonce: u64,
auto_renew: bool,
renew_count: Option<u32>,
lock_until: u64,
allow_recipient_termination: bool,
curve: VestingCurve,                        // Linear or TimeDecay(factor)
on_complete_contract: Option<Address>,      // Callback contract
on_complete_function: Option<Symbol>,       // Callback function name
escrow_hold: bool,                          // Start in EscrowHold state
```

**Returns:** `Result<u64, StreamError>`

**Example (Time-Decay Curve):**
```rust
// Front-weighted vesting: more tokens available early
let curve = VestingCurve::TimeDecay(500);  // 5% decay per 1000s window

let stream_id = client.create_stream_with_curve(
    &sender,
    &recipient,
    &token,
    &1_000_000,
    &31_536_000,               // 1 year
    &0,
    &0u64,
    &false,
    &None,
    &0u64,
    &false,
    &curve,
    &None,                     // No callback
    &None,
    &false,                    // Not escrow held
)?;
```

### create_stream_with_schedule

Creates a stream with step-vesting (discrete tranches).

**Parameters:**
```rust
env: Env,
sender: Address,
recipient: Address,
token: Address,
deposit: i128,
tranches: Vec<VestingTranche>,              // [(amount, unlock_time), ...]
nonce: u64,
lock_until: u64,
allow_recipient_termination: bool,
oracle: Option<Address>,                    // Price oracle contract
max_price_deviation_bps: u32,              // Max deviation in basis points
```

**Returns:** `Result<u64, StreamError>`

**Errors:** `InvalidTranches` if amounts don't sum to deposit or times unsorted.

**Example:**
```rust
let tranches = vec![
    VestingTranche { unlock_time: 1000, amount: 333_333 },
    VestingTranche { unlock_time: 2000, amount: 333_334 },
    VestingTranche { unlock_time: 3000, amount: 333_333 },
];

let stream_id = client.create_stream_with_schedule(
    &sender,
    &recipient,
    &token,
    &1_000_000,
    &tranches,
    &0u64,
    &0u64,
    &false,
    &None,                     // No oracle
    &0u32,
)?;
```

### create_stream_with_milestones

Creates a milestone-gated stream where tokens unlock at specific times.

**Parameters:**
```rust
env: Env,
sender: Address,
recipient: Address,
token: Address,
deposit: i128,
milestones: Vec<(i128, u64, BytesN<32>)>,  // (amount, unlock_time, description_hash)
nonce: u64,
lock_until: u64,
allow_recipient_termination: bool,
```

**Returns:** `Result<u64, StreamError>`

### create_stream_scheduled

Creates a stream starting at a future time.

**Parameters:**
```rust
env: Env,
sender: Address,
recipient: Address,
token: Address,
amount: i128,
duration_seconds: u64,
start_time: u64,                            // Future start (must be: now ≤ start_time ≤ now + max_future_start_offset)
cliff_seconds: u64,
nonce: u64,
auto_renew: bool,
lock_until: u64,
allow_recipient_termination: bool,
holdback_amount: i128,
on_complete_contract: Option<Address>,
on_complete_function: Option<Symbol>,
escrow_hold: bool,
```

**Returns:** `Result<u64, StreamError>`

**Errors:** `StartTimeTooFar` if start_time exceeds max future offset.

### create_dual_stream

Creates a dual-token stream (two tokens in one stream).

**Parameters:**
```rust
env: Env,
sender: Address,
recipient: Address,
token1: Address,
amount1: i128,
token2: Address,
amount2: i128,
duration_seconds: u64,
cliff_seconds: u64,
nonce: u64,
lock_until: u64,
allow_recipient_termination: bool,
```

**Returns:** `Result<u64, StreamError>`

**Errors:** `DuplicateTokenInDualStream` if `token1 == token2`.

---

## Withdrawal & Settlement

### withdraw

Recipient claims all earned tokens.

**Parameters:**
```rust
env: Env,
stream_id: u64,                             // Stream to withdraw from
recipient: Address,                         // Must be stream recipient (must auth)
```

**Returns:** `Result<(), StreamError>`

**Errors:** `StreamNotFound`, `NotRecipient`, `StreamNotActive`, `AwaitingApproval`, `WithdrawalCooldown`, `Overflow`

**Behavior:**
1. Calculates claimable amount: `flow_rate × (now - last_withdraw_time)`
2. Clamps to available balance (not yet withdrawn)
3. Transfers tokens to recipient
4. Updates `stream.total_withdrawn` and `stream.last_withdraw_time`
5. If stream has ended:
   - Calculates dust (remaining tokens due to rounding)
   - If auto-renew enabled: renews stream or completes if limit reached
   - Otherwise: returns dust to sender

**Example:**
```rust
client.withdraw(&stream_id, &recipient)?;
```
### get_stream_earnings_estimate

Returns the projected gross token earnings through the stream's scheduled end, including prior withdrawals and remaining vesting. Ongoing pauses use the pause timestamp as the effective current time; resumed pauses are reflected in the stream's adjusted timestamps. The estimate is before protocol fees.

```rust
let projected_total = client.get_stream_earnings_estimate(&stream_id)?;
```

### batch_withdraw

Withdraw from multiple streams in one transaction.

**Parameters:**
```rust
env: Env,
stream_ids: Vec<u64>,                       // Multiple streams
recipient: Address,                         // Must be recipient of all (must auth)
```

**Returns:** `Result<Vec<i128>, StreamError>`
- Success: Array of amounts withdrawn per stream
- Error: First error encountered stops batch

### cancel_stream

Sender terminates stream early. Recipient gets earned amount; sender gets remainder.

**Parameters:**
```rust
env: Env,
stream_id: u64,
sender: Address,                            // Must be stream sender (must auth)
```

**Returns:** `Result<(), StreamError>`

**Errors:** `StreamNotFound`, `NotSender`, `StreamNotActive`, `StreamIsLocked` (if sender called `lock_stream`)

**Behavior:**
1. Calculates earned amount: `flow_rate × (now - start_time)`
2. Transfers earned to recipient
3. Refunds remainder to sender
4. Removes stream record

**Example:**
```rust
client.cancel_stream(&stream_id, &sender)?;
```

### batch_cancel_stream

Cancel multiple streams.

**Parameters:**
```rust
env: Env,
stream_ids: Vec<u64>,
sender: Address,                            // Must be sender of all (must auth)
```

**Returns:** `Result<Vec<Result<(), StreamError>>, StreamError>`
- Each element is per-stream result

### partial_cancel_stream

Reduce stream amount, creating a remainder stream.

**Parameters:**
```rust
env: Env,
stream_id: u64,
sender: Address,                            // Must be sender (must auth)
cancel_amount: i128,                        // Amount to remove from stream
```

**Returns:** `Result<u64, StreamError>`
- Success: ID of newly created stream with canceled amount
- Errors: `InvalidPartialCancel`, stream not active, insufficient balance

**Behavior:**
1. Original stream: reduced by `cancel_amount`
2. New stream: created with `cancel_amount`
3. Both inherit other parameters (recipient, token, end_time, etc.)

### top_up

Sender adds more funds to active stream, extending end time.

**Parameters:**
```rust
env: Env,
stream_id: u64,
sender: Address,                            // Must be sender (must auth)
token: Address,                             // Must match stream token
amount: i128,                               // Amount to add (stroops)
```

**Returns:** `Result<(), StreamError>`

**Errors:** `StreamNotFound`, `NotSender`, `TokenMismatch`, `StreamNotActive`

**Behavior:**
1. Adds `amount` to stream `deposit` and contract balance
2. Recalculates `flow_rate` (may change if amount is odd)
3. Extends `end_time` automatically

**Example:**
```rust
// Add 0.1 USDC to stream, extending it
client.top_up(&stream_id, &sender, &usdc_token, &100_000)?;
```

### update_stream_rate

Sender changes the flow rate of an active stream.

**Parameters:**
```rust
env: Env,
stream_id: u64,
sender: Address,                            // Must be sender (must auth)
new_rate: i128,                             // New stroops/second
```

**Returns:** `Result<(), StreamError>`

**Errors:** `NotSender`, `StreamNotActive`, `ZeroFlowRate`, `InsufficientBalance`

**Behavior:**
1. Settles current earned tokens at old rate
2. Adjusts remaining deposit and end time to support new rate
3. Recipient always gets promised total

---

## Query & Retrieval

### get_stream

Retrieve full stream record.

**Parameters:**
```rust
env: Env,
stream_id: u64,
```

**Returns:** `Result<Stream, StreamError>`

**Errors:** `StreamNotFound`

**Example:**
```rust
let stream = client.get_stream(&stream_id)?;
println!("Flow rate: {} stroops/sec", stream.flow_rate);
```

### get_claimable

Calculate currently withdrawable amount.

**Parameters:**
```rust
env: Env,
stream_id: u64,
```

**Returns:** `Result<i128, StreamError>`
- Amount in stroops
- Returns 0 if before cliff or stream paused/completed

**Errors:** `StreamNotFound`

**Example:**
```rust
let claimable = client.get_claimable(&stream_id)?;
println!("Can withdraw: {} stroops", claimable);
```

### get_all_stream_ids

List all stream IDs (paginated).

**Parameters:**
```rust
env: Env,
start: u32,                                 // Pagination offset
limit: u32,                                 // Max results per page
```

**Returns:** `Vec<u64>`

### get_streams_by_sender

List all streams for a sender.

**Parameters:**
```rust
env: Env,
sender: Address,
start: u32,
limit: u32,
```

**Returns:** `Vec<Stream>`

### get_streams_by_recipient

List all streams for a recipient.

**Parameters:**
```rust
env: Env,
recipient: Address,
start: u32,
limit: u32,
```

**Returns:** `Vec<Stream>`

### get_active_streams_by_sender

List active (non-paused, non-completed) streams for a sender.

**Parameters:**
```rust
env: Env,
sender: Address,
```

**Returns:** `Vec<Stream>`

### get_active_streams_by_recipient

List active streams for a recipient.

**Parameters:**
```rust
env: Env,
recipient: Address,
```

**Returns:** `Vec<Stream>`

### query_streams

Advanced filtering with optional status, token, sender, recipient.

**Parameters:**
```rust
env: Env,
filter: StreamQueryFilter,
start: u32,
limit: u32,
```

**Returns:** `Vec<Stream>`

**Example:**
```rust
let filter = StreamQueryFilter {
    status: Some(StreamStatus::Active),
    asset: Some(usdc_token),
    sender: None,
    recipient: Some(recipient_address),
};

let streams = client.query_streams(&filter, 0, 10)?;
```

### simulate_claimable

Calculate claimable at a future time (off-chain preview).

**Parameters:**
```rust
env: Env,
stream_id: u64,
query_time: u64,                            // Unix timestamp to query
```

**Returns:** `Result<i128, StreamError>`

**Example:**
```rust
// What will be claimable in 1 week?
let future_time = env.ledger().timestamp() + 604_800;
let future_claimable = client.simulate_claimable(&stream_id, future_time)?;
```

### is_participant

Check if address is sender or recipient of a stream.

**Parameters:**
```rust
env: Env,
stream_id: u64,
address: Address,
```

**Returns:** `Result<bool, StreamError>`

### get_stream_health

Check on-chain storage health (TTL remaining).

**Parameters:**
```rust
env: Env,
stream_id: u64,
```

**Returns:** `Result<StreamHealth, StreamError>`

**Example:**
```rust
let health = client.get_stream_health(&stream_id)?;
match health.status {
    HealthStatus::Healthy => println!("Storage OK"),
    HealthStatus::TTLWarning => println!("Consider bumping TTL"),
    HealthStatus::AtRisk => println!("TTL expiry imminent!"),
}
```

### get_nonce

Get next available nonce for a sender.

**Parameters:**
```rust
env: Env,
sender: Address,
```

**Returns:** `u64`

---

## Administrative

### initialize

Initialize contract (must be called once before any streams).

**Parameters:**
```rust
env: Env,
admin: Address,                             // Contract administrator
version: String,                            // Version identifier (e.g., "1.0.0")
```

**Returns:** `Result<(), StreamError>`

**Errors:** `AlreadyInitialized` if called twice

### get_admin

Get current admin address.

**Parameters:**
```rust
env: Env,
```

**Returns:** `Result<Address, StreamError>`

### set_admin

Transfer admin privileges.

**Parameters:**
```rust
env: Env,
new_admin: Address,                         // Must auth
```

**Returns:** `Result<(), StreamError>`

### set_max_streams

Set global max concurrent streams limit.

**Parameters:**
```rust
env: Env,
max_streams: u32,
```

**Returns:** `Result<(), StreamError>`

### set_sender_stream_limit

Set per-sender stream cap (overrides default).

**Parameters:**
```rust
env: Env,
sender: Address,
limit: u32,
```

**Returns:** `Result<(), StreamError>`

### emergency_pause

Pause entire contract (emergency only).

**Parameters:**
```rust
env: Env,
```

**Returns:** `Result<(), StreamError>`

**Effect:** All stream operations blocked until `emergency_resume`.

### emergency_resume

Resume contract after pause.

**Parameters:**
```rust
env: Env,
```

**Returns:** `Result<(), StreamError>`

### is_paused

Check if contract is paused.

**Parameters:**
```rust
env: Env,
```

**Returns:** `bool`

### upgrade

Deploy new WASM contract code.

**Parameters:**
```rust
env: Env,
new_wasm_hash: BytesN<32>,                  // Hash of new WASM
```

**Returns:** `Result<(), StreamError>`

### get_version

Get contract version string.

**Parameters:**
```rust
env: Env,
```

**Returns:** `Result<String, StreamError>`

### migrate

Run one-time migration after WASM upgrade.

**Parameters:**
```rust
env: Env,
from_version: String,
to_version: String,
```

**Returns:** `Result<(), StreamError>`

### set_protocol_fee

Set fee charged on all withdrawals (in basis points).

**Parameters:**
```rust
env: Env,
fee_bps: u32,                               // 100 = 1%
```

**Returns:** `Result<(), StreamError>`

### get_protocol_fee_info

Get current fee and treasury address.

**Parameters:**
```rust
env: Env,
```

**Returns:** `(u32, Option<Address>)`
- Tuple of (fee_bps, treasury_address)

### propose_fee_change

Propose a fee change with timelock.

**Parameters:**
```rust
env: Env,
admin: Address,                             // Must auth
new_fee_bps: u32,
```

**Returns:** `Result<(), StreamError>`

### execute_fee_change

Execute previously proposed fee change.

**Parameters:**
```rust
env: Env,
```

**Returns:** `Result<(), StreamError>`

### set_treasury_address

Set address that receives protocol fees.

**Parameters:**
```rust
env: Env,
treasury: Address,
```

**Returns:** `Result<(), StreamError>`

### get_stats

Get contract-wide statistics.

**Parameters:**
```rust
env: Env,
```

**Returns:** `Stats`

**Example:**
```rust
let stats = client.get_stats()?;
println!("Total streams: {}", stats.total_streams);
println!("Active: {}", stats.active_streams);
println!("Volume: {} stroops", stats.total_volume);
```

### recalibrate_stats

Recalculate statistics from storage (admin only).

**Parameters:**
```rust
env: Env,
admin: Address,                             // Must auth
```

**Returns:** `Result<(), StreamError>`

---

## Advanced Features

### Pause & Resume

**pause_stream** - Temporarily halt a stream (sender only).

```rust
client.pause_stream(&stream_id, &sender)?;

let stream = client.get_stream(&stream_id)?;
assert_eq!(stream.status, StreamStatus::Paused);
```

**resume_stream** - Resume paused stream, extending end time by pause duration.

```rust
client.resume_stream(&stream_id, &sender)?;

let stream = client.get_stream(&stream_id)?;
assert_eq!(stream.status, StreamStatus::Active);
// end_time has been shifted forward
```

### Recipient Approval (requires_recipient_approval)

Stream created with `requires_recipient_approval = true` starts in `PendingApproval` state.

Recipient must call `approve_stream()` to activate streaming:

```rust
// Recipient approves
client.approve_stream(&stream_id, &recipient)?;

// Now active, recipient can withdraw
client.withdraw(&stream_id, &recipient)?;
```

### Holdback Escrow

`holdback_amount` is locked separately; sender must explicitly release it.

```rust
// Release holdback to recipient
client.release_holdback(&stream_id, &sender)?;

// Or claw back to sender
client.claw_back_holdback(&stream_id, &sender)?;
```

### Stream Redirect

Recipient can redirect withdrawals to another stream (composable DeFi).

```rust
// Redirect this stream to top-up another stream
client.set_redirect(&stream_id, &target_stream_id, &recipient)?;

// Now withdraw() on stream_id tops up target_stream_id instead
client.withdraw(&stream_id, &recipient)?;

// Clear redirect
client.clear_redirect(&stream_id, &recipient)?;
```

### Rate Limiting

Senders have rate limits on stream creation frequency.

```rust
let remaining = client.remaining_quota(&sender)?;
println!("Sender can create {} more streams this hour", remaining);
```

Admin can configure and exempt:

```rust
client.set_rate_limit_window(&admin, &720u32)?;  // 720 ledgers (~1 hour)
client.set_rate_limit_max(&admin, &20u32)?;      // Max 20 per window
client.add_rate_limit_exempt(&admin, &trusted_sender)?;
```

### Delegation

Sender can delegate stream management to another address.

```rust
// Delegate to manager
client.set_delegate(&sender, &stream_id, &manager)?;

// Manager can now pause/resume (replaces sender auth)
client.pause_stream(&stream_id, &manager)?;

// Revoke delegation
client.revoke_delegate(&sender, &stream_id)?;
```

### Non-Transferable Streams

If `non_transferable = true`, recipient cannot be changed:

```rust
// This will fail
let result = client.transfer_recipient(&stream_id, &old_recipient, &new_recipient);
assert!(result.is_err());  // StreamNonTransferable
```

### Lock Stream (Sender)

Sender can irrevocably lock their stream (cannot cancel):

```rust
client.lock_stream(&stream_id, &sender)?;

// Now cancel will fail
let result = client.cancel_stream(&stream_id, &sender);
assert!(result.is_err());  // StreamIsLocked
```

### Whitelist Management

Token & recipient allowlists (compliance).

```rust
// Enable token whitelist
client.set_token_whitelist_enabled(&admin, &true)?;

// Add token
client.add_token_to_whitelist(&admin, &usdc_token)?;

// Now only whitelisted tokens can be streamed

// Recipient allowlist (different use case)
client.set_recipient_allowlist_enabled(&admin, &true)?;
client.add_to_recipient_allowlist(&admin, &compliant_address)?;

// Streams with enforce_recipient_allowlist=true require recipient on list
```

### Blocklist

Block addresses from creating/receiving streams.

```rust
// Block an address
client.add_to_blocklist(&admin, &bad_actor)?;

// Try to create stream
let result = client.create_stream(
    &bad_actor,     // sender blocked
    &recipient,
    // ...
);
assert!(result.is_err());  // AddressBlocked

// Unblock
client.remove_from_blocklist(&admin, &bad_actor)?;
```

### Fee Tiers

Different fees per token.

```rust
// Set fee for specific token (e.g., 0.5%)
client.set_token_fee_tier(&admin, &risky_token, &50u32)?;

// Default protocol fee still applies to other tokens
```

### Admin Override (Dispute Resolution)

Admin can force-cancel or force-complete streams with timelock.

```rust
// Initiate override (e.g., for disputed stream)
let request_id = client.initiate_admin_override(
    &stream_id,
    &OverrideAction::Cancel,
    &"Disputed claim".to_string(),
)?;

// Wait for timelock (default 48 hours)

// Execute after timelock expires
client.execute_admin_override(&request_id)?;
```

### Tags

Organize streams with custom tags.

```rust
// Tag a stream
client.set_stream_tag(&stream_id, &sender, &Some("payroll".to_string()))?;

// Query by tag
let payroll_streams = client.get_streams_by_tag(&sender, &"payroll".to_string(), 0, 100)?;
```

### Metadata

Store and update off-chain metadata pointers.

```rust
// Update metadata URI
client.update_metadata_uri(
    &stream_id,
    &sender,
    &Some("ipfs://QmXxxx".to_string()),
)?;

// Retrieve
let uri = client.get_metadata_uri(&stream_id)?;
```

---

## Error Handling

### Common Pattern

```rust
match client.withdraw(&stream_id, &recipient) {
    Ok(()) => println!("Withdrawal successful"),
    Err(StreamError::StreamNotFound) => println!("Stream doesn't exist"),
    Err(StreamError::NotRecipient) => println!("Not the recipient"),
    Err(StreamError::StreamNotActive) => println!("Stream paused/completed"),
    Err(e) => println!("Unexpected error: {:?}", e),
}
```

### Recovery from Grace Period

If stream's storage TTL expires but grace period is active:

```rust
match client.get_stream(&stream_id) {
    Err(StreamError::StreamNotFound) => {
        // Try recovery within grace period
        client.recover_expired(&stream_id, &sender)?;
        println!("Stream recovered");
    }
    _ => {}
}
```

---

## Examples

### Example 1: Monthly Salary Stream

```rust
let monthly_salary = 10_000_000;  // 0.1 USDC per month
let month_seconds = 2_592_000;     // 30 days

let stream_id = client.create_stream(
    &company,
    &employee,
    &usdc_token,
    &(monthly_salary * 12),        // 12 months
    &(month_seconds * 12),
    &0,                            // No cliff
    &0u64,
    &true,                         // Auto-renew yearly
    &Some(5u32),                   // Max 5 renewals
    &0u64,
    &false,
    &0i128,
    &None,
    &None,
    &false,
    &false,
    &false,
)?;

// Employee can withdraw monthly
for month in 1..=12 {
    std::thread::sleep(Duration::from_secs(month_seconds));
    client.withdraw(&stream_id, &employee)?;
    println!("Month {} salary claimed", month);
}
```

### Example 2: Vesting Schedule (3-year cliff, then linear)

```rust
let grant = 1_000_000;            // 0.01 USDC
let three_years = 94_608_000;     // Seconds
let one_year = 31_536_000;

let stream_id = client.create_stream(
    &company,
    &employee,
    &usdc_token,
    &grant,
    &(one_year * 4),               // 4 year total
    &three_years,                  // Cliff at year 3
    &0u64,
    &false,                        // No auto-renew
    &None,
    &0u64,
    &false,
    &0i128,
    &None,
    &None,
    &false,
    &false,
    &false,
)?;

// First withdrawal at year 3 (cliff)
// Then linear stream over year 4
```

### Example 3: Milestone-Based Vesting

```rust
// Release in 4 equal tranches over 2 years
let tranches = vec![
    VestingTranche {
        unlock_time: start_time + 6_months,
        amount: 250_000,
    },
    VestingTranche {
        unlock_time: start_time + 1_year,
        amount: 250_000,
    },
    VestingTranche {
        unlock_time: start_time + 1_5_years,
        amount: 250_000,
    },
    VestingTranche {
        unlock_time: start_time + 2_years,
        amount: 250_000,
    },
];

let stream_id = client.create_stream_with_schedule(
    &sender,
    &recipient,
    &token,
    &1_000_000,
    &tranches,
    &0u64,
    &0u64,
    &false,
    &None,
    &0u32,
)?;
```

### Example 4: Batch Payments (Payroll)

```rust
let recipients = vec![emp1, emp2, emp3, emp4];
let amounts = vec![1_000_000, 900_000, 800_000, 750_000];  // Different salaries
let tokens = vec![usdc; 4];  // Same token

let stream_ids = client.batch_create_stream(
    &company,
    &recipients,
    &amounts,
    &tokens,
    &(2_592_000 * 12),             // 12-month duration
    &false,                        // No auto-renew
    &None,
    &vec![0u64; 4],               // Lock times
    &nonce,
)?;

// Withdraw all at once
let amounts_withdrawn = client.batch_withdraw(&stream_ids, &recipients)?;
```

---

## Best Practices

1. **Always handle errors explicitly** - Different error codes require different handling
2. **Check stream health regularly** - Use `get_stream_health()` to monitor TTL
3. **Use batch operations** - For multiple streams, use `batch_*` for gas efficiency
4. **Verify before withdrawing** - Call `get_claimable()` before `withdraw()`
5. **Plan for rounding** - Flow rate is integer-divided; dust is refunded at completion
6. **Set appropriate parameters** - Use cliffs, locks, and approval gates where needed
7. **Monitor auto-renewal** - Track `renewal_count` to avoid unexpected stream termination
8. **Use whitelists for compliance** - Token/recipient allowlists prevent unauthorized flows

---

## Support & Questions

For contract development questions:
- GitHub: https://github.com/SoroStream/sorostream-contracts
- Documentation: See CONTRIBUTING.md and ARCHITECTURE.md
- Issues: GitHub issue tracker
