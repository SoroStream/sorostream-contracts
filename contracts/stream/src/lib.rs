#![no_std]

mod errors;
mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use errors::StreamError;
use soroban_sdk::{
    contract, contractimpl, token, Address, Env, Vec,
};
use storage::{
    get_ids_by_recipient, get_ids_by_sender, index_by_recipient, index_by_sender,
    load_stream, next_stream_id, save_stream,
};
use types::{Stream, StreamStatus};

#[contract]
pub struct SoroStreamContract;

#[contractimpl]
impl SoroStreamContract {
    /// Creates a new payment stream locking `amount` tokens for `recipient` over `duration_seconds`.
    ///
    /// # Arguments
    /// * `sender` - The payer who funds the stream.
    /// * `recipient` - The beneficiary of the stream.
    /// * `token` - The SAC token contract address (e.g. USDC).
    /// * `amount` - Total tokens to stream (in stroops).
    /// * `duration_seconds` - Stream duration in seconds.
    /// * `cliff_seconds` - Seconds from start before any tokens are claimable (0 = no cliff).
    /// * `auto_renew` - Whether the stream restarts automatically on completion.
    ///
    /// # Returns
    /// The unique stream ID.
    #[allow(clippy::too_many_arguments)]
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        cliff_seconds: u64,
        auto_renew: bool,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if duration_seconds == 0 {
            return Err(StreamError::InvalidDuration);
        }
        if cliff_seconds > duration_seconds {
            return Err(StreamError::InvalidCliff);
        }

        let flow_rate = amount / duration_seconds as i128;
        let now = env.ledger().timestamp();
        let end_time = now + duration_seconds;
        let cliff_time = now + cliff_seconds;
        let stream_id = next_stream_id(&env);

        token::Client::new(&env, &token).transfer(&sender, &env.current_contract_address(), &amount);

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

        events::stream_created(&env, stream_id, &sender, &recipient, amount, flow_rate, end_time);

        Ok(stream_id)
    }

    /// Allows the recipient to withdraw all tokens earned since last withdrawal.
    ///
    /// If the stream has reached its end time and `auto_renew` is true, the stream
    /// is automatically restarted.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to withdraw from.
    /// * `recipient` - Must match the stream's recipient (auth required).
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
            0
        } else {
            effective_now.saturating_sub(stream.last_withdraw_time)
        };
        let claimable = stream.flow_rate * elapsed as i128;

        if claimable > 0 {
            token::Client::new(&env, &stream.token)
                .transfer(&env.current_contract_address(), &recipient, &claimable);
        }

        stream.last_withdraw_time = effective_now;

        // Handle natural completion
        if now >= stream.end_time {
            if stream.auto_renew {
                let duration = stream.end_time - stream.start_time;
                // Pull fresh deposit from sender for the new cycle
                stream.sender.require_auth();
                token::Client::new(&env, &stream.token).transfer(
                    &stream.sender,
                    &env.current_contract_address(),
                    &stream.deposit,
                );
                stream.start_time = stream.end_time;
                stream.end_time = stream.start_time + duration;
                stream.last_withdraw_time = stream.start_time;
            } else {
                stream.status = StreamStatus::Completed;
                events::stream_completed(&env, stream_id);
            }
        }

        save_stream(&env, &stream);
        events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);

        Ok(())
    }

    /// Cancels an active stream. The recipient receives all earned tokens so far;
    /// the sender receives the unstreamed remainder.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to cancel.
    /// * `sender` - Must match the stream's sender (auth required).
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
        let elapsed = effective_now.saturating_sub(stream.last_withdraw_time);
        let recipient_amount = stream.flow_rate * elapsed as i128;
        let refund_amount = stream.deposit.saturating_sub(
            stream.flow_rate * effective_now.saturating_sub(stream.start_time) as i128,
        );

        let token_client = token::Client::new(&env, &stream.token);

        if recipient_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.recipient, &recipient_amount);
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
    /// # Arguments
    /// * `stream_id` - The stream to partially cancel.
    /// * `sender` - Must match the stream's sender (auth required).
    /// * `cancel_amount` - Tokens to reclaim from the unstreamed balance.
    ///
    /// # Returns
    /// The new stream ID carrying the leftover deposit.
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

        // Tokens already earned by the recipient (since last withdrawal).
        let elapsed = effective_now.saturating_sub(stream.last_withdraw_time);
        let earned = stream.flow_rate * elapsed as i128;

        // Total streamed so far from start (already paid out in previous withdrawals + earned now).
        let total_streamed =
            stream.flow_rate * effective_now.saturating_sub(stream.start_time) as i128;

        // Remaining unstreamed deposit.
        let remaining = stream.deposit.saturating_sub(total_streamed);

        // cancel_amount must not exceed the unstreamed remainder, and enough must be left
        // to form at least one second of a new stream.
        if cancel_amount >= remaining || (remaining - cancel_amount) < stream.flow_rate {
            return Err(StreamError::InvalidPartialCancel);
        }

        let new_deposit = remaining - cancel_amount;

        let token_client = token::Client::new(&env, &stream.token);

        // Pay recipient their earned tokens.
        if earned > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.recipient, &earned);
        }

        // Refund cancel_amount to sender.
        token_client.transfer(&env.current_contract_address(), &sender, &cancel_amount);

        // Mark original stream as cancelled.
        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        events::stream_cancelled(&env, stream_id, &sender, cancel_amount, earned);

        // Create a new stream with new_deposit at the same flow_rate.
        let new_duration = (new_deposit / stream.flow_rate) as u64;
        let new_end_time = now + new_duration;
        let new_stream_id = next_stream_id(&env);

        let new_stream = Stream {
            id: new_stream_id,
            sender: stream.sender.clone(),
            recipient: stream.recipient.clone(),
            token: stream.token.clone(),
            deposit: new_deposit,
            flow_rate: stream.flow_rate,
            start_time: now,
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
    /// # Arguments
    /// * `stream_id` - The stream to top up.
    /// * `sender` - Must match the stream's sender (auth required).
    /// * `amount` - Additional tokens to add (in stroops).
    pub fn top_up(env: Env, stream_id: u64, sender: Address, amount: i128) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        token::Client::new(&env, &stream.token)
            .transfer(&sender, &env.current_contract_address(), &amount);

        let extra_seconds = (amount / stream.flow_rate) as u64;
        stream.end_time += extra_seconds;
        stream.deposit += amount;

        let new_end_time = stream.end_time;
        save_stream(&env, &stream);

        events::stream_topped_up(&env, stream_id, amount, new_end_time);

        Ok(())
    }

    /// Returns the full stream struct for a given stream ID.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to look up.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)
    }

    /// Returns the amount of tokens currently claimable by the recipient.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to check.
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
        Ok(stream.flow_rate * elapsed as i128)
    }

    /// Returns all streams created by a sender address.
    ///
    /// # Arguments
    /// * `sender` - The sender address to query.
    pub fn get_streams_by_sender(env: Env, sender: Address) -> Vec<Stream> {
        let ids = get_ids_by_sender(&env, &sender);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Returns all streams targeting a recipient address.
    ///
    /// # Arguments
    /// * `recipient` - The recipient address to query.
    pub fn get_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream> {
        let ids = get_ids_by_recipient(&env, &recipient);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                streams.push_back(s);
            }
        }
        streams
    }
}
