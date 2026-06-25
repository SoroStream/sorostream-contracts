use soroban_sdk::{contracttype, Address};

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
    /// Ledger timestamp when the stream ends.
    pub end_time: u64,
    /// Ledger timestamp of the last withdrawal.
    pub last_withdraw_time: u64,
    /// Current status of the stream.
    pub status: StreamStatus,
    /// Whether the stream auto-renews on completion.
    pub auto_renew: bool,
}
