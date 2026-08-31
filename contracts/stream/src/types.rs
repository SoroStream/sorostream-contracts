use soroban_sdk::{contracttype, Address, Bytes, BytesN, String, Vec};

/// Vesting release curve applied to a payment stream.
///
/// Choosing `Linear` reproduces the original constant-rate behaviour.
/// Choosing `TimeDecay` produces a front-weighted (convex) release schedule
/// where more tokens are claimable early in the stream lifetime.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingCurve {
    /// Constant rate: `claimable = flow_rate × elapsed`.
    Linear,
    /// Discretised exponential decay.
    ///
    /// `decay_factor` is expressed in **basis points per 1 000 seconds**
    /// (i.e. the per-mille decay rate per 1 ks window):
    ///
    /// ```text
    /// weight(t) = deposit × (1 − decay_factor/10_000)^(t / 1_000)
    /// cumulative_claimable(t) = deposit − weight(t)   (clamped to [0, deposit])
    /// ```
    ///
    /// A `decay_factor` of `0` degenerates to linear behaviour.
    /// Practical values: 50–500 bps (0.5 %–5 % per 1 ks window).
    TimeDecay(u32),
}

/// A single step-vesting tranche: tokens that unlock atomically at `unlock_time`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VestingTranche {
    /// Ledger timestamp at which this tranche becomes claimable.
    pub unlock_time: u64,
    /// Amount of tokens (in stroops) that unlock at this timestamp.
    pub amount: i128,
}

/// Status of a payment stream.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum StreamStatus {
    /// Stream is currently active and tokens are flowing.
    Active,
    /// Stream was cancelled before its natural end time.
    Cancelled,
    /// Stream reached its end time naturally.
    Completed,
    /// Stream is temporarily paused.
    Paused,
    /// Stream has passed its end_time and been explicitly marked as expired.
    Expired,
    /// Stream was created with `requires_recipient_approval = true` and the
    /// recipient has not yet called `approve_stream`.  No tokens accrue while
    /// in this state; the sender may cancel at zero cost.
    PendingApproval,
    /// Stream was created with `escrow_hold = true` and the sender has not yet
    /// called `activate_stream`. Funds are locked in escrow; no tokens accrue.
    /// The sender may cancel at zero cost while in this state.
    EscrowHold,
}

/// Status of a milestone.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    /// Milestone is pending (not yet released by sender).
    Pending,
    /// Milestone has been released and is claimable.
    Released,
    /// Milestone was forfeited (cancelled before release).
    Forfeited,
}

/// Represents a single milestone in a gated stream.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    /// Amount of tokens for this milestone (in stroops).
    pub amount: i128,
    /// Hash of the milestone description (for reference).
    pub description_hash: BytesN<32>,
    /// Ledger timestamp at which this milestone becomes automatically claimable (0 if sender-gated).
    pub unlock_time: u64,
    /// Current status of the milestone.
    pub status: MilestoneStatus,
}

