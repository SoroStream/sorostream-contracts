use crate::types::Stream;
use soroban_sdk::{Address, Env, Symbol, Vec};

const STREAM_ID_KEY: &str = "next_id";
const ADMIN_KEY: &str = "admin";

/// Stores the contract admin address.
pub fn write_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, ADMIN_KEY), admin);
}

/// Loads the contract admin address.
pub fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

/// Asserts that the current caller is the admin. Panics otherwise.
pub fn check_admin(env: &Env) {
    read_admin(env)
        .expect("contract not initialized")
        .require_auth();
}

/// Returns and increments the global stream ID counter.
pub fn next_stream_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&Symbol::new(env, STREAM_ID_KEY))
        .unwrap_or(0u64);
    env.storage()
        .instance()
        .set(&Symbol::new(env, STREAM_ID_KEY), &(id + 1));
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
