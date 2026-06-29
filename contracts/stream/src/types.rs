use soroban_sdk::{contracttype, Address, Bytes, String};

/// Status of a payment stream.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    /// Stream is currently active and tokens are flowing.
    Active,
    /// Stream was cancelled before its natural end time.
    Cancelled,
    /// Stream reached its end time naturally.
    Completed,
    /// Stream is temporarily paused.
    Paused,
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
    /// Whether the recipient is allowed to terminate the stream early.
    pub allow_recipient_termination: bool,
    /// Ledger timestamp of when the stream was last paused (0 if never paused).
    pub last_pause_time: u64,
    /// Total amount withdrawn from this stream so far.
    pub total_withdrawn: i128,
    /// Optional metadata blob associated with the stream.
    pub metadata: Bytes,
}

/// Aggregate contract statistics.
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
