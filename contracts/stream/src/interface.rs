//! # SoroStream Contract Interface
//!
//! This module defines the formal trait interface for the SoroStream payment streaming contract.
//!
//! ## Purpose
//!
//! `SoroStreamInterface` provides a canonical contract interface that:
//! - Serves as the contract's public API specification
//! - Enables Soroban SDK code generation via `#[contractclient]` for type-safe contract invocation
//! - Allows alternate implementations to implement the same streaming interface
//! - Provides clear documentation of all contract operations
//!
//! ## SDK Code Generation
//!
//! When this trait is decorated with `#[contractclient]`, the Soroban SDK automatically generates:
//! - A strongly-typed client struct for invoking contract functions remotely
//! - Proper argument marshalling and return value deserialization
//! - Inline documentation from trait method doc comments
//!
//! ## Implementing Alternate Contracts
//!
//! Other smart contracts can implement `SoroStreamInterface` to provide streaming functionality
//! compatible with the SoroStream ecosystem. Implementations must:
//! - Maintain the exact function signatures (parameter types, order, and return types)
//! - Enforce the documented error conditions (see each method's Errors section)
//! - Preserve the semantics of stream management, withdrawal, and cancellation logic
//! - Emit corresponding events for auditability
//!
//! Example implementation pattern:
//! ```ignore
//! #[contractimpl]
//! impl MyStreamContract {
//!     // Implement each method from SoroStreamInterface
//! }
//! ```

use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env, String, Vec};

use crate::errors::StreamError;
use crate::types::{AuditEntry, Stats, Stream};

/// Formal interface for SoroStream payment streaming contract.
///
/// Defines all contract operations for creating, managing, and withdrawing from payment streams.
#[contractclient(name = "SoroStreamClient")]
pub trait SoroStreamInterface {
    /// Initializes the contract by setting the admin address.
    ///
    /// Can only be called once; reverts if already initialized.
    ///
    /// # Parameters
    /// * `admin` - The address to designate as contract administrator.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Returns `StreamError::AlreadyInitialized` if the contract has been initialized previously.
    fn initialize(env: Env, admin: Address, version: String) -> Result<(), StreamError>;

    /// Returns the current admin address.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// The `Address` of the current contract administrator.
    ///
    /// # Errors
    /// Returns `StreamError::NotInitialized` if the contract has not been initialized.
    fn get_admin(env: Env) -> Result<Address, StreamError>;

    /// Returns the contract version string.
    fn get_version(env: Env) -> Result<String, StreamError>;

    /// Transfers the admin role to a new address.
    ///
    /// Only the current admin may call this function.
    ///
    /// # Parameters
    /// * `new_admin` - The address to transfer admin privileges to.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Panics if the caller is not the current admin (fails `require_auth()`).
    fn set_admin(env: Env, new_admin: Address) -> Result<(), StreamError>;

    /// Pauses the contract and all stream operations.
    ///
    /// Only the admin may call this function. When paused, no new streams can be created,
    /// and no withdrawals are permitted.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Panics if the caller is not the current admin (fails `require_auth()`).
    fn emergency_pause(env: Env) -> Result<(), StreamError>;

    /// Resumes the contract and allows stream operations to continue.
    ///
    /// Only the admin may call this function. Streams that were active before the pause
    /// resume from where they left off.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Panics if the caller is not the current admin (fails `require_auth()`).
    fn emergency_resume(env: Env) -> Result<(), StreamError>;

    /// Returns whether the contract is currently paused.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// `true` if the contract is paused, `false` if operational.
    fn is_paused(env: Env) -> bool;

