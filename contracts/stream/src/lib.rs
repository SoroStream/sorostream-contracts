#![no_std]
#![allow(clippy::too_many_arguments)]
// Make `std` available to test modules (host target is not no_std).
#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;
#[cfg(test)]
mod cost_bench;

use errors::StreamError;
use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, Vec};
use storage::{
    check_admin, get_current_stream_id, get_ids_by_recipient, get_ids_by_sender,
    get_protocol_fee, get_treasury, index_by_recipient, index_by_sender, is_paused,
    load_stream, mark_nonce_used, next_stream_id, nonce_used, read_admin, remove_stream, save_stream,
    set_paused, set_protocol_fee, set_treasury, write_admin,
};
use types::{Stats, Stream, StreamStatus};

/// Multiply `flow_rate * elapsed_seconds` with overflow protection.
/// Both operands are user-influenced, so a panic here would be a DoS vector.
#[inline]
fn checked_flow_amount(flow_rate: i128, elapsed_seconds: u64) -> Result<i128, StreamError> {
    flow_rate
        .checked_mul(elapsed_seconds as i128)
        .ok_or(StreamError::Overflow)
}

#[contract]
pub struct SoroStreamContract;

pub fn fanout_create_stream(
    _env: Env,
    _sender: Address,
    _recipients: Vec<Address>,
    _weights: Vec<u32>,
    _token: Address,
    _total_amount: i128,
    _duration_seconds: u64,
    _cliff_seconds: u64,
    _nonce: u64,
    _auto_renew: bool,
) -> Result<Vec<u64>, StreamError> {
    todo!("fanout_create_stream not yet implemented")
}

#[contractimpl]
impl SoroStreamContract {
    /// Initialises the contract by setting the admin address.
    /// Can only be called once; reverts if already initialised.
    pub fn initialize(env: Env, admin: Address) -> Result<(), StreamError> {
        if read_admin(&env).is_some() {
            return Err(StreamError::AlreadyInitialized);
        }
        write_admin(&env, &admin);
        Ok(())
    }

    /// Returns the current admin address. Panics if not initialized.
    pub fn get_admin(env: Env) -> Result<Address, StreamError> {
        read_admin(&env).ok_or(StreamError::NotInitialized)
    }

