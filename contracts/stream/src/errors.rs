use soroban_sdk::contracterror;

/// Custom errors for the SoroStream contract (≤50 variants, Soroban XDR limit).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StreamError {
    StreamNotFound = 1,
    NotRecipient = 2,
    NotSender = 3,
    StreamNotActive = 4,
    ZeroAmount = 5,
    InvalidDuration = 6,
    InvalidCliff = 8,
    AlreadyInitialized = 9,
    NotInitialized = 10,
    DuplicateStream = 11,
    InvalidStartTime = 12,
    ContractPaused = 14,
    Overflow = 15,
    ZeroFlowRate = 16,
    BatchLengthMismatch = 17,
    StreamLocked = 19,
    NotAuthorized = 20,
    StreamNotPaused = 21,
    StreamDurationTooShort = 22,
    InvalidNonce = 25,
    MigrationAlreadyApplied = 26,
    StreamNotSettled = 27,
    WithdrawalCooldownActive = 28,
    RecipientNotWhitelisted = 29,
    InvalidEndTime = 31,
    ReentrancyDetected = 34,
    InvalidMetadataUri = 35,
    StreamNotComplete = 36,
    TokenNotWhitelisted = 37,
    InvalidTranches = 38,
    RateLimitExceeded = 41,
    InvalidSlippage = 43,
    DurationExceedsMax = 44,
    StartTimeTooFar = 46,
    IDCollision = 47,
    /// The temporary stream metadata blob exceeds 256 bytes.
    MetadataTooLong = 30,
    /// Also returned when the next withdrawal-step boundary has not been reached.
    AmountBelowMinimum = 49,
    InvalidExpiryWindow = 50,
    NewSenderStreamCapExceeded = 51,
    CircularRedirect = 53,
    /// `transfer_recipient` was called on a stream marked as non-transferable at creation.
    StreamNonTransferable = 58,
    /// `withdraw` was called on a stream still in `PendingApproval` state.
    AwaitingApproval = 59,
    /// `cancel_stream` was called on a sender-locked stream.
    StreamIsLocked = 60,
    /// Recipient is not on the admin-managed recipient allowlist.
    RecipientNotAllowed = 61,
    /// The stream deposit exceeds the maximum allowed per-token limit.
    MaxDepositExceeded = 64,
    /// Operation is not allowed while the individual stream is paused.
    StreamPaused = 67,
    /// The comment attached to a stream exceeds the 256-byte limit.
    CommentTooLong = 65,
    /// Sender has not staked the required minimum collateral for this token.
    InsufficientStake = 66,
    /// Consolidated validation error for invalid parameters and operation modes.
    InvalidParameter = 69,
    /// `create_stream` was called with a zero or otherwise unusable duration.
    MinimumDurationNotMet = 68,
}

#[allow(non_upper_case_globals)]
impl StreamError {
    pub const NextStepNotReached: Self = Self::AmountBelowMinimum;
    pub const InvalidRedirectTarget: Self = Self::InvalidParameter;
    pub const RedirectRecipientMismatch: Self = Self::InvalidParameter;
    pub const DuplicateTokenInDualStream: Self = Self::InvalidParameter;
    pub const IsDualStream: Self = Self::InvalidParameter;
    pub const SenderStreamCapReached: Self = Self::NewSenderStreamCapExceeded;
    pub const InvalidPriority: Self = Self::InvalidParameter;
    pub const StorageVersionMismatch: Self = Self::InvalidParameter;
    pub const BatchSizeTooLarge: Self = Self::BatchLengthMismatch;
    pub const InvalidRecipient: Self = Self::NotRecipient;
    pub const EndTimeInPast: Self = Self::InvalidEndTime;
    pub const InvalidFeeRate: Self = Self::InvalidParameter;
    pub const NotSubscription: Self = Self::InvalidParameter;
    pub const OutsideWithdrawWindow: Self = Self::StreamNotActive;
    pub const RecipientBalanceOverflow: Self = Self::Overflow;
    pub const SenderNotWhitelisted: Self = Self::NotAuthorized;
    pub const StreamAlreadyCancelled: Self = Self::StreamNotActive;
    pub const StreamNotExpired: Self = Self::StreamNotComplete;
    pub const TagTooLong: Self = Self::InvalidParameter;
    pub const TooManyRecipients: Self = Self::InvalidParameter;
    pub const TooManyTags: Self = Self::InvalidParameter;
}
