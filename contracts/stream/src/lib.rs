#![no_std]
#![allow(clippy::too_many_arguments)]
//! # SoroStream Contract
//!
//! A Soroban smart contract for creating and managing payment streams.
//!
//! The formal interface is defined in [`SoroStreamInterface`].

#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod interface;
mod storage;
mod types;
pub mod vesting_math;

pub use interface::SoroStreamInterface;

pub use errors::StreamError;
pub use types::{Stream, Stats, StreamStatus};

#[cfg(test)]
mod test;
#[cfg(test)]
mod cost_bench;
#[cfg(test)]
mod storage_bench;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod testnet_integration_tests;

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, String, Vec, Symbol, IntoVal};
use storage::{
    check_admin, derive_stream_id, effective_sender_limit, get_global_stream_at,
    get_global_stream_count, get_ids_by_recipient, get_ids_by_sender, get_protocol_fee,
    get_sender_stream_count, get_treasury, index_by_recipient, index_by_sender,
    index_global_stream, is_paused, load_stream, mark_nonce_used, nonce_used, read_admin,
    read_min_duration, read_version, remove_stream, save_stream, set_max_streams_per_sender,
    set_paused, set_protocol_fee, set_sender_limit, set_treasury, stream_exists,
    unindex_by_recipient, unindex_by_sender, write_admin, write_min_duration, write_version,
    set_delegate, get_delegate, remove_delegate, read_pending_fee_proposal, write_pending_fee_proposal, clear_pending_fee_proposal,
};

fn checked_flow_amount(flow_rate: i128, elapsed: u64) -> Result<i128, StreamError> {
    flow_rate.checked_mul(elapsed as i128).ok_or(StreamError::Overflow)
}

#[contract]
pub struct SoroStreamContract;

#[contractimpl]
impl SoroStreamContract {
    /// Initialises the contract by setting the admin address and version.
    /// Can only be called once; reverts if already initialised.
    pub fn initialize(
        env: Env,
        admin: Address,
        version: String,
    ) -> Result<(), StreamError> {
        if read_admin(&env).is_some() {
            return Err(StreamError::AlreadyInitialized);
        }
        write_admin(&env, &admin);
        write_version(&env, &version);
        events::contract_deployed(&env, &version, &admin);
        Ok(())
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Result<Address, StreamError> {
        read_admin(&env).ok_or(StreamError::NotInitialized)
    }

    /// Returns the contract version string.
    pub fn get_version(env: Env) -> Result<String, StreamError> {
        read_version(&env).ok_or(StreamError::NotInitialized)
    }

    /// Transfers the admin role to `new_admin`. Only the current admin may call this.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), StreamError> {
        check_admin(&env);
        write_admin(&env, &new_admin);
        Ok(())
    }

