//! # SoroStream External Composability Interface (issue #291)
//!
//! Provides a standardised, minimal cross-contract client that external
//! Soroban contracts (lending protocols, DAOs, vesting managers, etc.)
//! can embed to programmatically create, query, and interact with
//! SoroStream payment streams.
//!
//! ## Usage
//!
//! An external contract imports and instantiates [`SoroStreamComposabilityClient`]:
//!
//! ```rust,ignore
//! use sorostream_stream::composability::SoroStreamComposabilityClient;
//!
//! let client = SoroStreamComposabilityClient::new(&env, &sorostream_contract_id);
//!
//! // Create a stream on behalf of a DAO grant
//! let stream_id = client.create_stream(
//!     &env.current_contract_address(), // sender = the calling contract
//!     &recipient,
//!     &usdc_token,
//!     &1_000_000_000i128,              // 1 000 USDC (7 decimals)
//!     &(30 * 24 * 60 * 60u64),         // 30 days
//!     &0u64,                           // no cliff
//!     &0u64,                           // nonce = 0
//!     &false,                          // no auto-renew
//!     &None::<u32>,                    // unlimited renewals
//!     &0u64,                           // no lock
//!     &false,                          // recipient cannot terminate
//!     &false,                          // transferable
//! )?;
//!
//! // Query current claimable balance
//! let claimable = client.get_claimable(&stream_id)?;
//! ```
//!
//! ## Design Decisions
//!
//! * The trait exposes **only the minimal surface** needed for composability — full
//!   admin and one-off operations remain on the main `SoroStreamInterface` trait.
//! * All functions return `Result<_, StreamError>` so callers can handle
//!   failures without panicking.
//! * The `#[contractclient]` attribute makes the SDK generate a type-safe,
//!   cross-contract-call stub automatically.  No additional glue code is
//!   required by the calling contract.

use soroban_sdk::{contractclient, Address, Env, Vec};

use crate::errors::StreamError;
use crate::types::Stream;

/// Minimal composability interface for external Soroban contracts.
///
/// External contracts that want to integrate with SoroStream should use the
/// auto-generated [`SoroStreamComposabilityClient`] stub (produced by the
/// `#[contractclient]` attribute below) rather than calling the full
/// [`SoroStreamInterface`] directly.  This keeps the integration surface
/// small, stable, and independently auditable.
#[contractclient(name = "SoroStreamComposabilityClient")]
pub trait ISoroStreamComposability {
    // ── Stream lifecycle ──────────────────────────────────────────────────

    /// Creates a new payment stream and returns its unique `stream_id`.
    ///
    /// This is a subset of the full `create_stream` instruction exposing only
    /// the parameters relevant to programmatic integrations.  The calling
    /// contract must have pre-approved the token transfer (SAC `approve` call)
    /// before invoking this function.
    ///
    /// # Parameters
    /// * `sender`     — the address that funds the stream (usually the calling contract).
    /// * `recipient`  — the address that will receive the streamed tokens.
    /// * `token`      — SAC-compatible token contract address.
    /// * `amount`     — total tokens to lock (in stroops / smallest denomination).
    /// * `duration_seconds` — stream duration in seconds (must be > 0).
    /// * `cliff_seconds`    — seconds after start before any tokens can be claimed
    ///                         (0 = no cliff, must be ≤ `duration_seconds`).
    /// * `nonce`      — caller-supplied nonce for replay protection.
    /// * `auto_renew` — whether to restart the stream automatically on completion.
    /// * `renew_count` — optional maximum number of automatic renewals (`None` = unlimited).
    /// * `lock_until` — timestamp before which the recipient cannot withdraw.
    /// * `allow_recipient_termination` — whether the recipient may stop the stream early.
    /// * `non_transferable` — whether the recipient rights are permanently locked.
    ///
    /// # Errors
    /// Returns a [`StreamError`] on validation failure, insufficient sender balance,
    /// rate-limit breach, or any other protocol invariant violation.
    #[allow(clippy::too_many_arguments)]
    fn composable_create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        auto_renew: bool,
        params: crate::types::CreateStreamParams,
    ) -> Result<u64, StreamError>;

    /// Triggers a withdrawal for `recipient` on `stream_id`.
    ///
    /// External contracts can use this to atomically settle a stream as part
    /// of a larger transaction (e.g., a lending protocol that needs the
    /// stream recipient to have received payment before releasing collateral).
    ///
    /// # Errors
    /// - [`StreamError::StreamNotFound`]  — no stream with that ID.
    /// - [`StreamError::NotRecipient`]    — caller is not the stream recipient.
    /// - [`StreamError::StreamNotActive`] — stream is not in Active state.
    /// - [`StreamError::ZeroAmount`]      — nothing claimable at this time.
    fn composable_withdraw(
        env: Env,
        stream_id: u64,
        recipient: Address,
    ) -> Result<(), StreamError>;

    /// Returns the amount of tokens currently claimable by the recipient.
    ///
    /// This is a read-only operation — safe to call from any context.
    fn composable_get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError>;

    // ── Stream queries ────────────────────────────────────────────────────

    /// Returns the full [`Stream`] struct for `stream_id`.
    ///
    /// Callers can inspect `stream.status`, `stream.flow_rate`, `stream.deposit`,
    /// and other fields to make conditional decisions.
    fn composable_get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError>;

    /// Returns all active stream IDs where `address` is the sender or recipient.
    ///
    /// Intended for on-chain aggregation (e.g., a DAO querying outstanding grants
    /// before a governance vote).  Returns an empty `Vec` if there are none.
    fn composable_get_streams_for(
        env: Env,
        address: Address,
        as_sender: bool,
        start: u32,
        limit: u32,
    ) -> Vec<Stream>;

    /// Returns whether `stream_id` currently has a claimable balance above zero.
    ///
    /// Useful for on-chain conditional logic (e.g., "only proceed if the grant
    /// stream has vested at least some tokens").
    fn composable_has_claimable(env: Env, stream_id: u64) -> bool;

    // ── Protocol metadata ─────────────────────────────────────────────────

    /// Returns the SoroStream contract version string.
    ///
    /// External integrations can gate on version to handle future upgrades.
    fn composable_get_version(env: Env) -> Result<soroban_sdk::String, StreamError>;
}
