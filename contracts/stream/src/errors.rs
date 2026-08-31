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
    NextStepNotReached = 48,
    AmountBelowMinimum = 49,
    InvalidExpiryWindow = 50,
    NewSenderStreamCapExceeded = 51,
    InvalidRedirectTarget = 52,
    CircularRedirect = 53,
    RedirectRecipientMismatch = 54,
    /// Dual stream requires both token addresses to be distinct.
    DuplicateTokenInDualStream = 55,
    /// Operation requires a single-token stream but the stream is dual-token.
    IsDualStream = 57,
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
    /// The comment attached to a stream exceeds the 256-byte limit.
    CommentTooLong = 65,
}