/// Extended configuration and runtime state attached to a payment stream.
///
/// Kept separate from [`Stream`] so the core struct stays within Soroban's XDR
/// field-count limit for `#[contracttype]` structs (40 fields).
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamOptions {
    /// Optional limit on the number of auto-renewals. When set, the stream will automatically
    /// renew up to this many times. Once reached, the stream will complete permanently and not renew.
    /// `None` means unlimited auto-renewals (default behaviour when auto_renew is true).
    pub renew_count: Option<u32>,
    /// Number of times this stream has been renewed so far. Starts at 0 and increments each time
    /// the stream auto-renews. Only meaningful when `auto_renew` is true.
    pub renewals_used: u32,
    /// Whether the recipient is allowed to terminate the stream early.
    pub allow_recipient_termination: bool,
    /// Ledger timestamp of when the stream was last paused (0 if never paused).
    pub last_pause_time: u64,
    /// Total amount withdrawn from this stream so far.
    pub total_withdrawn: i128,
    /// Optional metadata blob associated with the stream.
    pub metadata: Bytes,
    /// Optional URI pointing to off-chain metadata (IPFS or HTTPS, max 128 bytes).
    pub metadata_uri: Option<String>,
    /// Optional milestones for gated release (empty if not milestone-gated).
    pub milestones: Vec<Milestone>,
    /// Whether this stream uses timestamp-gated milestone release mode.
    /// When true, milestones unlock automatically at their unlock_time (no sender approval needed).
    /// When false, milestones require sender approval via release_milestone().
    pub milestone_release_mode: bool,
    /// Reentrancy guard: true if currently processing a withdrawal to prevent re-entrance.
    pub locked: bool,

    // ── Step-vesting (tranche) fields ────────────────────────────────────────

    /// Whether this stream uses step-vesting (tranche-based release).
    /// When `true`, token release is governed by `tranches` rather than the
    /// continuous flow rate.
    pub is_step_vesting: bool,
    /// Index of the next unclaimed tranche (cursor). Starts at 0.
    pub tranches_claimed: u32,

    // ── Oracle price-check fields ────────────────────────────────────────────

    /// Optional oracle contract address for on-chain price validation.
    /// When set, price is checked on stream creation and withdrawal.
    pub oracle: Option<Address>,
    /// Maximum allowed price deviation from the creation price, in basis points
    /// (e.g. 500 = 5 %).  Ignored when `oracle` is `None`.
    pub max_price_deviation_bps: u32,
    /// Token price (raw oracle value) recorded at stream-creation time.
    /// Used as the baseline for deviation calculations on subsequent calls.
    pub creation_price: i128,

    // ── Vesting curve ────────────────────────────────────────────────────────

    /// Release curve governing how tokens become claimable over time.
    /// Defaults to `VestingCurve::Linear` for all existing streams.
    pub curve: VestingCurve,

    // ── Withdrawal steps ─────────────────────────────────────────────────────

    /// Optional number of evenly-spaced withdrawal steps.
    ///
    /// When `Some(n)`, the stream duration is divided into `n` equal intervals
    /// of `(end_time - start_time) / n` seconds each.  Recipients may only call
    /// `withdraw` at or after the boundary of the next unclaimed step.
    /// `None` means free-form withdrawal (default behaviour).
    pub withdrawal_steps: Option<u32>,

    /// Index of the last completed withdrawal step (0-based).
    /// Starts at 0; incremented each time a step boundary is crossed.
    /// Only meaningful when `withdrawal_steps` is `Some`.
    pub current_step: u32,

    // ── Minimum withdrawal amount ─────────────────────────────────────────────

    /// Optional minimum claimable amount required before a withdrawal is accepted.
    ///
    /// When `Some(floor)`, `withdraw` rejects any call where the claimable
    /// amount is below `floor` — unless it is the final claim (i.e. the full
    /// remaining deposit is being drained), in which case the floor is bypassed.
    /// `None` means no minimum (default behaviour).
    pub min_withdrawal_amount: Option<i128>,

    // ── Non-transferable flag ─────────────────────────────────────────────────

    /// Whether the stream's recipient rights are locked to the original recipient.
    ///
    /// When `true`, any call to `transfer_recipient` on this stream will return
    /// `StreamError::StreamNonTransferable`.  Useful for identity-linked grants
    /// and personal vesting schedules where the sender needs on-chain enforcement
    /// of non-transferability.  Set at creation time and immutable thereafter.
    pub non_transferable: bool,

    // ── Recipient approval ────────────────────────────────────────────────────

    /// Whether this stream requires explicit recipient approval before tokens
    /// begin to accrue.
    ///
    /// When `true`, the stream is created in `StreamStatus::PendingApproval`.
    /// The recipient must call `approve_stream` to transition it to `Active`.
    /// While pending, `withdraw` returns `StreamError::AwaitingApproval` and the
    /// sender may cancel at zero cost (full deposit refunded).
    /// Set at creation time and immutable thereafter.
    pub requires_recipient_approval: bool,

    /// Ledger timestamp at which the recipient approved the stream.
    ///
    /// `0` while the stream is in `PendingApproval` state.
    /// Set by `approve_stream` and used as the effective `start_time` for all
    /// claimable-balance calculations so that no tokens accrue during the
    /// pending window.
    pub approval_timestamp: u64,

    // ── Sender-initiated irrevocable lock ─────────────────────────────────────

    /// Whether the sender has voluntarily renounced their right to cancel.
    ///
    /// Starts `false`.  Once `lock_stream` is called, transitions to `true`
    /// and cannot be reversed.  While `true`, any `cancel_stream` call from
    /// the sender (or their delegate) returns `StreamError::StreamIsLocked`.
    /// Recipients can still `withdraw` normally; admin pause is unaffected.
    pub sender_locked: bool,

    /// Optional redirect target stream ID.
    pub redirect_to_stream_id: Option<u64>,

    // ── On-complete callback (composable DeFi) ──────────────────────────────

    /// Optional contract address to invoke when the stream completes.
    /// If set, the contract's function specified by `on_complete_function` will be called.
    pub on_complete_contract: Option<Address>,
    /// Optional function signature to invoke on stream completion.
    /// Only used if `on_complete_contract` is set.
    pub on_complete_function: Option<Symbol>,

    // ── Stream comment (issue #513) ──────────────────────────────────────────

    /// Optional human-readable payment reference attached by the sender at
    /// creation time. UTF-8 text, at most 256 bytes.
    pub comment: Option<String>,
}

