use soroban_sdk::Env;

use crate::types::{DataKey, Stream, StreamProposal};

// ── TTL constants ─────────────────────────────────────────────────────────────
// Approximately 30 days at ~6-second average ledger close time.
const LEDGER_BUMP_AMOUNT: u32 = 518_400;
// Extend when fewer than 25 days of TTL remain.
const LEDGER_THRESHOLD: u32 = 432_000;

// ── Stream helpers ────────────────────────────────────────────────────────────

pub fn next_stream_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::StreamCount)
        .unwrap_or(0u64);
    env.storage()
        .instance()
        .set(&DataKey::StreamCount, &(id + 1));
    id
}

pub fn save_stream(env: &Env, stream_id: u64, stream: &Stream) {
    let key = DataKey::Stream(stream_id);
    env.storage().persistent().set(&key, stream);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP_AMOUNT);
}

pub fn get_stream(env: &Env, stream_id: u64) -> Stream {
    env.storage()
        .persistent()
        .get(&DataKey::Stream(stream_id))
        .expect("stream not found")
}

// ── Proposal helpers ──────────────────────────────────────────────────────────

pub fn next_proposal_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::ProposalCount)
        .unwrap_or(0u64);
    env.storage()
        .instance()
        .set(&DataKey::ProposalCount, &(id + 1));
    id
}

pub fn save_proposal(env: &Env, proposal_id: u64, proposal: &StreamProposal) {
    let key = DataKey::Proposal(proposal_id);
    env.storage().persistent().set(&key, proposal);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP_AMOUNT);
}

pub fn get_proposal(env: &Env, proposal_id: u64) -> StreamProposal {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(proposal_id))
        .expect("proposal not found")
}

pub fn remove_proposal(env: &Env, proposal_id: u64) {
    env.storage()
        .persistent()
        .remove(&DataKey::Proposal(proposal_id));
}

// ── Reputation helpers ────────────────────────────────────────────────────────

pub fn get_reputation(env: &Env, sender: &soroban_sdk::Address) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::Reputation(sender.clone()))
        .unwrap_or(0u64)
}

pub fn increment_reputation(env: &Env, sender: &soroban_sdk::Address) {
    let score = get_reputation(env, sender);
    env.storage()
        .instance()
        .set(&DataKey::Reputation(sender.clone()), &(score + 1));
}
