use soroban_sdk::{Address, Env, Symbol, Vec};
use crate::types::Stream;

const STREAM_ID_KEY: &str = "next_id";
const PROTOCOL_FEE_KEY: &str = "fee_bps";
const TREASURY_KEY: &str = "treasury";

/// Gets the current stream ID counter without incrementing.
pub fn get_current_stream_id(env: &Env) -> u64 {
    env.storage().instance().get(&Symbol::new(env, STREAM_ID_KEY)).unwrap_or(0u64)
}

/// Returns and increments the global stream ID counter.
pub fn next_stream_id(env: &Env) -> u64 {
    let id: u64 = get_current_stream_id(env);
    env.storage().instance().set(&Symbol::new(env, STREAM_ID_KEY), &(id + 1));
    id
}

/// Persists a stream to storage.
pub fn save_stream(env: &Env, stream: &Stream) {
    env.storage().persistent().set(&stream.id, stream);
}

/// Loads a stream from storage. Returns None if not found.
pub fn load_stream(env: &Env, stream_id: u64) -> Option<Stream> {
    env.storage().persistent().get(&stream_id)
}

/// Appends a stream ID to the sender's index.
pub fn index_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let key = (Symbol::new(env, "s"), sender.clone());
    let mut ids: Vec<u64> = env.storage().temporary().get(&key).unwrap_or(Vec::new(env));
    ids.push_back(stream_id);
    env.storage().temporary().set(&key, &ids);
}

/// Appends a stream ID to the recipient's index.
pub fn index_by_recipient(env: &Env, recipient: &Address, stream_id: u64) {
    let key = (Symbol::new(env, "r"), recipient.clone());
    let mut ids: Vec<u64> = env.storage().temporary().get(&key).unwrap_or(Vec::new(env));
    ids.push_back(stream_id);
    env.storage().temporary().set(&key, &ids);
}

/// Returns all stream IDs for a sender.
pub fn get_ids_by_sender(env: &Env, sender: &Address) -> Vec<u64> {
    let key = (Symbol::new(env, "s"), sender.clone());
    env.storage().temporary().get(&key).unwrap_or(Vec::new(env))
}

/// Returns all stream IDs for a recipient.
pub fn get_ids_by_recipient(env: &Env, recipient: &Address) -> Vec<u64> {
    let key = (Symbol::new(env, "r"), recipient.clone());
    env.storage().temporary().get(&key).unwrap_or(Vec::new(env))
}

/// Gets the protocol fee in basis points (0 = no fee).
pub fn get_protocol_fee(env: &Env) -> u32 {
    env.storage().instance().get(&Symbol::new(env, PROTOCOL_FEE_KEY)).unwrap_or(0u32)
}

/// Sets the protocol fee in basis points.
pub fn set_protocol_fee(env: &Env, fee_bps: u32) {
    env.storage().instance().set(&Symbol::new(env, PROTOCOL_FEE_KEY), &fee_bps);
}

/// Gets the treasury address for protocol fees.
pub fn get_treasury(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, TREASURY_KEY))
}

/// Sets the treasury address for protocol fees.
pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&Symbol::new(env, TREASURY_KEY), treasury);
}
