use soroban_sdk::{contracttype, Address};

/// A payment stream from sender → recipient over a time window.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    /// Total tokens deposited for this stream cycle.
    pub deposit: i128,
    /// Tokens already claimed by the recipient.
    pub claimed: i128,
    /// Tokens released per ledger-second.
    pub rate_per_second: i128,
    /// Ledger timestamp when the stream begins.
    pub start_time: u64,
    /// Ledger timestamp when the stream ends.
    pub end_time: u64,
    /// Whether the stream renews automatically after expiry.
    pub auto_renew: bool,
    /// Whether the stream has been cancelled.
    pub cancelled: bool,
}

/// A pending multisig stream proposal awaiting co-signer approval.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamProposal {
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub deposit: i128,
    pub rate_per_second: i128,
    pub duration: u64,
    pub auto_renew: bool,
    /// The address that must co-sign to activate the stream.
    pub cosigner: Address,
    /// Ledger sequence number after which the proposal expires.
    pub expiry_ledger: u32,
}

/// Top-level contract storage keys.
#[contracttype]
pub enum DataKey {
    /// Stores a Stream by its u64 ID.
    Stream(u64),
    /// Monotonically increasing counter for stream IDs.
    StreamCount,
    /// Stores a StreamProposal by its u64 proposal ID.
    Proposal(u64),
    /// Counter for proposal IDs.
    ProposalCount,
    /// Per-sender reputation score: DataKey::Reputation(Address) → u64.
    Reputation(Address),
}