    /// Pauses the contract. Only the admin may call this.
    pub fn emergency_pause(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, true);
        let admin = read_admin(&env).unwrap();
        events::contract_paused(&env, &admin, env.ledger().timestamp());
        Ok(())
    }

    /// Unpauses the contract. Only the admin may call this.
    pub fn emergency_resume(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, false);
        let admin = read_admin(&env).unwrap();
        events::contract_resumed(&env, &admin, env.ledger().timestamp());
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Upgrades the contract WASM bytecode. Only the admin may call this.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), StreamError> {
        let admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Sets the global maximum streams per sender. Only the admin may call this.
    pub fn set_max_streams(env: Env, max_streams: u32) -> Result<(), StreamError> {
        check_admin(&env);
        set_max_streams_per_sender(&env, max_streams);
        Ok(())
    }

    /// Sets a per-sender stream limit override. Only the admin may call this.
    pub fn set_sender_stream_limit(
        env: Env,
        sender: Address,
        limit: u32,
    ) -> Result<(), StreamError> {
        check_admin(&env);
        set_sender_limit(&env, &sender, limit);
        Ok(())
    }

    /// Creates a new payment stream locking `amount` tokens for `recipient` over `duration_seconds`.
    ///
    /// Stream ID is deterministically derived from hash(sender, recipient, start_time, nonce).
    #[allow(clippy::too_many_arguments)]
    pub fn create_stream(
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
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if cliff_seconds >= duration_seconds {
            return Err(StreamError::InvalidCliff);
        }

        let min_dur = read_min_duration(&env);
        if duration_seconds < min_dur {
            return Err(StreamError::StreamDurationTooShort);
        }

        let flow_rate = amount / duration_seconds as i128;
        if flow_rate == 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::SenderStreamLimitExceeded);
        }

        mark_nonce_used(&env, &sender, nonce);

        let now = env.ledger().timestamp();
        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        let cliff_time = now
            .checked_add(cliff_seconds)
            .ok_or(StreamError::Overflow)?;

        let stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);

        if stream_exists(&env, stream_id) {
            return Err(StreamError::StreamIdConflict);
        }

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token,
            deposit: amount,
            flow_rate,
            start_time: now,
            cliff_time,
            lock_until,
            end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew,
            last_pause_time: 0,
            total_withdrawn: 0,
        };

        save_stream(&env, &stream);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        index_global_stream(&env, stream_id);

        events::stream_created(
            &env, stream_id, &sender, &recipient, amount, flow_rate, end_time,
        );

        Ok(stream_id)
    }

    /// Returns the minimum allowed stream duration in seconds.
    pub fn min_duration(env: Env) -> u64 {
        read_min_duration(&env)
    }

    /// Sets the minimum allowed stream duration in seconds.
    /// Only the admin may call this.
    pub fn set_min_duration(env: Env, admin: Address, seconds: u64) {
        admin.require_auth();
        write_min_duration(&env, seconds);
    }

    /// Allows the recipient to withdraw all tokens earned since last withdrawal.
    pub fn withdraw(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };
        if now < stream.lock_until {
            return Err(StreamError::StreamLocked);
        }

        let effective_now = now.min(stream.end_time);
        let claimable = vesting_math::compute_claimable(
            stream.flow_rate, now, stream.cliff_time, stream.end_time, stream.last_withdraw_time,
        ).ok_or(StreamError::Overflow)?;

        if claimable > 0 {
            if stream.total_withdrawn
                .checked_add(claimable)
                .ok_or(StreamError::Overflow)?
                > stream.deposit
            {
                return Err(StreamError::Overflow);
            }

            let fee_bps = get_protocol_fee(&env);
            let fee_amount = if fee_bps > 0 {
                claimable
                    .checked_mul(fee_bps as i128)
                    .ok_or(StreamError::Overflow)?
                    / 10_000
            } else {
                0
            };
            let recipient_amount = claimable - fee_amount;

            let token_client = token::Client::new(&env, &stream.token);

            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipient,
                    &recipient_amount,
                );
            }
            if fee_amount > 0 {
                if let Some(treasury) = get_treasury(&env) {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &treasury,
                        &fee_amount,
                    );
                    events::fee_collected(&env, stream_id, fee_amount, &treasury);
                }
            }

            stream.total_withdrawn = stream
                .total_withdrawn
                .checked_add(claimable)
                .ok_or(StreamError::Overflow)?;

            let _ = env.try_invoke_contract::<(), soroban_sdk::Error>(
                &recipient,
                &Symbol::new(&env, "on_stream_withdraw"),
                (stream_id, recipient_amount).into_val(&env),
            );
        }

        stream.last_withdraw_time = effective_now;

        if now >= stream.end_time {
            // Dust: the deposit may not be evenly divisible by flow_rate, leaving
            // `deposit - flow_rate * duration` stroops that were never streamable.
            // Return any such dust to the sender now that the stream is finalised.
            let duration = stream.end_time - stream.start_time;
            let dust = stream.deposit.saturating_sub(stream.flow_rate.saturating_mul(duration as i128));

            if stream.auto_renew {
                let token_client = token::Client::new(&env, &stream.token);
                let sender_balance = token_client.balance(&stream.sender);
                if sender_balance < stream.deposit {
                    events::auto_renew_failed(&env, stream_id, &stream.sender, stream.deposit);
                    // Return creation-time dust to sender before marking completed.
                    if dust > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &stream.sender,
                            &dust,
                        );
                    }
                    stream.status = StreamStatus::Completed;
                    events::stream_completed(&env, stream_id);
                    save_stream(&env, &stream);
                } else {
                    let duration = stream.end_time - stream.start_time;
                    stream.sender.require_auth();
                    // On renewal the full original deposit is re-locked, so dust is
                    // naturally absorbed into the new cycle — no separate dust refund.
                    token_client.transfer(
                        &stream.sender,
                        &env.current_contract_address(),
                        &stream.deposit,
                    );
                    let new_end = stream
                        .end_time
                        .checked_add(duration)
                        .ok_or(StreamError::Overflow)?;
                    stream.start_time = stream.end_time;
                    stream.end_time = new_end;
                    stream.last_withdraw_time = stream.start_time;
                    stream.total_withdrawn = 0;
                    save_stream(&env, &stream);
                }
            } else {
                // Return creation-time dust to sender before removing the stream.
                if dust > 0 {
                    token::Client::new(&env, &stream.token).transfer(
                        &env.current_contract_address(),
                        &stream.sender,
                        &dust,
                    );
                }
                events::stream_completed(&env, stream_id);
                remove_stream(&env, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);
            }
        } else {
            save_stream(&env, &stream);
        }
        events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);

        Ok(())
    }

    /// Cancels an active stream. The recipient receives all earned tokens so far;
    /// the sender receives the unstreamed remainder.
    pub fn cancel_stream(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        let recipient_amount = vesting_math::compute_earned(
            stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
        ).ok_or(StreamError::Overflow)?;

        // Refund is the full remaining balance for this stream: everything deposited
        // that has not yet been withdrawn and is not owed to the recipient right now.
        // Using `deposit - total_withdrawn - recipient_amount` rather than
        // `deposit - total_streamed` ensures any creation-time dust (deposit %
        // flow_rate) is always returned to the sender and no stroop is lost.
        let refund_amount = stream.deposit
            .saturating_sub(stream.total_withdrawn)
            .saturating_sub(recipient_amount);

        let token_client = token::Client::new(&env, &stream.token);

        if recipient_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_amount,
            );
        }
        if refund_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.sender, &refund_amount);
        }

        remove_stream(&env, stream_id);
        unindex_by_sender(&env, &stream.sender, stream_id);
        unindex_by_recipient(&env, &stream.recipient, stream_id);

        events::stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);

        Ok(())
    }

    /// Transfers claim rights of a stream to a new recipient.
    pub fn transfer_recipient(
        env: Env,
        stream_id: u64,
        current_recipient: Address,
        new_recipient: Address,
    ) -> Result<(), StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        current_recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != current_recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        if now >= stream.lock_until {
            let effective_now = now.min(stream.end_time);
            if now >= stream.cliff_time {
                let claimable = vesting_math::compute_claimable(
                    stream.flow_rate,
                    now,
                    stream.cliff_time,
                    stream.end_time,
                    stream.last_withdraw_time,
                ).ok_or(StreamError::Overflow)?;

                if claimable > 0 {
                    let fee_bps = get_protocol_fee(&env);
                    let fee_amount = if fee_bps > 0 {
                        claimable
                            .checked_mul(fee_bps as i128)
                            .ok_or(StreamError::Overflow)?
                            / 10_000
                    } else {
                        0
                    };
                    let recipient_amount = claimable - fee_amount;

                    let token_client = token::Client::new(&env, &stream.token);

                    if recipient_amount > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &current_recipient,
                            &recipient_amount,
                        );
                    }
                    if fee_amount > 0 {
                        if let Some(treasury) = get_treasury(&env) {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &treasury,
                                &fee_amount,
                            );
                            events::fee_collected(&env, stream_id, fee_amount, &treasury);
                        }
                    }

                    stream.total_withdrawn = stream
                        .total_withdrawn
                        .checked_add(claimable)
                        .ok_or(StreamError::Overflow)?;
                    
                    stream.last_withdraw_time = effective_now;
                    events::stream_withdrawn(&env, stream_id, &current_recipient, claimable, now);
                }
            }
        }

        let old_recipient = stream.recipient.clone();
        stream.recipient = new_recipient.clone();
        save_stream(&env, &stream);

        unindex_by_recipient(&env, &old_recipient, stream_id);
        index_by_recipient(&env, &new_recipient, stream_id);

        events::recipient_transferred(&env, stream_id, &old_recipient, &new_recipient);

        Ok(())
    }

    /// Partially cancels an active stream by reclaiming `cancel_amount` from the unstreamed
    /// remainder.
    pub fn partial_cancel_stream(
        env: Env,
        stream_id: u64,
        caller: Address,
        cancel_amount: i128,
    ) -> Result<u64, StreamError> {
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }
        if cancel_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        let effective_now = now.min(stream.end_time);
        let elapsed_since_withdraw = now.saturating_sub(stream.last_withdraw_time);
        let earned = checked_flow_amount(stream.flow_rate, elapsed_since_withdraw)?;

        let elapsed_since_start = now.saturating_sub(stream.start_time);
        let total_streamed = checked_flow_amount(stream.flow_rate, elapsed_since_start)?;

        let remaining = stream.deposit.saturating_sub(total_streamed);

        if cancel_amount >= remaining || (remaining - cancel_amount) < stream.flow_rate {
            return Err(StreamError::InvalidPartialCancel);
        }

        let new_deposit = remaining - cancel_amount;

        let new_duration_i128 = new_deposit / stream.flow_rate;
        let new_duration = u64::try_from(new_duration_i128).map_err(|_| StreamError::Overflow)?;
        let new_end_time = now
            .checked_add(new_duration)
            .ok_or(StreamError::Overflow)?;

        let token_client = token::Client::new(&env, &stream.token);

        if earned > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.recipient, &earned);
        }
        token_client.transfer(&env.current_contract_address(), &stream.sender, &cancel_amount);

        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        events::stream_cancelled(&env, stream_id, &stream.sender, cancel_amount, earned);

        // Use a derived nonce for the new stream to avoid collisions
        let new_nonce = stream_id;
        let new_stream_id =
            derive_stream_id(&env, &stream.sender, &stream.recipient, now, new_nonce);

        let new_stream = Stream {
            id: new_stream_id,
            sender: stream.sender.clone(),
            recipient: stream.recipient.clone(),
            token: stream.token.clone(),
            deposit: new_deposit,
            flow_rate: stream.flow_rate,
            start_time: now,
            cliff_time: now,
            lock_until: now,
            end_time: new_end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew: stream.auto_renew,
            last_pause_time: 0,
            total_withdrawn: 0,
        };

        save_stream(&env, &new_stream);
        index_by_sender(&env, &stream.sender, new_stream_id);
        index_by_recipient(&env, &stream.recipient, new_stream_id);
        index_global_stream(&env, new_stream_id);

        events::stream_partial_cancelled(
            &env,
            stream_id,
            new_stream_id,
            &stream.sender,
            cancel_amount,
            new_deposit,
        );

        Ok(new_stream_id)
    }

    /// Adds more tokens to an existing stream, extending its end time proportionally.
    pub fn top_up(
        env: Env,
        stream_id: u64,
        caller: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.token != token {
            return Err(StreamError::TokenMismatch);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        let _now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        let effective_amount = amount - (amount % stream.flow_rate);

        if effective_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        token::Client::new(&env, &stream.token)
            .transfer(&caller, &env.current_contract_address(), &effective_amount);

        let extra_seconds_i128 = effective_amount / stream.flow_rate;
        let extra_seconds =
            u64::try_from(extra_seconds_i128).map_err(|_| StreamError::Overflow)?;

        stream.end_time = stream
            .end_time
            .checked_add(extra_seconds)
            .ok_or(StreamError::Overflow)?;

        stream.deposit = stream
            .deposit
            .checked_add(effective_amount)
            .ok_or(StreamError::Overflow)?;

        let new_end_time = stream.end_time;
        save_stream(&env, &stream);

        events::stream_topped_up(&env, stream_id, effective_amount, new_end_time);

        Ok(())
    }

    /// Delegates management of a stream to another address.
    pub fn delegate(env: Env, sender: Address, stream_id: u64, operator: Address) -> Result<(), StreamError> {
        sender.require_auth();
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        set_delegate(&env, stream_id, &operator);
        Ok(())
    }

    /// Revokes management of a stream from the current delegate.
    pub fn revoke_delegate(env: Env, sender: Address, stream_id: u64) -> Result<(), StreamError> {
        sender.require_auth();
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        remove_delegate(&env, stream_id);
        Ok(())
    }

    /// Returns the full stream struct for a given stream ID.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)
    }

    /// Returns a paginated list of all stream IDs that have ever been created.
    pub fn get_all_stream_ids(env: Env, start: u32, limit: u32) -> Vec<u64> {
        let total = get_global_stream_count(&env);
        let cap = limit.min(20);
        let end = start.saturating_add(cap).min(total);
        let mut ids = Vec::new(&env);

        for i in start..end {
            if let Some(id) = get_global_stream_at(&env, i) {
                if load_stream(&env, id).is_some() {
                    ids.push_back(id);
                }
            }
        }

        ids
    }

    /// Returns the amount of tokens currently claimable by the recipient.
    pub fn get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Ok(0);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        if now < stream.cliff_time {
            return Ok(0);
        }

        vesting_math::compute_claimable(
            stream.flow_rate,
            now,
            stream.cliff_time,
            stream.end_time,
            stream.last_withdraw_time,
        )
        .ok_or(StreamError::Overflow)
    }

    /// Returns true if `address` is either the sender or recipient of the given stream.
    pub fn is_participant(env: Env, stream_id: u64, address: Address) -> Result<bool, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        Ok(stream.sender == address || stream.recipient == address)
    }

    /// Returns a paginated slice of streams created by a sender address.
    pub fn get_streams_by_sender(env: Env, sender: Address, start: u32, limit: u32) -> Vec<Stream> {
        let ids = get_ids_by_sender(&env, &sender);
        let cap = limit.min(20) as usize;
        let mut streams = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(ids.len() as usize) {
            if let Some(s) = load_stream(&env, ids.get(i as u32).unwrap()) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Returns a paginated slice of streams targeting a recipient address.
    pub fn get_streams_by_recipient(env: Env, recipient: Address, start: u32, limit: u32) -> Vec<Stream> {
        let ids = get_ids_by_recipient(&env, &recipient);
        let cap = limit.min(20) as usize;
        let mut streams = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(ids.len() as usize) {
            if let Some(s) = load_stream(&env, ids.get(i as u32).unwrap()) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Returns only active streams created by a sender address.
    pub fn get_active_streams_by_sender(env: Env, sender: Address) -> Vec<Stream> {
        let ids = get_ids_by_sender(&env, &sender);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                if s.status == StreamStatus::Active {
                    streams.push_back(s);
                }
            }
        }
        streams
    }

    /// Returns only active streams targeting a recipient address.
    pub fn get_active_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream> {
        let ids = get_ids_by_recipient(&env, &recipient);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                if s.status == StreamStatus::Active {
                    streams.push_back(s);
                }
            }
        }
        streams
    }

    /// Pauses an active stream.
    pub fn pause_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        stream.status = StreamStatus::Paused;
        stream.last_pause_time = env.ledger().timestamp();
        save_stream(&env, &stream);

        events::stream_paused(&env, stream_id, &sender);
        Ok(())
    }

    /// Resumes a paused stream, pushing back the end time.
    pub fn resume_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotPaused);
        }

        let now = env.ledger().timestamp();
        let paused_duration = now.saturating_sub(stream.last_pause_time);

        stream.end_time = stream.end_time.checked_add(paused_duration).unwrap_or(u64::MAX);
        stream.cliff_time = stream.cliff_time.checked_add(paused_duration).unwrap_or(u64::MAX);
        stream.start_time = stream.start_time.checked_add(paused_duration).unwrap_or(u64::MAX);
        stream.last_withdraw_time = stream.last_withdraw_time.checked_add(paused_duration).unwrap_or(u64::MAX);
        stream.lock_until = stream.lock_until.checked_add(paused_duration).unwrap_or(u64::MAX);

        stream.status = StreamStatus::Active;
        stream.last_pause_time = 0;
        save_stream(&env, &stream);

        events::stream_resumed(&env, stream_id, &sender);
        Ok(())
    }

    /// Creates multiple payment streams in a single transaction.
    pub fn batch_create_stream(
        env: Env,
        sender: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        tokens: Vec<Address>,
        duration_seconds: u64,
        auto_renew: bool,
        lock_untils: Vec<u64>,
    ) -> Result<Vec<u64>, StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        if recipients.len() != amounts.len() || recipients.len() != lock_untils.len() || recipients.len() != tokens.len() {
            return Err(StreamError::BatchLengthMismatch);
        }

        let now = env.ledger().timestamp();
        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        if end_time <= now {
            return Err(StreamError::InvalidDuration);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count + recipients.len() > limit {
            return Err(StreamError::SenderStreamLimitExceeded);
        }

        let mut stream_ids = Vec::new(&env);

        for i in 0..recipients.len().min(amounts.len()) {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            let token = tokens.get_unchecked(i);
            
            if amount <= 0 {
                return Err(StreamError::ZeroAmount);
            }

            let flow_rate = amount / duration_seconds as i128;
            if flow_rate == 0 {
                return Err(StreamError::ZeroFlowRate);
            }
            
            token::Client::new(&env, &token).transfer(
                &sender,
                &env.current_contract_address(),
                &amount,
            );
            
            let stream_id = derive_stream_id(&env, &sender, &recipient, now, i as u64);

            let stream = Stream {
                id: stream_id,
                sender: sender.clone(),
                recipient: recipient.clone(),
                token: token.clone(),
                deposit: amount,
                flow_rate,
                start_time: now,
                cliff_time: now,
                lock_until: lock_untils.get_unchecked(i),
                end_time,
                last_withdraw_time: now,
                status: StreamStatus::Active,
                auto_renew,
                last_pause_time: 0,
                total_withdrawn: 0,
            };

            save_stream(&env, &stream);
            index_by_sender(&env, &sender, stream_id);
            index_by_recipient(&env, &recipient, stream_id);
            index_global_stream(&env, stream_id);

            stream_ids.push_back(stream_id);
            events::stream_created(
                &env, stream_id, &sender, &recipient, amount, flow_rate, end_time,
            );
        }

        Ok(stream_ids)
    }

    /// Withdraws from multiple streams in a single transaction.
    pub fn batch_withdraw(
        env: Env,
        stream_ids: Vec<u64>,
        recipient: Address,
    ) -> Result<Vec<i128>, StreamError> {
        if is_paused(&env) {
            return Err(StreamError::ContractPaused);
        }
        recipient.require_auth();

        let mut amounts = Vec::new(&env);

        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

            if stream.recipient != recipient {
                return Err(StreamError::NotRecipient);
            }
            if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
                return Err(StreamError::StreamNotActive);
            }

            let now = if stream.status == StreamStatus::Paused {
                stream.last_pause_time
            } else {
                env.ledger().timestamp()
            };

            if now < stream.lock_until {
                return Err(StreamError::StreamLocked);
            }

            let effective_now = now.min(stream.end_time);
            let claimable = vesting_math::compute_earned(
                stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
            ).ok_or(StreamError::Overflow)?;

            if claimable > 0 {
                if stream.total_withdrawn
                    .checked_add(claimable)
                    .ok_or(StreamError::Overflow)?
                    > stream.deposit
                {
                    return Err(StreamError::Overflow);
                }

                let fee_bps = get_protocol_fee(&env);
                let fee_amount = if fee_bps > 0 {
                    claimable
                        .checked_mul(fee_bps as i128)
                        .ok_or(StreamError::Overflow)?
                        / 10_000
                } else {
                    0
                };
                let recipient_amount = claimable - fee_amount;

                let token_client = token::Client::new(&env, &stream.token);

                if recipient_amount > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &recipient_amount,
                    );
                }
                if fee_amount > 0 {
                    if let Some(treasury) = get_treasury(&env) {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &treasury,
                            &fee_amount,
                        );
                        let _ = env.try_invoke_contract::<(), soroban_sdk::Error>(
                            &treasury,
                            &Symbol::new(&env, "deposit"),
                            (stream.token.clone(), fee_amount).into_val(&env),
                        );
                    }
                }

                stream.total_withdrawn = stream
                    .total_withdrawn
                    .checked_add(claimable)
                    .ok_or(StreamError::Overflow)?;
            }

            stream.last_withdraw_time = effective_now;

            if now >= stream.end_time {
                // Dust: return any non-streamable deposit remainder to the sender.
                let duration = stream.end_time - stream.start_time;
                let dust = stream.deposit.saturating_sub(stream.flow_rate.saturating_mul(duration as i128));

                if stream.auto_renew {
                    // On renewal the full deposit is re-locked, absorbing any dust
                    // into the next cycle — no separate dust refund needed.
                    stream.sender.require_auth();
                    token::Client::new(&env, &stream.token).transfer(
                        &stream.sender,
                        &env.current_contract_address(),
                        &stream.deposit,
                    );
                    let new_end = stream
                        .end_time
                        .checked_add(duration)
                        .ok_or(StreamError::Overflow)?;
                    stream.start_time = stream.end_time;
                    stream.end_time = new_end;
                    stream.last_withdraw_time = stream.start_time;
                    stream.total_withdrawn = 0;
                    save_stream(&env, &stream);
                } else {
                    if dust > 0 {
                        token::Client::new(&env, &stream.token).transfer(
                            &env.current_contract_address(),
                            &stream.sender,
                            &dust,
                        );
                    }
                    events::stream_completed(&env, stream_id);
                    remove_stream(&env, stream_id);
                    unindex_by_sender(&env, &stream.sender, stream_id);
                    unindex_by_recipient(&env, &stream.recipient, stream_id);
                }
            } else {
                save_stream(&env, &stream);
            }

            amounts.push_back(claimable);
            events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);
        }

        Ok(amounts)
    }

    /// Cancels multiple streams in a single transaction, returning per-stream results.
    pub fn batch_cancel_stream(
        env: Env,
        stream_ids: Vec<u64>,
        sender: Address,
    ) -> Result<Vec<Result<(), StreamError>>, StreamError> {
        sender.require_auth();

        if stream_ids.is_empty() || stream_ids.len() > 20 {
            return Err(StreamError::BatchLengthMismatch);
        }

        let mut results = Vec::new(&env);

        for stream_id in stream_ids.iter() {
            let result = (|| {
                let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

                if stream.sender != sender {
                    return Err(StreamError::NotSender);
                }

                if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
                    return Err(StreamError::StreamNotActive);
                }

                let now = env.ledger().timestamp();
                let recipient_amount = vesting_math::compute_earned(
                    stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
                ).ok_or(StreamError::Overflow)?;

                // Use balance-exact formula: refund everything that isn't owed to the
                // recipient right now and hasn't already been withdrawn, so no stroop
                // is left behind.
                let refund_amount = stream.deposit
                    .saturating_sub(stream.total_withdrawn)
                    .saturating_sub(recipient_amount);

                let token_client = token::Client::new(&env, &stream.token);

                if recipient_amount > 0 {
                    token_client.transfer(&env.current_contract_address(), &stream.recipient, &recipient_amount);
                }
                if refund_amount > 0 {
                    token_client.transfer(&env.current_contract_address(), &stream.sender, &refund_amount);
                }

                remove_stream(&env, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);
                events::stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);
                Ok(())
            })();
            results.push_back(result);
        }

        Ok(results)
    }

    /// Sets the protocol fee in basis points (100 bps = 1%).
    pub fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError> {
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration); // Reuse error code for now
        }
        set_protocol_fee(&env, fee_bps);
        Ok(())
    }

    pub fn propose_fee_change(env: Env, admin: Address, new_fee_bps: u32) -> Result<(), StreamError> {
        admin.require_auth();
        let current_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != current_admin {
            return Err(StreamError::NotAuthorized);
        }
        if new_fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration); // Reuse error code for now
        }

        let now = env.ledger().timestamp();
        let unlock_time = now.checked_add(7 * 24 * 60 * 60).unwrap_or(u64::MAX);

        write_pending_fee_proposal(&env, new_fee_bps, unlock_time);
        events::fee_change_proposed(&env, new_fee_bps, unlock_time);
        Ok(())
    }

    pub fn execute_fee_change(env: Env) -> Result<(), StreamError> {
        let (new_fee_bps, unlock_time) = read_pending_fee_proposal(&env).ok_or(StreamError::NotAuthorized)?; // Or some other appropriate error

        let now = env.ledger().timestamp();
        if now < unlock_time {
            return Err(StreamError::StreamLocked); // Reuse StreamLocked error code for timelock
        }

        set_protocol_fee(&env, new_fee_bps);
        clear_pending_fee_proposal(&env);
        events::fee_change_executed(&env, new_fee_bps);

        Ok(())
    }

    /// Sets the treasury address to receive protocol fees.
    pub fn set_treasury_address(env: Env, treasury: Address) -> Result<(), StreamError> {
        set_treasury(&env, &treasury);
        Ok(())
    }

    /// Returns protocol fee configuration.
    pub fn get_protocol_fee_info(env: Env) -> (u32, Option<Address>) {
        (get_protocol_fee(&env), get_treasury(&env))
    }

    /// Withdraws accumulated protocol fees from the treasury contract.
    pub fn withdraw_treasury(
        env: Env,
        token: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), StreamError> {
        check_admin(&env);
        let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
        env.invoke_contract::<()>(
            &treasury,
            &Symbol::new(&env, "withdraw_treasury"),
            (token, amount, destination).into_val(&env),
        );
        Ok(())
    }

    /// Withdraws all accumulated protocol fees for a token from the treasury contract.
    pub fn withdraw_all_from_treasury(
        env: Env,
        token: Address,
        destination: Address,
    ) -> Result<i128, StreamError> {
        check_admin(&env);
        let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
        let result = env.invoke_contract::<i128>(
            &treasury,
            &Symbol::new(&env, "withdraw_all"),
            (token, destination).into_val(&env),
        );
        Ok(result)
    }

    /// Returns aggregate contract statistics.
    pub fn get_stats(env: Env) -> Stats {
        let mut total_streams = 0u64;
        let mut active_streams = 0u64;
        let mut total_volume: i128 = 0;

        let count = get_global_stream_count(&env);

        for i in 0..count {
            if let Some(stream_id) = get_global_stream_at(&env, i) {
                if let Some(stream) = load_stream(&env, stream_id) {
                    total_streams += 1;
                    total_volume = total_volume.saturating_add(stream.deposit);
                    if stream.status == StreamStatus::Active {
                        active_streams += 1;
                    }
                }
            }
        }

        Stats {
            total_streams,
            active_streams,
            total_volume,
        }
    }
}

