use crate::types::Stream;
use soroban_sdk::{Address, Env, Symbol, Vec};

const STREAM_ID_KEY: &str = "next_id";
const ADMIN_KEY: &str = "admin";
const PAUSED_KEY: &str = "paused";
const PROTOCOL_FEE_KEY: &str = "fee_bps";
const TREASURY_KEY: &str = "treasury";

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

/// Returns the current stream ID counter without incrementing it.
pub fn get_current_stream_id(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, STREAM_ID_KEY))
        .unwrap_or(0u64)
}

/// Returns and increments the global stream ID counter.
///
/// # Panics
/// Panics if the stream ID counter would overflow `u64::MAX` — this requires
/// 2^64 streams to have been created and is not reachable in practice.
pub fn next_stream_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&Symbol::new(env, STREAM_ID_KEY))
        .unwrap_or(0u64);
    let next = id.checked_add(1).expect("stream id counter overflow");
    env.storage()
        .instance()
        .set(&Symbol::new(env, STREAM_ID_KEY), &next);
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

/// Removes a stream from storage.
pub fn remove_stream(env: &Env, stream_id: u64) {
    env.storage().persistent().remove(&stream_id);
}

// --- Counter helpers (persistent, O(1) per write) ---

fn sender_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sc"), addr.clone())
}

fn recipient_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "rc"), addr.clone())
}

fn sender_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
    (Symbol::new(env, "s"), addr.clone(), idx)
}

fn recipient_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
    (Symbol::new(env, "r"), addr.clone(), idx)
}

/// Appends a stream ID to the sender's index using counter+slot keys.
///
/// # Panics
/// Panics if the per-sender index slot counter would overflow `u32::MAX`
/// — this requires 4 billion streams from one sender and is not reachable.
pub fn index_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let cnt_key = sender_count_key(env, sender);
    let idx: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    env.storage().persistent().set(&sender_slot_key(env, sender, idx), &stream_id);
    let next = idx.checked_add(1).expect("sender index overflow");
    env.storage().persistent().set(&cnt_key, &next);
}

/// Appends a stream ID to the recipient's index using counter+slot keys.
///
/// # Panics
/// Panics if the per-recipient index slot counter would overflow `u32::MAX`.
pub fn index_by_recipient(env: &Env, recipient: &Address, stream_id: u64) {
    let cnt_key = recipient_count_key(env, recipient);
    let idx: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    env.storage().persistent().set(&recipient_slot_key(env, recipient, idx), &stream_id);
    let next = idx.checked_add(1).expect("recipient index overflow");
    env.storage().persistent().set(&cnt_key, &next);
}

/// Removes a stream ID from the sender's index (swap-and-pop).
pub fn unindex_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let cnt_key = sender_count_key(env, sender);
    let cnt: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    for i in 0..cnt {
        let slot_key = sender_slot_key(env, sender, i);
        if let Some(id) = env.storage().persistent().get::<_, u64>(&slot_key) {
            if id == stream_id {
                let last = cnt - 1;
                if i != last {
                    let last_id: u64 = env.storage().persistent().get(&sender_slot_key(env, sender, last)).unwrap_or(0);
                    env.storage().persistent().set(&slot_key, &last_id);
                }
                env.storage().persistent().remove(&sender_slot_key(env, sender, last));
                env.storage().persistent().set(&cnt_key, &last);
                return;
            }
        }
    }
}

/// Removes a stream ID from the recipient's index (swap-and-pop).
pub fn unindex_by_recipient(env: &Env, recipient: &Address, stream_id: u64) {
    let cnt_key = recipient_count_key(env, recipient);
    let cnt: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    for i in 0..cnt {
        let slot_key = recipient_slot_key(env, recipient, i);
        if let Some(id) = env.storage().persistent().get::<_, u64>(&slot_key) {
            if id == stream_id {
                let last = cnt - 1;
                if i != last {
                    let last_id: u64 = env.storage().persistent().get(&recipient_slot_key(env, recipient, last)).unwrap_or(0);
                    env.storage().persistent().set(&slot_key, &last_id);
                }
                env.storage().persistent().remove(&recipient_slot_key(env, recipient, last));
                env.storage().persistent().set(&cnt_key, &last);
                return;
            }
        }
    }
}

/// Returns all stream IDs for a sender by iterating over slots.
pub fn get_ids_by_sender(env: &Env, sender: &Address) -> Vec<u64> {
    let cnt: u32 = env.storage().persistent().get(&sender_count_key(env, sender)).unwrap_or(0u32);
    let mut ids = Vec::new(env);
    for i in 0..cnt {
        if let Some(id) = env.storage().persistent().get::<(Symbol, Address, u32), u64>(&sender_slot_key(env, sender, i)) {
            ids.push_back(id);
        }
    }
    ids
}

/// Returns all stream IDs for a recipient by iterating over slots.
pub fn get_ids_by_recipient(env: &Env, recipient: &Address) -> Vec<u64> {
    let cnt: u32 = env.storage().persistent().get(&recipient_count_key(env, recipient)).unwrap_or(0u32);
    let mut ids = Vec::new(env);
    for i in 0..cnt {
        if let Some(id) = env.storage().persistent().get::<(Symbol, Address, u32), u64>(&recipient_slot_key(env, recipient, i)) {
            ids.push_back(id);
        }
    }
    ids
}

/// Returns true if this (sender, nonce) pair has already been used.
pub fn nonce_used(env: &Env, sender: &Address, nonce: u64) -> bool {
    let key = (Symbol::new(env, "n"), sender.clone(), nonce);
    env.storage().persistent().has(&key)
}

/// Records a (sender, nonce) pair as used.
pub fn mark_nonce_used(env: &Env, sender: &Address, nonce: u64) {
    let key = (Symbol::new(env, "n"), sender.clone(), nonce);
    env.storage().persistent().set(&key, &true);
}

/// Returns whether the contract is currently paused.
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&Symbol::new(env, PAUSED_KEY))
        .unwrap_or(false)
}

/// Sets the paused state.
pub fn set_paused(env: &Env, paused: bool) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, PAUSED_KEY), &paused);
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

// --- Delegate helpers ---

fn delegate_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "del"), stream_id)
}

/// Gets the authorized delegate for a stream.
pub fn get_delegate(env: &Env, stream_id: u64) -> Option<Address> {
    env.storage().persistent().get(&delegate_key(env, stream_id))
}

/// Sets the authorized delegate for a stream.
pub fn set_delegate(env: &Env, stream_id: u64, delegate: &Address) {
    env.storage().persistent().set(&delegate_key(env, stream_id), delegate);
}

/// Removes the authorized delegate for a stream.
pub fn remove_delegate(env: &Env, stream_id: u64) {
    env.storage().persistent().remove(&delegate_key(env, stream_id));
}
