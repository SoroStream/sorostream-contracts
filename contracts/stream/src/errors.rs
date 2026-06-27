use soroban_sdk::contracterror;

/// Custom errors for the SoroStream contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StreamError {
    /// No stream exists with the given ID.
    StreamNotFound = 1,
    /// Caller is not the stream recipient.
    NotRecipient = 2,
    /// Caller is not the stream sender.
    NotSender = 3,
    /// Stream is not in Active status.
    StreamNotActive = 4,
    /// Amount must be greater than zero.
    ZeroAmount = 5,
    /// Duration must be greater than zero.
    InvalidDuration = 6,
    /// Contract has insufficient token balance.
    InsufficientBalance = 7,
    /// cliff_time must be >= start_time and <= end_time.
    InvalidCliff = 8,
    /// Contract has already been initialized.
    AlreadyInitialized = 9,
    /// Contract has not been initialized.
    NotInitialized = 10,
    /// A stream with this sender+nonce already exists.
    DuplicateStream = 11,
    /// Provided start_time is in the past.
    InvalidStartTime = 12,
    /// Partial cancel amount is invalid (exceeds remainder or leaves too little).
    InvalidPartialCancel = 13,
    /// Operation is not allowed while the contract is paused.
    ContractPaused = 14,
    /// Amount is too small relative to duration: would produce a zero flow rate.
    ZeroFlowRate = 15,
    /// Batch recipients and amounts vectors have different lengths.
    BatchLengthMismatch = 16,
    /// Token address does not match the stream's token.
    TokenMismatch = 17,
    /// A withdrawal was attempted before the stream's lock_until timestamp.
    StreamLocked = 18,
    /// A numeric operation overflowed or produced an out-of-range value.
    /// This is returned instead of panicking when user-controllable inputs
    /// (e.g. very large amounts or durations) would cause integer overflow.
    Overflow = 19,
    /// A numeric operation overflowed or produced an out-of-range value.
    /// This is returned instead of panicking when user-controllable inputs
    /// (e.g. very large amounts or durations) would cause integer overflow.
    Overflow = 18,
    /// Stream is locked until the lock_until timestamp.
    StreamLocked = 19,
}