impl SoroStreamInterface for SoroStreamContract {
    fn initialize(env: Env, admin: Address, version: String) -> Result<(), StreamError> {
        Self::initialize(env, admin, version)
    }

    fn get_admin(env: Env) -> Result<Address, StreamError> {
        Self::get_admin(env)
    }

    fn get_version(env: Env) -> Result<String, StreamError> {
        Self::get_version(env)
    }

    fn set_admin(env: Env, new_admin: Address) -> Result<(), StreamError> {
        Self::set_admin(env, new_admin)
    }

    fn emergency_pause(env: Env) -> Result<(), StreamError> {
        Self::emergency_pause(env)
    }

    fn emergency_resume(env: Env) -> Result<(), StreamError> {
        Self::emergency_resume(env)
    }

    fn is_paused(env: Env) -> bool {
        Self::is_paused(env)
    }

    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), StreamError> {
        Self::upgrade(env, new_wasm_hash)
    }

    fn set_max_streams(env: Env, max_streams: u32) -> Result<(), StreamError> {
        Self::set_max_streams(env, max_streams)
    }

    fn set_sender_stream_limit(env: Env, sender: Address, limit: u32) -> Result<(), StreamError> {
        Self::set_sender_stream_limit(env, sender, limit)
    }

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
    ) -> Result<u64, StreamError> {
        Self::create_stream(
            env,
            sender,
            recipient,
            token,
            amount,
            duration_seconds,
            cliff_seconds,
            nonce,
            auto_renew,
            lock_until,
        )
    }

    fn withdraw(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        Self::withdraw(env, stream_id, recipient)
    }

    fn cancel_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        Self::cancel_stream(env, stream_id, sender)
    }

    fn partial_cancel_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
        cancel_amount: i128,
    ) -> Result<u64, StreamError> {
        Self::partial_cancel_stream(env, stream_id, sender, cancel_amount)
    }

    fn top_up(
        env: Env,
        stream_id: u64,
        sender: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), StreamError> {
        Self::top_up(env, stream_id, sender, token, amount)
    }

    fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        Self::get_stream(env, stream_id)
    }

    fn get_all_stream_ids(env: Env, start: u32, limit: u32) -> Vec<u64> {
        Self::get_all_stream_ids(env, start, limit)
    }

    fn get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        Self::get_claimable(env, stream_id)
    }

    fn is_participant(env: Env, stream_id: u64, address: Address) -> Result<bool, StreamError> {
        Self::is_participant(env, stream_id, address)
    }

    fn get_streams_by_sender(env: Env, sender: Address, start: u32, limit: u32) -> Vec<Stream> {
        Self::get_streams_by_sender(env, sender, start, limit)
    }

    fn get_streams_by_recipient(
        env: Env,
        recipient: Address,
        start: u32,
        limit: u32,
    ) -> Vec<Stream> {
        Self::get_streams_by_recipient(env, recipient, start, limit)
    }

    fn get_active_streams_by_sender(env: Env, sender: Address) -> Vec<Stream> {
        Self::get_active_streams_by_sender(env, sender)
    }

    fn get_active_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream> {
        Self::get_active_streams_by_recipient(env, recipient)
    }

    fn batch_create_stream(
        env: Env,
        sender: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        tokens: Vec<Address>,
        duration_seconds: u64,
        auto_renew: bool,
        lock_untils: Vec<u64>,
    ) -> Result<Vec<u64>, StreamError> {
        Self::batch_create_stream(
            env,
            sender,
            recipients,
            amounts,
            tokens,
            duration_seconds,
            auto_renew,
            lock_untils,
        )
    }

    fn batch_withdraw(
        env: Env,
        stream_ids: Vec<u64>,
        recipient: Address,
    ) -> Result<Vec<i128>, StreamError> {
        Self::batch_withdraw(env, stream_ids, recipient)
    }

    fn batch_cancel_stream(env: Env, stream_ids: Vec<u64>, sender: Address) -> Result<Vec<Result<(), StreamError>>, StreamError> {
        Self::batch_cancel_stream(env, stream_ids, sender)
    }

    fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError> {
        Self::set_protocol_fee(env, fee_bps)
    }

    fn set_treasury_address(env: Env, treasury: Address) -> Result<(), StreamError> {
        Self::set_treasury_address(env, treasury)
    }

    fn get_protocol_fee_info(env: Env) -> (u32, Option<Address>) {
        Self::get_protocol_fee_info(env)
    }

    fn get_stats(env: Env) -> Stats {
        Self::get_stats(env)
    }

    fn min_duration(env: Env) -> u64 {
        Self::min_duration(env)
    }

    fn set_min_duration(env: Env, admin: Address, seconds: u64) {
        Self::set_min_duration(env, admin, seconds)
    }

    fn pause_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        Self::pause_stream(env, stream_id, sender)
    }

    fn resume_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        Self::resume_stream(env, stream_id, sender)
    }

    fn transfer_recipient(env: Env, stream_id: u64, current_recipient: Address, new_recipient: Address) -> Result<(), StreamError> {
        Self::transfer_recipient(env, stream_id, current_recipient, new_recipient)
    }

    fn propose_fee_change(env: Env, admin: Address, new_fee_bps: u32) -> Result<(), StreamError> {
        Self::propose_fee_change(env, admin, new_fee_bps)
    }

    fn execute_fee_change(env: Env) -> Result<(), StreamError> {
        Self::execute_fee_change(env)
    }
}