/// Represents a single payment stream.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Stream {
    /// Unique stream identifier.
    pub id: u64,
    /// Address of the stream creator / payer.
    pub sender: Address,
    /// Address of the stream beneficiary.
    pub recipient: Address,
    /// SAC-compatible token contract address (e.g. USDC).
    pub token: Address,
    /// Total token deposit locked in the contract (in stroops).
    pub deposit: i128,
    /// Tokens released per second (stroops/second).
    pub flow_rate: i128,
    /// Ledger timestamp when the stream started.
    pub start_time: u64,
    /// Ledger timestamp before which no tokens are claimable (>= start_time, <= end_time).
    pub cliff_time: u64,
    /// Ledger timestamp before which no withdrawals are permitted (>= start_time, <= end_time).
    pub lock_until: u64,
    /// Ledger timestamp when the stream ends.
    pub end_time: u64,
    /// Ledger timestamp of the last withdrawal.
    pub last_withdraw_time: u64,
    /// Current status of the stream.
    pub status: StreamStatus,
    /// Whether the stream auto-renews on completion.
    pub auto_renew: bool,
    /// Extended configuration and runtime state. Nested so `Stream` stays within
    /// Soroban's XDR field-count limit for `#[contracttype]` structs.
    pub options: StreamOptions,
}

/// Creation-time options for `create_stream`.
///
/// Bundles the optional per-stream configuration that would otherwise push
/// `create_stream` past Soroban's 10-parameter limit.
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct CreateStreamOptions {
    /// Optional limit on the number of auto-renewals. When set, the stream will
    /// automatically renew up to this many times. `None` means unlimited
    /// auto-renewals (default behaviour when `auto_renew` is true).
    pub renew_count: Option<u32>,
    /// Whether the recipient is allowed to terminate the stream early.
    pub allow_recipient_termination: bool,
    /// Whether the stream's recipient rights are locked to the original recipient.
    pub non_transferable: bool,
    /// Optional holdback amount kept in escrow until explicitly released (in stroops).
    /// Deducted from the streaming portion at creation time.
    pub holdback_amount: i128,
    /// Optional number of evenly-spaced withdrawal steps.
    /// `None` means free-form withdrawal (default behaviour).
    pub withdrawal_steps: Option<u32>,
    /// Optional minimum claimable amount required before a withdrawal is accepted.
    /// `None` means no minimum (default behaviour).
    pub min_withdrawal_amount: Option<i128>,
    /// Whether this stream requires explicit recipient approval before tokens
    /// begin to accrue.
    pub requires_recipient_approval: bool,
    /// Optional human-readable payment reference (UTF-8, at most 256 bytes).
    pub comment: Option<String>,
}