    /// Upgrades the contract WASM bytecode.
    ///
    /// Only the admin may call this function. All existing storage (streams, indices, counters)
    /// is preserved across the upgrade.
    ///
    /// # Parameters
    /// * `new_wasm_hash` - A 32-byte hash of the new WASM bytecode to deploy.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Panics if the caller is not the current admin (fails `require_auth()`).
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), StreamError>;

    /// Sets the global maximum streams per sender. Only the admin may call this.
    fn set_max_streams(env: Env, max_streams: u32) -> Result<(), StreamError>;

    /// Sets a per-sender stream limit override. Only the admin may call this.
    fn set_sender_stream_limit(env: Env, sender: Address, limit: u32) -> Result<(), StreamError>;

    /// Creates a new payment stream.
    ///
    /// Locks `amount` tokens for `recipient` over `duration_seconds`. The sender funds the stream
    /// upfront via token transfer. Tokens are released at a constant flow rate.
    ///
    /// # Parameters
    /// * `sender` - The payer who funds the stream (must sign the transaction).
    /// * `recipient` - The beneficiary who receives streamed tokens.
    /// * `token` - The SAC token contract address (e.g., USDC).
    /// * `amount` - Total tokens to stream (in stroops).
    /// * `duration_seconds` - Stream duration in seconds.
    /// * `cliff_seconds` - Seconds from start before any tokens are claimable (0 = no cliff).
    /// * `nonce` - Caller-supplied deduplication nonce (unique per sender).
    /// * `auto_renew` - Whether the stream restarts automatically upon completion.
    /// * `lock_until` - Ledger timestamp before which withdrawals are not permitted.
    /// * `metadata` - Optional metadata bytes (max 64 bytes) attached to the stream.
    ///
    /// # Returns
    /// The unique stream ID (u64) of the newly created stream.
    ///
    /// # Errors
    /// * `StreamError::ContractPaused` if the contract is paused.
    /// * `StreamError::DuplicateStream` if this sender has already used this nonce.
    /// * `StreamError::ZeroAmount` if `amount <= 0`.
    /// * `StreamError::InvalidCliff` if `cliff_seconds > duration_seconds`.
    /// * `StreamError::ZeroFlowRate` if `amount / duration_seconds` rounds down to 0.
    /// * `StreamError::Overflow` if `now + duration_seconds` or `now + cliff_seconds` overflows u64.
    fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        cliff_seconds: u64,
        nonce: u64,
        auto_renew: bool,
        lock_until: u64,
        allow_recipient_termination: bool,
        metadata: Bytes,
    ) -> Result<u64, StreamError>;

    /// Sets the global withdrawal cooldown in seconds.
    fn set_withdrawal_cooldown(env: Env, admin: Address, cooldown_seconds: u64) -> Result<(), StreamError>;

    /// Enables or disables recipient whitelisting.
    fn set_whitelist_enabled(env: Env, admin: Address, enabled: bool) -> Result<(), StreamError>;

    /// Adds a recipient to the whitelist.
    fn add_to_whitelist(env: Env, admin: Address, recipient: Address) -> Result<(), StreamError>;

    /// Removes a recipient from the whitelist.
    fn remove_from_whitelist(env: Env, admin: Address, recipient: Address) -> Result<(), StreamError>;

    /// Updates the metadata blob attached to a stream.
    fn update_metadata(env: Env, sender: Address, stream_id: u64, metadata: Bytes) -> Result<(), StreamError>;

    /// Cancels auto-renewal for an existing stream.
    fn cancel_auto_renew(env: Env, sender: Address, stream_id: u64) -> Result<(), StreamError>;

    /// Allows the recipient to withdraw all earned tokens since last withdrawal.
    ///
    /// If the stream has reached its end time and `auto_renew` is true, the stream is
    /// automatically restarted with a fresh deposit from the sender. A callback to the
    /// recipient's `on_stream_withdraw` method is attempted (failures are silently ignored).
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to withdraw from.
    /// * `recipient` - The beneficiary address (must sign the transaction).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// * `StreamError::ContractPaused` if the contract is paused.
    /// * `StreamError::StreamNotFound` if no stream with this ID exists.
    /// * `StreamError::NotRecipient` if the caller is not the stream recipient.
    /// * `StreamError::StreamNotActive` if the stream is not in Active status.
    /// * `StreamError::StreamLocked` if the current time is before `lock_until`.
    /// * `StreamError::Overflow` if `flow_rate * elapsed` overflows i128, or if auto-renew
    ///   `start_time + duration` overflows u64.
    fn withdraw(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError>;

    /// Cancels an active stream.
    ///
    /// The recipient receives all earned tokens so far; the sender receives the unstreamed
    /// remainder. The stream is marked as Cancelled and removed from indices.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to cancel.
    /// * `sender` - The stream creator (must sign the transaction).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// * `StreamError::StreamNotFound` if no stream with this ID exists.
    /// * `StreamError::NotSender` if the caller is not the stream creator.
    /// * `StreamError::StreamNotActive` if the stream is not in Active status.
    /// * `StreamError::Overflow` if `flow_rate * elapsed` multiplication overflows i128.
    fn cancel_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError>;

    /// Transfers claim rights of a stream to a new recipient.
    ///
    /// Only the current recipient can call this function. Accumulated unwithdrawn tokens
    /// are withdrawn to the current recipient before the transfer.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to transfer.
    /// * `current_recipient` - The current recipient (must sign the transaction).
    /// * `new_recipient` - The new recipient address.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    fn transfer_recipient(env: Env, stream_id: u64, current_recipient: Address, new_recipient: Address) -> Result<(), StreamError>;

    /// Partially cancels a stream by reclaiming tokens from the unstreamed remainder.
    ///
    /// The recipient receives all currently earned tokens. A new stream is created with the
    /// leftover deposit (`remaining - cancel_amount`) at the same flow rate. The original stream
    /// is marked Cancelled, and `cancel_amount` is refunded to the sender.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to partially cancel.
    /// * `sender` - The stream creator (must sign the transaction).
    /// * `cancel_amount` - The amount to reclaim from the unstreamed remainder.
    ///
    /// # Returns
    /// The stream ID of the newly created replacement stream.
    ///
    /// # Errors
    /// * `StreamError::StreamNotFound` if no stream with this ID exists.
    /// * `StreamError::NotSender` if the caller is not the stream creator.
    /// * `StreamError::StreamNotActive` if the stream is not in Active status.
    /// * `StreamError::ZeroAmount` if `cancel_amount <= 0`.
    /// * `StreamError::InvalidPartialCancel` if `cancel_amount >= remaining` or if the
    ///   remainder would be less than one second of flow.
    /// * `StreamError::Overflow` if the new `end_time` calculation or any intermediate
    ///   multiplication overflows.
    fn partial_cancel_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
        cancel_amount: i128,
    ) -> Result<u64, StreamError>;

    /// Adds more tokens to an existing stream, extending its end time proportionally.
    ///
    /// The sender funds the additional tokens upfront. Dust amounts that don't map to whole
    /// seconds (less than one second of flow) are retained by the sender.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to top up.
    /// * `sender` - The stream creator (must sign the transaction).
    /// * `token` - The token address (must match the stream's token).
    /// * `amount` - The amount of additional tokens to add (in stroops).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// * `StreamError::ContractPaused` if the contract is paused.
    /// * `StreamError::StreamNotFound` if no stream with this ID exists.
    /// * `StreamError::NotSender` if the caller is not the stream creator.
    /// * `StreamError::TokenMismatch` if the provided token does not match the stream's token.
    /// * `StreamError::StreamNotActive` if the stream is not in Active status.
    /// * `StreamError::ZeroAmount` if `amount <= 0` or if the effective amount after dust
    ///   deduction is <= 0.
    /// * `StreamError::Overflow` if the new `end_time` or `deposit` calculation overflows.
    fn top_up(
        env: Env,
        stream_id: u64,
        sender: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), StreamError>;

    /// Returns the full stream struct for a given stream ID.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to retrieve.
    ///
    /// # Returns
    /// The `Stream` struct containing all stream metadata and state.
    ///
    /// # Errors
    /// Returns `StreamError::StreamNotFound` if no stream with this ID exists.
    fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError>;

    /// Returns a paginated list of all stream IDs that have ever been created.
    ///
    /// # Parameters
    /// * `start` - Zero-based index of the first stream ID to return.
    /// * `limit` - Maximum number of stream IDs to return (capped at 20).
    ///
    /// # Returns
    /// A vector of stream IDs in the requested range.
    fn get_all_stream_ids(env: Env, start: u32, limit: u32) -> Vec<u64>;

    /// Returns the amount of tokens currently claimable by the recipient.
    ///
    /// Takes into account the cliff period, lock time, and last withdrawal timestamp.
    /// Inactive streams return 0.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to query.
    ///
    /// # Returns
    /// The amount of claimable tokens (in stroops).
    ///
    /// # Errors
    /// * `StreamError::StreamNotFound` if no stream with this ID exists.
    /// * `StreamError::Overflow` if `flow_rate * elapsed` overflows i128.
    fn get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError>;

    /// Returns true if the given address is either the sender or recipient of a stream.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to check.
    /// * `address` - The address to test for participation.
    ///
    /// # Returns
    /// `true` if the address is a participant, `false` otherwise.
    ///
    /// # Errors
    /// Returns `StreamError::StreamNotFound` if no stream with this ID exists.
    fn is_participant(env: Env, stream_id: u64, address: Address) -> Result<bool, StreamError>;

    /// Returns a paginated slice of streams created by a sender address.
    ///
    /// # Parameters
    /// * `sender` - The sender address to filter by.
    /// * `start` - Zero-based index into the sender's streams (capped at 20 per page).
    /// * `limit` - Maximum number of streams to return.
    ///
    /// # Returns
    /// A vector of `Stream` structs created by the sender in the requested range.
    fn get_streams_by_sender(env: Env, sender: Address, start: u32, limit: u32) -> Vec<Stream>;

    /// Returns a paginated slice of streams targeting a recipient address.
    ///
    /// # Parameters
    /// * `recipient` - The recipient address to filter by.
    /// * `start` - Zero-based index into the recipient's streams (capped at 20 per page).
    /// * `limit` - Maximum number of streams to return.
    ///
    /// # Returns
    /// A vector of `Stream` structs targeting the recipient in the requested range.
    fn get_streams_by_recipient(
        env: Env,
        recipient: Address,
        start: u32,
        limit: u32,
    ) -> Vec<Stream>;

    /// Returns only active streams created by a sender address.
    ///
    /// # Parameters
    /// * `sender` - The sender address to filter by.
    ///
    /// # Returns
    /// A vector of active `Stream` structs created by the sender.
    fn get_active_streams_by_sender(env: Env, sender: Address) -> Vec<Stream>;

    /// Returns only active streams targeting a recipient address.
    ///
    /// # Parameters
    /// * `recipient` - The recipient address to filter by.
    ///
    /// # Returns
    /// A vector of active `Stream` structs targeting the recipient.
    fn get_active_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream>;

    /// Pauses an active stream.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to pause.
    /// * `sender` - The sender address.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    fn pause_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError>;

    /// Resumes a paused stream, pushing back the end time.
    ///
    /// # Parameters
    /// * `stream_id` - The ID of the stream to resume.
    /// * `sender` - The sender address.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    fn resume_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError>;

    /// Creates multiple payment streams in a single transaction.
    ///
    /// All streams share the same token, duration, and auto-renew flag. The sender funds all
    /// streams upfront with a single token transfer.
    ///
    /// # Parameters
    /// * `sender` - The payer (must sign the transaction).
    /// * `recipients` - Vector of recipient addresses (length must match `amounts`, `tokens`, and `lock_untils`).
    /// * `amounts` - Vector of amounts per stream (in stroops).
    /// * `tokens` - Vector of token contract addresses for each stream.
    /// * `duration_seconds` - Duration for all streams.
    /// * `auto_renew` - Whether all streams auto-renew on completion.
    /// * `lock_untils` - Vector of lock timestamps per stream.
    /// * `nonce` - Monotonic per-sender replay-protection nonce. Must equal `get_nonce(sender)`.
    ///
    /// # Returns
    /// A vector of stream IDs for the newly created streams.
    ///
    /// # Errors
    /// * `StreamError::InvalidNonce` if `nonce` does not match the stored counter.
    /// * `StreamError::ContractPaused` if the contract is paused.
    /// * `StreamError::BatchLengthMismatch` if vector lengths do not match.
    /// * `StreamError::InvalidDuration` if `duration_seconds` is 0 or overflows.
    /// * `StreamError::ZeroAmount` if any amount is <= 0.
    /// * `StreamError::ZeroFlowRate` if any `amount / duration_seconds` rounds to 0.
    /// * `StreamError::Overflow` if total accumulation or any intermediate calculation overflows.
    fn batch_create_stream(
        env: Env,
        sender: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        tokens: Vec<Address>,
        duration_seconds: u64,
        auto_renew: bool,
        lock_untils: Vec<u64>,
        nonce: u64,
    ) -> Result<Vec<u64>, StreamError>;

    /// Returns the current batch nonce for a sender (next expected value).
    fn get_nonce(env: Env, sender: Address) -> u64;

    /// Withdraws from multiple streams in a single transaction.
    ///
    /// All streams must have the same recipient. Returns an array of amounts withdrawn from
    /// each stream in the same order as the input stream IDs.
    ///
    /// # Parameters
    /// * `stream_ids` - Vector of stream IDs to withdraw from.
    /// * `recipient` - The beneficiary address (must sign the transaction).
    ///
    /// # Returns
    /// A vector of amounts withdrawn from each stream (in stroops).
    ///
    /// # Errors
    /// * `StreamError::ContractPaused` if the contract is paused.
    /// * `StreamError::StreamNotFound` if any stream ID does not exist.
    /// * `StreamError::NotRecipient` if the caller is not the recipient of any stream.
    /// * `StreamError::StreamNotActive` if any stream is not in Active status.
    /// * `StreamError::StreamLocked` if any stream is still locked.
    /// * `StreamError::Overflow` if any `flow_rate * elapsed` multiplication overflows.
    fn batch_withdraw(
        env: Env,
        stream_ids: Vec<u64>,
        recipient: Address,
    ) -> Result<Vec<i128>, StreamError>;

    /// Cancels multiple streams in a single transaction.
    ///
    /// All streams must have the same sender. The sender receives a refund for the
    /// unstreamed portion of each, and the recipient receives the earned portion.
    ///
    /// # Parameters
    /// * `stream_ids` - Vector of stream IDs to cancel (max 20).
    /// * `sender` - The stream creator (must sign the transaction).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// * `StreamError::StreamNotFound` if any stream ID does not exist.
    /// * `StreamError::NotSender` if the caller is not the creator of all streams.
    /// * `StreamError::StreamNotActive` if any stream is not in Active status.
    /// * `StreamError::Overflow` if any intermediate calculation overflows.
    /// * `StreamError::BatchLengthMismatch` if `stream_ids` is empty or exceeds 20.
    fn batch_cancel_stream(env: Env, stream_ids: Vec<u64>, sender: Address) -> Result<Vec<Result<(), StreamError>>, StreamError>;

    /// Sets the protocol fee in basis points (100 bps = 1%).
    ///
    /// Only the admin may call this function. The fee is deducted from withdrawals and
    /// sent to the treasury address.
    ///
    /// # Parameters
    /// * `fee_bps` - Fee in basis points (must be <= 10,000).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Returns `StreamError::InvalidDuration` if `fee_bps > 10_000` (reused error code).
    fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError>;

    /// Proposes a change to the protocol fee, initiating a 7-day timelock.
    ///
    /// Only the admin may call this function.
    ///
    /// # Parameters
    /// * `admin` - The admin address (must sign the transaction).
    /// * `new_fee_bps` - Proposed fee in basis points (must be <= 10,000).
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    fn propose_fee_change(env: Env, admin: Address, new_fee_bps: u32) -> Result<(), StreamError>;

    /// Executes a pending fee proposal if its 7-day timelock has passed.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    fn execute_fee_change(env: Env) -> Result<(), StreamError>;

    /// Sets the treasury address to receive protocol fees.
    ///
    /// Only the admin may call this function.
    ///
    /// # Parameters
    /// * `treasury` - The address to receive accumulated fees.
    ///
    /// # Returns
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    /// This function does not explicitly error; treasury can be any valid address.
    fn set_treasury_address(env: Env, treasury: Address) -> Result<(), StreamError>;

    /// Returns the current protocol fee configuration.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// A tuple `(fee_bps, treasury_address)` where:
    /// - `fee_bps` is the current fee in basis points
    /// - `treasury_address` is `Some(Address)` if configured, `None` otherwise
    fn get_protocol_fee_info(env: Env) -> (u32, Option<Address>);

    /// Returns aggregate contract statistics.
    ///
    /// # Parameters
    /// None (uses environment context).
    ///
    /// # Returns
    /// A `Stats` struct containing:
    /// - `total_streams`: Total number of streams ever created
    /// - `active_streams`: Number of currently active streams
    /// - `total_volume`: Sum of all stream deposits (saturating arithmetic)
    fn get_stats(env: Env) -> Stats;

    /// Returns the minimum stream duration in seconds.
    fn min_duration(env: Env) -> u64;

    /// Allows the recipient to terminate a stream early if `allow_recipient_termination` was set.
    ///
    /// Recipient receives all vested tokens; sender receives the unstreamed remainder.
    fn recipient_terminate(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError>;

    /// Sets the minimum stream duration in seconds.
    /// Only the admin may call this.
    fn set_min_duration(env: Env, admin: Address, seconds: u64);

    /// Runs a one-time migration step after a WASM upgrade. Admin-gated and idempotent.
    fn migrate(env: Env, from_version: String, to_version: String) -> Result<(), StreamError>;

    /// Returns the last 20 admin actions from the circular audit log.
    fn get_admin_log(env: Env) -> Vec<AuditEntry>;

    /// Archives a fully settled stream (total_withdrawn == deposit), deleting its storage entry.
    fn archive_stream(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError>;
}
