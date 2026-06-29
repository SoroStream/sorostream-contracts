use soroban_sdk::contracterror;

/// Custom errors for the SoroStream contract.
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
    InsufficientBalance = 7,
    InvalidCliff = 8,
    AlreadyInitialized = 9,
    NotInitialized = 10,
    DuplicateStream = 11,
    InvalidStartTime = 12,
    InvalidPartialCancel = 13,
    ContractPaused = 14,
    Overflow = 15,
    ZeroFlowRate = 16,
    BatchLengthMismatch = 17,
    TokenMismatch = 18,
    StreamLocked = 19,
    NotAuthorized = 20,
    StreamNotPaused = 21,
    StreamDurationTooShort = 22,
    StreamIdConflict = 23,
    SenderStreamLimitExceeded = 24,
    InvalidNonce = 25,
    MigrationAlreadyApplied = 26,
    StreamNotSettled = 27,
    WithdrawalCooldownActive = 28,
    RecipientNotWhitelisted = 29,
    MetadataTooLong = 30,
    InvalidEndTime = 31,
    InsufficientXlmForFee = 32,
}