/// Health status of a stream's on-chain storage entry, based on its TTL.
///
/// Thresholds:
/// - `Healthy`    — TTL remaining >= 10,000 ledgers
/// - `TTLWarning` — TTL remaining in [1,000 .. 10,000) ledgers
/// - `AtRisk`     — TTL remaining < 1,000 ledgers (eviction imminent)
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HealthStatus {
    /// Stream storage TTL is comfortable (>= 10,000 ledgers remaining).
    Healthy,
    /// Stream storage TTL is getting low (< 10,000 ledgers remaining).
    /// Clients should consider calling `bump_stream_ttl` soon.
    TTLWarning,
    /// Stream storage TTL is critically low (< 1,000 ledgers remaining).
    /// The stream is at risk of being evicted from the ledger.
    AtRisk,
}

/// Snapshot of a stream's on-chain storage health.
///
/// Returned by `get_stream_health(stream_id)`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamHealth {
    /// Current ledger sequence number at the time of the query.
    pub current_ledger: u32,
    /// Stream end timestamp (Unix seconds).
    pub end_time: u64,
    /// Ledgers remaining before the stream's persistent storage entry expires.
    pub ttl_remaining_ledgers: u32,
    /// Derived health classification based on `ttl_remaining_ledgers`.
    pub status: HealthStatus,
}

/// Statistics for streams grouped by status.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StatusStats {
    /// Number of active streams.
    pub active: u64,
    /// Number of cancelled streams.
    pub cancelled: u64,
    /// Number of completed streams.
    pub completed: u64,
    /// Number of paused streams.
    pub paused: u64,
    /// Number of expired streams.
    pub expired: u64,
    /// Number of streams pending recipient approval.
    pub pending_approval: u64,
}

/// Statistics for a single asset (token).
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetStats {
    /// Token contract address.
    pub token: Address,
    /// Number of streams using this token.
    pub stream_count: u64,
    /// Total volume locked in this token (in stroops).
    pub total_volume: i128,
    /// Number of active streams using this token.
    pub active_streams: u64,
}

/// Enhanced protocol-level statistics with per-asset and per-status breakdown.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProtocolStats {
    /// Total number of streams ever created.
    pub total_streams: u64,
    /// Number of currently active streams.
    pub active_streams: u64,
    /// Sum of all deposits in stroops (across all tokens).
    pub total_volume: i128,
    /// Statistics broken down by stream status.
    pub status_breakdown: StatusStats,
    /// Statistics for each asset, ordered by total volume (descending).
    pub asset_breakdown: Vec<AssetStats>,
}

/// Aggregate contract statistics.
/// Deprecated: Use ProtocolStats instead. Kept for backwards compatibility.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Stats {
    /// Total number of streams ever created.
    pub total_streams: u64,
    /// Number of currently active streams.
    pub active_streams: u64,
    /// Sum of all deposits in stroops.
    pub total_volume: i128,
}

/// A single admin audit log entry.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AuditEntry {
    /// Name of the admin instruction (e.g. "emergency_pause").
    pub instruction: String,
    /// Admin address that performed the action.
    pub admin: Address,
    /// Ledger timestamp of the action.
    pub timestamp: u64,
    /// Serialised parameters (JSON-style string for human readability).
    pub params: String,
}


/// Override action type for administrative dispute resolution.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OverrideAction {
    /// Force-cancel the stream and split funds based on current earned amount.
    Cancel,
    /// Force-complete the stream and release remaining balance to recipient.
    Complete,
}