    /// Transfers the admin role to `new_admin`. Only the current admin may call this.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), StreamError> {
        check_admin(&env);
        write_admin(&env, &new_admin);
        Ok(())
    }

    /// Pauses the contract. Only the admin may call this.
    pub fn pause(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, true);
        Ok(())
    }

    /// Unpauses the contract. Only the admin may call this.
    pub fn unpause(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, false);
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Upgrades the contract WASM bytecode. Only the admin may call this.
    /// All existing storage (streams, indices, counters) is preserved.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), StreamError> {
        let admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Creates a new payment stream locking `amount` tokens for `recipient` over `duration_seconds`.
    ///
    /// # Arguments
    /// * `sender` - The payer who funds the stream.
    /// * `recipient` - The beneficiary of the stream.
    /// * `token` - The SAC token contract address (e.g. USDC).
    /// * `amount` - Total tokens to stream (in stroops).
    /// * `duration_seconds` - Stream duration in seconds.
    /// * `cliff_seconds` - Seconds from start before any tokens are claimable (0 = no cliff).
    /// * `nonce` - Caller-supplied deduplication nonce (unique per sender).
    /// * `auto_renew` - Whether the stream restarts automatically on completion.
    ///
    /// # Returns
    /// The unique stream ID.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if `now + duration_seconds` or
    /// `now + cliff_seconds` overflows `u64`.
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
        if cliff_seconds > duration_seconds {
            return Err(StreamError::InvalidCliff);
        }

        let flow_rate = amount / duration_seconds as i128;
        if flow_rate == 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        mark_nonce_used(&env, &sender, nonce);

        let now = env.ledger().timestamp();
        // Checked: both operands are user-supplied.
        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        let cliff_time = now
            .checked_add(cliff_seconds)
            .ok_or(StreamError::Overflow)?;

        // Division is safe: duration_seconds > 0 is already validated.
        let flow_rate = amount / duration_seconds as i128;
        let stream_id = next_stream_id(&env);

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
            end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew,
        };

        save_stream(&env, &stream);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);

        events::stream_created(
            &env, stream_id, &sender, &recipient, amount, flow_rate, end_time,
        );

        Ok(stream_id)
    }

    /// Allows the recipient to withdraw all tokens earned since last withdrawal.
    ///
    /// If the stream has reached its end time and `auto_renew` is true, the stream
    /// is automatically restarted with a fresh deposit from the sender.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if `flow_rate * elapsed` overflows `i128`,
    /// or if the auto-renew `start_time + duration` overflows `u64`.
    pub fn withdraw(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();
        let effective_now = now.min(stream.end_time);
        let elapsed = if now < stream.cliff_time {
            0u64
        } else {
            effective_now.saturating_sub(stream.last_withdraw_time)
        };
        // Checked: flow_rate * elapsed can overflow with large user-supplied values.
        let claimable = checked_flow_amount(stream.flow_rate, elapsed)?;

        if claimable > 0 {
            token::Client::new(&env, &stream.token).transfer(
                &env.current_contract_address(),
                &recipient,
                &claimable,
            );
        }

        stream.last_withdraw_time = effective_now;

        // Handle natural completion.
        if now >= stream.end_time {
            if stream.auto_renew {
                // end_time > start_time is an invariant maintained on creation.
                let duration = stream.end_time - stream.start_time;
                stream.sender.require_auth();
                token::Client::new(&env, &stream.token).transfer(
                    &stream.sender,
                    &env.current_contract_address(),
                    &stream.deposit,
                );
                // Checked: new end_time could overflow if start_time is near u64::MAX.
                let new_end = stream
                    .end_time
                    .checked_add(duration)
                    .ok_or(StreamError::Overflow)?;
                stream.start_time = stream.end_time;
                stream.end_time = new_end;
                stream.last_withdraw_time = stream.start_time;
            } else {
                events::stream_completed(&env, stream_id);
                remove_stream(&env, stream_id);
            }
        } else {
            save_stream(&env, &stream);
        }
        events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);

        Ok(())
    }

    /// Cancels an active stream. The recipient receives all earned tokens so far;
    /// the sender receives the unstreamed remainder.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if any intermediate multiplication overflows.
    pub fn cancel_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();
        let effective_now = now.min(stream.end_time);

        // elapsed since last withdrawal — used for recipient payout.
        let elapsed_since_withdraw = effective_now.saturating_sub(stream.last_withdraw_time);
        // Checked multiply: flow_rate * elapsed can overflow with extreme inputs.
        let recipient_amount = checked_flow_amount(stream.flow_rate, elapsed_since_withdraw)?;

        // Total streamed from stream start — used to compute sender refund.
        let elapsed_since_start = effective_now.saturating_sub(stream.start_time);
        let total_streamed = checked_flow_amount(stream.flow_rate, elapsed_since_start)?;
        let refund_amount = stream.deposit.saturating_sub(total_streamed);

        let token_client = token::Client::new(&env, &stream.token);

        if recipient_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_amount,
            );
        }
        if refund_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &sender, &refund_amount);
        }

        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);

        events::stream_cancelled(&env, stream_id, &sender, refund_amount, recipient_amount);

        Ok(())
    }

    /// Partially cancels an active stream by reclaiming `cancel_amount` from the unstreamed
    /// remainder. The recipient receives all currently earned tokens. A new stream is created
    /// with the leftover deposit (`remaining - cancel_amount`) at the same flow_rate, and
    /// `cancel_amount` is refunded to the sender. The original stream is marked Cancelled.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if any intermediate multiplication or the new
    /// `end_time` calculation overflows.
    pub fn partial_cancel_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
        cancel_amount: i128,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if cancel_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        let now = env.ledger().timestamp();
        let effective_now = now.min(stream.end_time);

        // Tokens earned since last withdrawal.
        let elapsed_since_withdraw = effective_now.saturating_sub(stream.last_withdraw_time);
        let earned = checked_flow_amount(stream.flow_rate, elapsed_since_withdraw)?;

        // Total streamed from start.
        let elapsed_since_start = effective_now.saturating_sub(stream.start_time);
        let total_streamed = checked_flow_amount(stream.flow_rate, elapsed_since_start)?;

        // Remaining unstreamed deposit.
        let remaining = stream.deposit.saturating_sub(total_streamed);

        // cancel_amount must not exceed the unstreamed remainder, and enough must be left
        // to form at least one second of a new stream.
        if cancel_amount >= remaining || (remaining - cancel_amount) < stream.flow_rate {
            return Err(StreamError::InvalidPartialCancel);
        }

        let new_deposit = remaining - cancel_amount;

        // Division is safe: flow_rate >= 1 (enforced by creation).
        // Cast is checked: new_duration could exceed u64::MAX for tiny flow_rates + huge deposits.
        let new_duration_i128 = new_deposit / stream.flow_rate;
        let new_duration = u64::try_from(new_duration_i128).map_err(|_| StreamError::Overflow)?;
        let new_end_time = now
            .checked_add(new_duration)
            .ok_or(StreamError::Overflow)?;

        let token_client = token::Client::new(&env, &stream.token);

        if earned > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.recipient, &earned);
        }
        token_client.transfer(&env.current_contract_address(), &sender, &cancel_amount);

        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        events::stream_cancelled(&env, stream_id, &sender, cancel_amount, earned);

        let new_stream_id = next_stream_id(&env);
        let new_stream = Stream {
            id: new_stream_id,
            sender: stream.sender.clone(),
            recipient: stream.recipient.clone(),
            token: stream.token.clone(),
            deposit: new_deposit,
            flow_rate: stream.flow_rate,
            start_time: now,
            cliff_time: now,
            end_time: new_end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew: stream.auto_renew,
        };

        save_stream(&env, &new_stream);
        index_by_sender(&env, &sender, new_stream_id);
        index_by_recipient(&env, &stream.recipient, new_stream_id);

        events::stream_partial_cancelled(
            &env,
            stream_id,
            new_stream_id,
            &sender,
            cancel_amount,
            new_deposit,
        );

        Ok(new_stream_id)
    }

    /// Adds more tokens to an existing stream, extending its end time proportionally.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if the new `end_time` or `deposit` overflows.
    pub fn top_up(
        env: Env,
        stream_id: u64,
        sender: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.token != token {
            return Err(StreamError::TokenMismatch);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        // Dust that doesn't map to whole seconds stays with the sender.
        // amount % flow_rate is always < flow_rate ≤ amount, so subtraction is safe.
        let effective_amount = amount - (amount % stream.flow_rate);

        if effective_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        token::Client::new(&env, &stream.token)
            .transfer(&sender, &env.current_contract_address(), &effective_amount);

        // Checked cast: extra_seconds = effective_amount / flow_rate.
        // With a 1-stroop flow_rate and i128::MAX deposit this is ~1.7e38 seconds — overflows u64.
        let extra_seconds_i128 = effective_amount / stream.flow_rate;
        let extra_seconds =
            u64::try_from(extra_seconds_i128).map_err(|_| StreamError::Overflow)?;

        // Checked add: end_time + extra_seconds could overflow u64.
        stream.end_time = stream
            .end_time
            .checked_add(extra_seconds)
            .ok_or(StreamError::Overflow)?;

        // Checked add: deposit + effective_amount could overflow i128.
        stream.deposit = stream
            .deposit
            .checked_add(effective_amount)
            .ok_or(StreamError::Overflow)?;

        let new_end_time = stream.end_time;
        save_stream(&env, &stream);

        events::stream_topped_up(&env, stream_id, effective_amount, new_end_time);

        Ok(())
    }

    /// Returns the full stream struct for a given stream ID.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)
    }

    /// Returns a paginated list of all stream IDs that have ever been created.
    ///
    /// # Arguments
    /// * `start` - Zero-based index of the first stream ID to return.
    /// * `limit` - Maximum number of stream IDs to return (capped at 20).
    pub fn get_all_stream_ids(env: Env, start: u32, limit: u32) -> Vec<u64> {
        let current_id = get_current_stream_id(&env) as usize;
        let cap = limit.min(20) as usize;
        let start = start as usize;
        let end = start.saturating_add(cap).min(current_id);
        let mut ids = Vec::new(&env);

        for stream_id in start..end {
            if load_stream(&env, stream_id as u64).is_some() {
                ids.push_back(stream_id as u64);
            }
        }

        ids
    }

    /// Returns the amount of tokens currently claimable by the recipient.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if `flow_rate * elapsed` overflows `i128`.
    pub fn get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active {
            return Ok(0);
        }

        let now = env.ledger().timestamp();
        if now < stream.cliff_time {
            return Ok(0);
        }

        let effective_now = now.min(stream.end_time);
        let elapsed = effective_now.saturating_sub(stream.last_withdraw_time);
        checked_flow_amount(stream.flow_rate, elapsed)
    }

    /// Returns true if `address` is either the sender or recipient of the given stream.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to check.
    /// * `address` - The address to test for participation.
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

    /// Creates multiple payment streams in a single transaction.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if `now + duration_seconds` overflows `u64`,
    /// or if accumulating `total_amount` overflows `i128`.
    pub fn batch_create_stream(
        env: Env,
        sender: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        duration_seconds: u64,
        auto_renew: bool,
    ) -> Result<Vec<u64>, StreamError> {
        sender.require_auth();

        if recipients.len() != amounts.len() {
            return Err(StreamError::BatchLengthMismatch);
        }

        let now = env.ledger().timestamp();
        let end_time = now.checked_add(duration_seconds).unwrap_or(0);
        if end_time <= now {
            return Err(StreamError::InvalidDuration);
        }

        let mut stream_ids = Vec::new(&env);
        let now = env.ledger().timestamp();
        // Checked: user-supplied duration could push end_time past u64::MAX.
        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;

        // Accumulate total while guarding against i128 overflow from many large amounts.
        let mut total_amount: i128 = 0;
        for amount in amounts.iter() {
            if amount <= 0 {
                return Err(StreamError::ZeroAmount);
            }
            total_amount = total_amount
                .checked_add(amount)
                .ok_or(StreamError::Overflow)?;
        }

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &total_amount,
        );

        for i in 0..recipients.len().min(amounts.len()) {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            // Division safe: duration_seconds > 0 validated above.
            let flow_rate = amount / duration_seconds as i128;
            if flow_rate == 0 {
                return Err(StreamError::ZeroFlowRate);
            }
            let stream_id = next_stream_id(&env);

            let stream = Stream {
                id: stream_id,
                sender: sender.clone(),
                recipient: recipient.clone(),
                token: token.clone(),
                deposit: amount,
                flow_rate,
                start_time: now,
                cliff_time: now,
                end_time,
                last_withdraw_time: now,
                status: StreamStatus::Active,
                auto_renew,
            };

            save_stream(&env, &stream);
            index_by_sender(&env, &sender, stream_id);
            index_by_recipient(&env, &recipient, stream_id);

            stream_ids.push_back(stream_id);
            events::stream_created(
                &env, stream_id, &sender, &recipient, amount, flow_rate, end_time,
            );
        }

        Ok(stream_ids)
    }

    /// Withdraws from multiple streams in a single transaction.
    ///
    /// # Errors
    /// Returns [`StreamError::Overflow`] if any `flow_rate * elapsed` multiplication
    /// overflows, or if the auto-renew `end_time` overflows.
    pub fn batch_withdraw(
        env: Env,
        stream_ids: Vec<u64>,
        recipient: Address,
    ) -> Result<Vec<i128>, StreamError> {
        recipient.require_auth();

        let mut amounts = Vec::new(&env);

        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

            if stream.recipient != recipient {
                return Err(StreamError::NotRecipient);
            }
            if stream.status != StreamStatus::Active {
                return Err(StreamError::StreamNotActive);
            }

            let now = env.ledger().timestamp();
            let effective_now = now.min(stream.end_time);
            let elapsed = effective_now.saturating_sub(stream.last_withdraw_time);
            // Checked: flow_rate * elapsed can overflow with large inputs.
            let claimable = checked_flow_amount(stream.flow_rate, elapsed)?;

            if claimable > 0 {
                let fee_bps = get_protocol_fee(&env);
                // fee_bps ≤ 10_000 (validated in set_protocol_fee), so
                // claimable * fee_bps fits in i128 as long as claimable < i128::MAX / 10_000.
                // claimable ≤ stream.deposit which is at most i128::MAX, so we check.
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
                    }
                }
            }

            stream.last_withdraw_time = effective_now;

            // Handle natural completion.
            if now >= stream.end_time {
                if stream.auto_renew {
                    let duration = stream.end_time - stream.start_time;
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
                    save_stream(&env, &stream);
                } else {
                    events::stream_completed(&env, stream_id);
                    remove_stream(&env, stream_id);
                }
            } else {
                save_stream(&env, &stream);
            }

            amounts.push_back(claimable);
            events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);
        }

        Ok(amounts)
    }

    /// Sets the protocol fee in basis points (100 bps = 1%).
    pub fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError> {
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }
        set_protocol_fee(&env, fee_bps);
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

    /// Returns aggregate contract statistics.
    pub fn get_stats(env: Env) -> Stats {
        let mut total_streams = 0u64;
        let mut active_streams = 0u64;
        let mut total_volume: i128 = 0;

        let current_id = get_current_stream_id(&env);

        for i in 0..current_id {
            if let Some(stream) = load_stream(&env, i) {
                total_streams += 1;
                // Saturating add: total_volume is informational; losing precision is
                // preferable to panicking on a read-only view.
                total_volume = total_volume.saturating_add(stream.deposit);
                if stream.status == StreamStatus::Active {
                    active_streams += 1;
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
