//! SoroStream — a Soroban streaming payment contract.
//!
//! Features
//! ─────────
//! • create_stream / claim / cancel — core payment stream lifecycle
//! • renew — pull-based auto-renewal (fix for issue #8)
//! • get_reputation / (internal) increment_reputation — reputation scoring
//! • propose_stream / approve_proposal / discard_proposal — 2-of-2 multisig flow

#![no_std]

mod storage;
mod types;

pub use types::{DataKey, Stream, StreamProposal};

use soroban_sdk::{contract, contractimpl, token, Address, Env};

#[contract]
pub struct StreamingContract;

#[contractimpl]
impl StreamingContract {
    // ─────────────────────────────────────────────────────────────────────────
    //  Core stream lifecycle
    // ─────────────────────────────────────────────────────────────────────────

    /// Create a new payment stream.
    ///
    /// `deposit` must be exactly divisible by `(end_time - start_time)` so that
    /// `rate_per_second` is a whole integer.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        start_time: u64,
        end_time: u64,
        auto_renew: bool,
    ) -> u64 {
        sender.require_auth();

        assert!(end_time > start_time, "end_time must be after start_time");
        assert!(deposit > 0, "deposit must be positive");

        let duration = (end_time - start_time) as i128;
        assert!(
            deposit % duration == 0,
            "deposit must be exactly divisible by duration"
        );

        let rate_per_second = deposit / duration;

        // Pull deposit from sender into the contract.
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        let stream_id = storage::next_stream_id(&env);
        storage::save_stream(
            &env,
            stream_id,
            &Stream {
                sender,
                recipient,
                token,
                deposit,
                claimed: 0,
                rate_per_second,
                start_time,
                end_time,
                auto_renew,
                cancelled: false,
            },
        );
        stream_id
    }

    /// Claim all vested tokens up to the current timestamp.
    pub fn claim(env: Env, stream_id: u64) -> i128 {
        let mut stream = storage::get_stream(&env, stream_id);
        stream.recipient.require_auth();

        assert!(!stream.cancelled, "stream is cancelled");

        let claimable = Self::_claimable(&env, &stream);
        assert!(claimable > 0, "nothing to claim");

        stream.claimed += claimable;
        storage::save_stream(&env, stream_id, &stream);

        token::Client::new(&env, &stream.token).transfer(
            &env.current_contract_address(),
            &stream.recipient,
            &claimable,
        );
        claimable
    }

    /// Cancel a stream. Vested tokens go to recipient; remainder goes back to sender.
    pub fn cancel(env: Env, stream_id: u64) {
        let mut stream = storage::get_stream(&env, stream_id);
        stream.sender.require_auth();

        assert!(!stream.cancelled, "stream already cancelled");

        let claimable = Self::_claimable(&env, &stream);
        let refund = stream.deposit - stream.claimed - claimable;

        stream.cancelled = true;
        storage::save_stream(&env, stream_id, &stream);

        let token_client = token::Client::new(&env, &stream.token);
        if claimable > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &claimable,
            );
        }
        if refund > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.sender, &refund);
        }
        // Cancelled streams do NOT increment reputation.
    }

    /// Pull-based auto-renewal (issue #8 fix).
    ///
    /// Must be called at or after `end_time`. The renewed stream's `start_time`
    /// is exactly the previous `end_time` (not `now`), preserving correct
    /// accounting.
    pub fn renew(env: Env, stream_id: u64) {
        let mut stream = storage::get_stream(&env, stream_id);
        stream.sender.require_auth();

        assert!(stream.auto_renew, "auto_renew is not enabled for this stream");
        assert!(!stream.cancelled, "stream is cancelled");
        assert!(
            env.ledger().timestamp() >= stream.end_time,
            "stream has not expired yet"
        );

        // Flush any unclaimed vested balance to recipient first.
        let claimable = Self::_claimable(&env, &stream);
        if claimable > 0 {
            stream.claimed += claimable;
            token::Client::new(&env, &stream.token).transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &claimable,
            );
        }

        // Pull a fresh deposit for the next cycle.
        token::Client::new(&env, &stream.token).transfer(
            &stream.sender,
            &env.current_contract_address(),
            &stream.deposit,
        );

        // ── Critical: new start_time == old end_time, NOT now ──────────────
        let duration = stream.end_time - stream.start_time;
        stream.start_time = stream.end_time;
        stream.end_time = stream.start_time + duration;
        stream.claimed = 0;

        storage::save_stream(&env, stream_id, &stream);

        // Reputation is incremented on each successful natural-completion + renewal.
        storage::increment_reputation(&env, &stream.sender);
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Reputation
    // ─────────────────────────────────────────────────────────────────────────

    /// Return the reputation score for a sender address.
    /// Score increments on each stream that reaches natural completion
    /// (via `renew`) without being cancelled.
    pub fn get_reputation(env: Env, sender: Address) -> u64 {
        storage::get_reputation(&env, &sender)
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  2-of-2 multisig stream creation
    // ─────────────────────────────────────────────────────────────────────────

    /// Propose a stream that requires a co-signer's approval before activation.
    ///
    /// The sender's deposit is held by the contract. If the co-signer does not
    /// approve within `approval_window_ledgers` ledgers, anyone can call
    /// `discard_proposal` to return the deposit.
    pub fn propose_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        duration: u64,
        auto_renew: bool,
        cosigner: Address,
        approval_window_ledgers: u32,
    ) -> u64 {
        sender.require_auth();

        assert!(duration > 0, "duration must be positive");
        assert!(deposit > 0, "deposit must be positive");
        assert!(
            deposit % (duration as i128) == 0,
            "deposit must be exactly divisible by duration"
        );
        assert!(approval_window_ledgers > 0, "approval window must be positive");

        let rate_per_second = deposit / (duration as i128);

        // Escrow the deposit immediately.
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        let expiry_ledger = env.ledger().sequence() + approval_window_ledgers;
        let proposal_id = storage::next_proposal_id(&env);
        storage::save_proposal(
            &env,
            proposal_id,
            &StreamProposal {
                sender,
                recipient,
                token,
                deposit,
                rate_per_second,
                duration,
                auto_renew,
                cosigner,
                expiry_ledger,
            },
        );
        proposal_id
    }

    /// Co-signer approves a pending proposal, activating the stream.
    ///
    /// The stream's `start_time` is set to the current ledger timestamp at the
    /// moment of approval.
    pub fn approve_proposal(env: Env, proposal_id: u64) -> u64 {
        let proposal = storage::get_proposal(&env, proposal_id);
        proposal.cosigner.require_auth();

        assert!(
            env.ledger().sequence() <= proposal.expiry_ledger,
            "proposal has expired"
        );

        let start_time = env.ledger().timestamp();
        let end_time = start_time + proposal.duration;

        let stream_id = storage::next_stream_id(&env);
        storage::save_stream(
            &env,
            stream_id,
            &Stream {
                sender: proposal.sender,
                recipient: proposal.recipient,
                token: proposal.token,
                deposit: proposal.deposit,
                claimed: 0,
                rate_per_second: proposal.rate_per_second,
                start_time,
                end_time,
                auto_renew: proposal.auto_renew,
                cancelled: false,
            },
        );

        storage::remove_proposal(&env, proposal_id);
        stream_id
    }

    /// Discard an expired (or otherwise unwanted) proposal and return the deposit.
    ///
    /// - Before expiry: only the original sender can call this (requires auth).
    /// - After expiry: anyone can call this; no auth required.
    pub fn discard_proposal(env: Env, proposal_id: u64, caller: Address) {
        let proposal = storage::get_proposal(&env, proposal_id);

        let is_expired = env.ledger().sequence() > proposal.expiry_ledger;

        if !is_expired {
            // Only the sender can retract before expiry.
            assert!(
                caller == proposal.sender,
                "proposal has not expired; only sender can retract"
            );
            proposal.sender.require_auth();
        }
        // After expiry anyone may discard — no auth needed.

        // Return escrowed deposit to sender.
        token::Client::new(&env, &proposal.token).transfer(
            &env.current_contract_address(),
            &proposal.sender,
            &proposal.deposit,
        );

        storage::remove_proposal(&env, proposal_id);
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Read-only queries
    // ─────────────────────────────────────────────────────────────────────────

    pub fn get_stream(env: Env, stream_id: u64) -> Stream {
        storage::get_stream(&env, stream_id)
    }

    pub fn get_claimable(env: Env, stream_id: u64) -> i128 {
        let stream = storage::get_stream(&env, stream_id);
        Self::_claimable(&env, &stream)
    }

    pub fn get_proposal(env: Env, proposal_id: u64) -> StreamProposal {
        storage::get_proposal(&env, proposal_id)
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Internal helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn _claimable(env: &Env, stream: &Stream) -> i128 {
        if stream.cancelled {
            return 0;
        }
        let now = env.ledger().timestamp();
        if now <= stream.start_time {
            return 0;
        }
        let effective = now.min(stream.end_time);
        let elapsed = (effective - stream.start_time) as i128;
        let vested = elapsed * stream.rate_per_second;
        (vested - stream.claimed).max(0)
    }
}
