use soroban_sdk::contracterror;

/// Custom errors for the SoroStream contract.
///
/// Hard-capped at 50 variants — Soroban's `#[contracterror]` macro encodes
/// this enum's cases into `ScSpecUdtErrorEnumV0.cases`, a
/// `VecM<ScSpecUdtErrorEnumCaseV0, 50>` (see `stellar-xdr`). Exceeding 50
/// makes the `#[contracterror]` macro itself panic at compile time with
/// `LengthExceedsMax`, failing the whole crate's build — not a soft lint.
/// Before adding a new variant here, either free a slot by folding a
/// low-traffic existing one into a semantically close neighbor (see the
/// consolidation below — a prerequisite fix for #402/#617/#620/#661, none
/// of which could be verified while this enum already exceeded 50), or
/// confirm the count is still under 50 with
/// `grep -cE '^\s+\w+ = [0-9]+,?$' errors.rs`.
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
    /// The temporary stream metadata blob exceeds 256 bytes.
    MetadataTooLong = 30,
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
    /// Also returned when a withdrawal-step-gated stream's next unclaimed
    /// step boundary has not yet been reached (prerequisite 50-variant-cap
    /// cleanup: folded in the former, separate `NextStepNotReached`).
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
    /// A parameter decoded from XDR but is semantically invalid for this entry
    /// point — for example an empty or over-long identifier string, or an
    /// operation that requires a single-token stream called on a dual-token
    /// one (prerequisite 50-variant-cap cleanup: folded in the former,
    /// separate `IsDualStream`).
    InvalidParameter = 67,
    /// `create_stream` was called with `duration_seconds == 0` (i.e.
    /// `end_time == start_time`), which leaves claimable-amount accrual
    /// undefined. A stream must span at least one ledger.
    MinimumDurationNotMet = 68,
}
