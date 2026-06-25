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
    get_current_stream_id, get_ids_by_recipient, get_ids_by_sender, get_protocol_fee, get_treasury,
    index_by_recipient, index_by_sender, load_stream, next_stream_id, save_stream,
    set_protocol_fee, set_treasury,
};
use types::{Stats, Stream, StreamStatus};

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
    /// * `auto_renew` - Whether the stream restarts automatically on completion.
    ///
    /// # Returns
    /// The unique stream ID.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        auto_renew: bool,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if duration_seconds == 0 {
            return Err(StreamError::InvalidDuration);
        }

        let flow_rate = amount / duration_seconds as i128;
        let now = env.ledger().timestamp();
        let end_time = now + duration_seconds;
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
        let elapsed = effective_now.saturating_sub(stream.last_withdraw_time);
        let claimable = stream.flow_rate * elapsed as i128;

        if claimable > 0 {
            let fee_bps = get_protocol_fee(&env);
            let fee_amount = if fee_bps > 0 { (claimable * fee_bps as i128) / 10_000 } else { 0 };
            let recipient_amount = claimable - fee_amount;

            let token_client = token::Client::new(&env, &stream.token);

            if recipient_amount > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &recipient_amount);
            }

            if fee_amount > 0 {
                if let Some(treasury) = get_treasury(&env) {
                    token_client.transfer(&env.current_contract_address(), &treasury, &fee_amount);
                }
            }
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

    /// Creates multiple payment streams in a single transaction.
    ///
    /// # Arguments
    /// * `sender` - The payer who funds all streams.
    /// * `recipients` - Vector of recipient addresses.
    /// * `amounts` - Vector of amounts (must match recipients length).
    /// * `token` - The token contract address.
    /// * `duration_seconds` - Duration for all streams.
    /// * `auto_renew` - Whether streams auto-renew.
    ///
    /// # Returns
    /// Vector of created stream IDs.
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

        if duration_seconds == 0 {
            return Err(StreamError::InvalidDuration);
        }

        let mut stream_ids = Vec::new(&env);
        let now = env.ledger().timestamp();
        let end_time = now + duration_seconds;

        let mut total_amount: i128 = 0;
        for amount in amounts.iter() {
            if amount <= 0 {
                return Err(StreamError::ZeroAmount);
            }
            total_amount += amount;
        }

        token::Client::new(&env, &token).transfer(&sender, &env.current_contract_address(), &total_amount);

        for i in 0..recipients.len().min(amounts.len()) {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            let flow_rate = amount / duration_seconds as i128;
            let stream_id = next_stream_id(&env);

            let stream = Stream {
                id: stream_id,
                sender: sender.clone(),
                recipient: recipient.clone(),
                token: token.clone(),
                deposit: amount,
                flow_rate,
                start_time: now,
                end_time,
                last_withdraw_time: now,
                status: StreamStatus::Active,
                auto_renew,
            };

            save_stream(&env, &stream);
            index_by_sender(&env, &sender, stream_id);
            index_by_recipient(&env, &recipient, stream_id);

            stream_ids.push_back(stream_id);
            events::stream_created(&env, stream_id, &sender, &recipient, amount, flow_rate, end_time);
        }

        Ok(stream_ids)
    }

    /// Withdraws from multiple streams in a single transaction.
    ///
    /// # Arguments
    /// * `stream_ids` - Vector of stream IDs to withdraw from.
    /// * `recipient` - The recipient address (must be the recipient of all streams).
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
            let claimable = stream.flow_rate * elapsed as i128;

            if claimable > 0 {
                let fee_bps = get_protocol_fee(&env);
                let fee_amount = if fee_bps > 0 { (claimable * fee_bps as i128) / 10_000 } else { 0 };
                let recipient_amount = claimable - fee_amount;

                let token_client = token::Client::new(&env, &stream.token);

                if recipient_amount > 0 {
                    token_client.transfer(&env.current_contract_address(), &recipient, &recipient_amount);
                }

                if fee_amount > 0 {
                    if let Some(treasury) = get_treasury(&env) {
                        token_client.transfer(&env.current_contract_address(), &treasury, &fee_amount);
                    }
                }
            }

            stream.last_withdraw_time = effective_now;

            // Handle natural completion
            if now >= stream.end_time {
                if stream.auto_renew {
                    let duration = stream.end_time - stream.start_time;
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
            amounts.push_back(claimable);
            events::stream_withdrawn(&env, stream_id, &recipient, claimable, now);
        }

        Ok(amounts)
    }

    /// Sets the protocol fee in basis points (100 bps = 1%).
    ///
    /// # Arguments
    /// * `fee_bps` - Fee in basis points (0-10000).
    pub fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError> {
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }
        set_protocol_fee(&env, fee_bps);
        Ok(())
    }

    /// Sets the treasury address to receive protocol fees.
    ///
    /// # Arguments
    /// * `treasury` - The treasury address.
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
                total_volume += stream.deposit;
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
