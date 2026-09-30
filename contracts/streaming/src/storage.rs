use soroban_sdk::{Address, Bytes, Env};

use crate::types::{DataKey, Stream, StreamProposal};

// ── TTL constants ─────────────────────────────────────────────────────────────
// Approximately 30 days at ~6-second average ledger close time.
const LEDGER_BUMP_AMOUNT: u32 = 518_400;
// Extend when fewer than 25 days of TTL remain.
const LEDGER_THRESHOLD: u32 = 432_000;

// ── Stream helpers ────────────────────────────────────────────────────────────

/// Derives a pseudo-random stream ID by hashing (sender ‖ recipient ‖ timestamp ‖ nonce).
///
/// This replaces the previous sequential counter, making stream IDs unpredictable
/// and preventing an attacker from enumerating all streams by iterating integers
/// (issue #621).  The first 8 bytes of the SHA-256 digest are used as the u64 ID.
///
/// `nonce` is a per-call counter stored in contract instance storage that ensures
/// two streams created in the same ledger second get distinct IDs.
pub fn derive_stream_id(env: &Env, sender: &Address, recipient: &Address, timestamp: u64, nonce: u64) -> u64 {
    let mut buf = Bytes::new(env);
    buf.append(&sender.to_xdr(env));
    buf.append(&recipient.to_xdr(env));
    buf.append(&Bytes::from_array(env, &timestamp.to_be_bytes()));
    buf.append(&Bytes::from_array(env, &nonce.to_be_bytes()));
    let hash = env.crypto().sha256(&buf);
    let hash_bytes = hash.to_array();
    u64::from_be_bytes([
        hash_bytes[0],
        hash_bytes[1],
        hash_bytes[2],
        hash_bytes[3],
        hash_bytes[4],
        hash_bytes[5],
        hash_bytes[6],
        hash_bytes[7],
    ])
}

/// Returns the current monotonic nonce, then increments it.
///
/// Used as an entropy input in [`derive_stream_id`] so that two streams
/// created in the same block with the same sender/recipient get distinct IDs.
pub fn next_nonce(env: &Env) -> u64 {
    let nonce: u64 = env
        .storage()
        .instance()
        .get(&DataKey::StreamCount)
        .unwrap_or(0u64);
    env.storage()
        .instance()
        .set(&DataKey::StreamCount, &(nonce + 1));
    nonce
}

/// Returns `true` if a stream with the given ID already exists in persistent storage.
pub fn stream_id_exists(env: &Env, stream_id: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Stream(stream_id))
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