/// Status of an admin override request.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OverrideRequestStatus {
    /// Request has been initiated, awaiting timelock expiry.
    Pending,
    /// Timelock has expired and override can be executed.
    Ready,
    /// Override has been executed.
    Executed,
    /// Request was cancelled before execution.
    Cancelled,
}

/// An admin override request for dispute resolution on a stream.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminOverrideRequest {
    /// Unique request ID.
    pub request_id: u64,
    /// Stream ID to override.
    pub stream_id: u64,
    /// The action to perform (Cancel or Complete).
    pub action: OverrideAction,
    /// Admin who initiated this request.
    pub initiator: Address,
    /// Ledger timestamp when this request was created.
    pub created_at: u64,
    /// Ledger timestamp after which this request can be executed.
    pub executable_at: u64,
    /// Current status of the request.
    pub status: OverrideRequestStatus,
    /// Reason/description for the override.
    pub reason: String,
}

/// Status filter for `query_streams`.
///
/// A dedicated enum (rather than `Option<StreamStatus>`) sidesteps a
/// soroban-sdk 22.x limitation: `#[contracttype]` unit enums nested inside
/// an `Option<_>` field don't get the XDR `Into<ScVal>` conversion the SDK's
/// `testutils` machinery needs, which breaks the test build even though the
/// contract itself builds fine.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusFilter {
    /// No filtering on status.
    Any,
    /// Only streams with this exact status.
    Only(StreamStatus),
}

/// Per-asset on-chain analytics snapshot (Issue #468).
///
/// Unlike [`ProtocolStats`], which is recomputed by scanning every stream on
/// each call, these counters are maintained incrementally at the storage
/// mutation points (stream creation, withdrawal, cancellation) so a snapshot
/// is a handful of storage reads rather than an O(n) scan — cheap enough for
/// dashboards/indexers to poll directly instead of replaying events.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetAnalytics {
    /// The asset (token) this snapshot is for.
    pub token: Address,
    /// Cumulative amount of `token` ever paid out to recipients (via
    /// withdrawals and cancellation payouts), in stroops.
    pub total_value_streamed: i128,
    /// Total number of streams ever created using `token`.
    pub total_streams_created: u64,
    /// Total number of streams using `token` that were cancelled
    /// (cancel_stream / stop_stream / batch_cancel_stream / recipient_terminate).
    pub total_streams_cancelled: u64,
}

/// Optional filter struct for querying streams efficiently without iterating all records.
///
/// `asset`/`sender`/`recipient` are optional; a `None` value means no filtering on
/// that criterion. `status` filtering is instead controlled by `filter_by_status`
/// (rather than `Option<StreamStatus>`) because Soroban's `#[contracttype]` XDR
/// conversion for `testutils` builds only supports `Option<T>` for SDK-provided
/// types with infallible XDR conversions — not for user-defined enums like
/// `StreamStatus`, which convert fallibly.
/// Multiple filters are combined with AND logic (all must match).
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamQueryFilter {
    /// Whether to filter by status at all. When `false`, `status` is ignored.
    pub filter_by_status: bool,
    /// Status to filter by. Only applied when `filter_by_status` is `true`.
    pub status: StreamStatus,
    /// Optional asset (token) filter. If set, only streams using this token are returned.
    pub asset: Option<Address>,
    /// Optional sender filter. If set, only streams created by this address are returned.
    pub sender: Option<Address>,
    /// Optional recipient filter. If set, only streams targeting this address are returned.
    pub recipient: Option<Address>,
}

/// Less-frequently-varied options for `create_stream`, bundled into a single
/// struct so the entry point stays within Soroban's 10-parameter contract
/// function limit.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamCreateOptions {
    /// Optional limit on the number of auto-renewals (see `Stream::renew_count`).
    pub renew_count: Option<u32>,
    /// Whether the recipient is allowed to terminate the stream early.
    pub allow_recipient_termination: bool,
    /// Whether the stream's recipient rights are locked to the original recipient.
    pub non_transferable: bool,
}
