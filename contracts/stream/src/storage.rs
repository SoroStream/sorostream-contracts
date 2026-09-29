#![allow(dead_code)]
use crate::types::{AuditEntry, Stream, StreamTransition, VestingTranche};
use soroban_sdk::{Address, Bytes, BytesN, Env, String, Symbol, Vec, xdr::ToXdr};

const ADMIN_KEY: &str = "admin";
const PAUSED_KEY: &str = "paused";
const PROTOCOL_FEE_KEY: &str = "fee_bps";
const TREASURY_KEY: &str = "treasury";
const MIN_DURATION_KEY: &str = "min_dur";
const MAX_DURATION_KEY: &str = "max_dur";
const VERSION_KEY: &str = "version";
const MAX_STREAMS_KEY: &str = "max_str";
const STREAM_COUNT_KEY: &str = "str_cnt";
const PENDING_FEE_KEY: &str = "pnd_fee";
const WITHDRAWAL_COOLDOWN_KEY: &str = "wd_cd";
const WHITELIST_ENABLED_KEY: &str = "wl_en";
const GUARDIAN_KEY: &str = "guardian";
const GOVERNANCE_KEY: &str = "governance";
const PAUSE_EXPIRES_KEY: &str = "p_exp";
/// Maximum pause duration in seconds (72 hours). After this the contract auto-unpauses.
pub const MAX_PAUSE_DURATION: u64 = 72 * 60 * 60;
const CREATION_FEE_XLM_KEY: &str = "cf_xlm";
/// Default maximum start-time offset: 365 days in seconds.
pub const DEFAULT_MAX_FUTURE_START_OFFSET: u64 = 365 * 24 * 60 * 60;
const MAX_FUTURE_OFFSET_KEY: &str = "mf_start";

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

/// Derives a deterministic stream ID from sender, recipient, start_time, and nonce.
pub fn derive_stream_id(
    env: &Env,
    sender: &Address,
    recipient: &Address,
    start_time: u64,
    nonce: u64,
) -> u64 {
    let mut buf = Bytes::new(env);
    buf.append(&sender.to_xdr(env));
    buf.append(&recipient.to_xdr(env));
    buf.append(&Bytes::from_array(env, &start_time.to_be_bytes()));
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

/// Returns true if a stream with the given ID already exists.
pub fn stream_exists(env: &Env, stream_id: u64) -> bool {
    env.storage().persistent().has(&stream_id)
}

/// Indexes a stream ID in the global enumeration list.
pub fn index_global_stream(env: &Env, stream_id: u64) {
    let cnt_key = Symbol::new(env, STREAM_COUNT_KEY);
    let idx: u32 = env.storage().instance().get(&cnt_key).unwrap_or(0u32);
    let slot_key = (Symbol::new(env, "gi"), idx);
    env.storage().persistent().set(&slot_key, &stream_id);
    env.storage().instance().set(&cnt_key, &(idx + 1));
}

/// Returns the total number of streams in the global index.
pub fn get_global_stream_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, STREAM_COUNT_KEY))
        .unwrap_or(0u32)
}

/// Returns the stream ID at a given position in the global index.
pub fn get_global_stream_at(env: &Env, idx: u32) -> Option<u64> {
    let slot_key = (Symbol::new(env, "gi"), idx);
    env.storage().persistent().get(&slot_key)
}

/// Persists a stream to storage.
pub fn save_stream(env: &Env, stream: &Stream) {
    let previous = load_stream(env, stream.id);
    env.storage().persistent().set(&stream.id, stream);

    let previous_status = previous.map(|previous_stream| previous_stream.status);
    if previous_status.as_ref() != Some(&stream.status) {
        let is_creation = previous_status.is_none();
        let from_status = previous_status.unwrap_or_else(|| stream.status.clone());
        append_stream_transition(
            env,
            stream.id,
            &StreamTransition {
                from_status,
                to_status: stream.status.clone(),
                is_creation,
                timestamp: env.ledger().timestamp(),
            },
        );
    }
}

/// Loads a stream from storage. Returns None if not found.
pub fn load_stream(env: &Env, stream_id: u64) -> Option<Stream> {
    env.storage().persistent().get(&stream_id)
}

/// Removes a stream from storage.
pub fn remove_stream(env: &Env, stream_id: u64) {
    env.storage().persistent().remove(&stream_id);
}

/// Marks a stream ID as cancelled using a lightweight persistent sentinel.
/// This allows `cancel_stream` to distinguish "already cancelled" from "never existed"
/// and return `StreamAlreadyCancelled` on a second call.
pub fn mark_stream_cancelled(env: &Env, stream_id: u64) {
    let key = (Symbol::new(env, "xcl"), stream_id);
    env.storage().persistent().set(&key, &true);
}

/// Returns true if the stream was previously cancelled (sentinel is present).
pub fn is_stream_cancelled(env: &Env, stream_id: u64) -> bool {
    let key = (Symbol::new(env, "xcl"), stream_id);
    env.storage().persistent().get::<_, bool>(&key).unwrap_or(false)
}

/// Key for the monotonic event sequence number associated with a stream.
pub fn stream_event_nonce_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "evn"), stream_id)
}

/// Reads the last emitted event nonce for a stream. Starts at 0 before any
/// stream events are published for this ID.
pub fn read_stream_event_nonce(env: &Env, stream_id: u64) -> u64 {
    env.storage()
        .persistent()
        .get(&stream_event_nonce_key(env, stream_id))
        .unwrap_or(0u64)
}

/// Increments the event nonce for a stream and returns the new value.
///
/// The first event for a stream gets nonce 1, which makes off-chain replay
/// protection deterministic and unambiguous even when multiple withdrawals share
/// the same `StreamWithdrawn` topic and stream_id.
pub fn next_stream_event_nonce(env: &Env, stream_id: u64) -> u64 {
    let key = stream_event_nonce_key(env, stream_id);
    let next = read_stream_event_nonce(env, stream_id)
        .checked_add(1)
        .expect("stream event nonce overflow");
    env.storage().persistent().set(&key, &next);
    next
}

// --- Counter helpers (persistent, O(1) per write) ---

pub fn sender_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sc"), addr.clone())
}

pub fn recipient_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "rc"), addr.clone())
}

pub fn sender_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
    (Symbol::new(env, "s"), addr.clone(), idx)
}

pub fn recipient_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
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
    if load_stream(env, stream_id)
        .map(|stream| stream.status == crate::types::StreamStatus::Active)
        .unwrap_or(false)
    {
        index_active_by_sender(env, sender, stream_id);
    }
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
    unindex_active_by_sender(env, sender, stream_id);
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

// --- Active sender index ---

fn active_sender_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "asc"), addr.clone())
}

fn active_sender_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
    (Symbol::new(env, "as"), addr.clone(), idx)
}

/// Adds a stream to the sender's active-only index.
pub fn index_active_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let count_key = active_sender_count_key(env, sender);
    let idx: u32 = env.storage().persistent().get(&count_key).unwrap_or(0u32);
    env.storage()
        .persistent()
        .set(&active_sender_slot_key(env, sender, idx), &stream_id);
    env.storage()
        .persistent()
        .set(&count_key, &idx.checked_add(1).expect("active sender index overflow"));
}

/// Removes a stream from the sender's active-only index using swap-and-pop.
pub fn unindex_active_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let count_key = active_sender_count_key(env, sender);
    let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0u32);
    for i in 0..count {
        let slot_key = active_sender_slot_key(env, sender, i);
        if env.storage().persistent().get::<_, u64>(&slot_key) == Some(stream_id) {
            let last = count - 1;
            if i != last {
                let last_id: u64 = env
                    .storage()
                    .persistent()
                    .get(&active_sender_slot_key(env, sender, last))
                    .unwrap_or(0);
                env.storage().persistent().set(&slot_key, &last_id);
            }
            env.storage()
                .persistent()
                .remove(&active_sender_slot_key(env, sender, last));
            env.storage().persistent().set(&count_key, &last);
            return;
        }
    }
}

/// Returns active stream IDs for a sender without scanning terminal streams.
pub fn get_active_ids_by_sender(env: &Env, sender: &Address) -> Vec<u64> {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&active_sender_count_key(env, sender))
        .unwrap_or(0u32);
    let mut ids = Vec::new(env);
    for i in 0..count {
        if let Some(id) = env
            .storage()
            .persistent()
            .get::<(Symbol, Address, u32), u64>(&active_sender_slot_key(env, sender, i))
        {
            ids.push_back(id);
        }
    }
    ids
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

/// Returns the number of streams created by a sender (including cancelled/expired).
pub fn get_sender_stream_count(env: &Env, sender: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&sender_count_key(env, sender))
        .unwrap_or(0u32)
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

// ── Tag index helpers ────────────────────────────────────────────────────────

fn tag_count_key(env: &Env, tag: &String) -> (Symbol, String) {
    (Symbol::new(env, "tc"), tag.clone())
}

fn tag_slot_key(env: &Env, tag: &String, idx: u32) -> (Symbol, String, u32) {
    (Symbol::new(env, "t"), tag.clone(), idx)
}

pub fn index_by_tag(env: &Env, tag: &String, stream_id: u64) {
    let cnt_key = tag_count_key(env, tag);
    let idx: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    env.storage().persistent().set(&tag_slot_key(env, tag, idx), &stream_id);
    let next = idx.checked_add(1).expect("tag index overflow");
    env.storage().persistent().set(&cnt_key, &next);
}

pub fn unindex_by_tag(env: &Env, tag: &String, stream_id: u64) {
    let cnt_key = tag_count_key(env, tag);
    let cnt: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    for i in 0..cnt {
        let slot_key = tag_slot_key(env, tag, i);
        if let Some(id) = env.storage().persistent().get::<_, u64>(&slot_key) {
            if id == stream_id {
                let last = cnt - 1;
                if i != last {
                    let last_id: u64 = env.storage().persistent().get(&tag_slot_key(env, tag, last)).unwrap_or(0);
                    env.storage().persistent().set(&slot_key, &last_id);
                }
                env.storage().persistent().remove(&tag_slot_key(env, tag, last));
                env.storage().persistent().set(&cnt_key, &last);
                return;
            }
        }
    }
}

pub fn get_ids_by_tag(env: &Env, tag: &String) -> Vec<u64> {
    let cnt: u32 = env.storage().persistent().get(&tag_count_key(env, tag)).unwrap_or(0u32);
    let mut ids = Vec::new(env);
    for i in 0..cnt {
        if let Some(id) = env.storage().persistent().get::<_, u64>(&tag_slot_key(env, tag, i)) {
            ids.push_back(id);
        }
    }
    ids
}

/// Returns the current batch nonce for a sender (next expected value).
pub fn get_batch_nonce(env: &Env, sender: &Address) -> u64 {
    let key = (Symbol::new(env, "bn"), sender.clone());
    env.storage().persistent().get(&key).unwrap_or(0u64)
}

/// Increments and stores the batch nonce for a sender.
pub fn increment_batch_nonce(env: &Env, sender: &Address) {
    let key = (Symbol::new(env, "bn"), sender.clone());
    let next = get_batch_nonce(env, sender).checked_add(1).expect("batch nonce overflow");
    env.storage().persistent().set(&key, &next);
}
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
#[allow(dead_code)]
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

/// Sets the timestamp at which the contract auto-unpauses (0 = no expiry).
pub fn set_pause_expiry(env: &Env, expiry: u64) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, PAUSE_EXPIRES_KEY), &expiry);
}

/// Returns the pause expiry timestamp (0 if not set).
pub fn get_pause_expiry(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, PAUSE_EXPIRES_KEY))
        .unwrap_or(0u64)
}

/// Returns whether the contract is currently paused, auto-unpausing if the
/// maximum pause duration has elapsed.
pub fn is_paused_or_auto_unpause(env: &Env) -> bool {
    let paused: bool = env.storage()
        .instance()
        .get(&Symbol::new(env, PAUSED_KEY))
        .unwrap_or(false);
    if !paused {
        return false;
    }
    let expiry = get_pause_expiry(env);
    if expiry > 0 && env.ledger().timestamp() >= expiry {
        // Auto-unpause: clear flags without emitting an event (event is emitted by caller)
        env.storage()
            .instance()
            .set(&Symbol::new(env, PAUSED_KEY), &false);
        env.storage()
            .instance()
            .set(&Symbol::new(env, PAUSE_EXPIRES_KEY), &0u64);
        return false;
    }
    true
}

/// Stores the guardian address (can call `pause`).
pub fn write_guardian(env: &Env, guardian: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, GUARDIAN_KEY), guardian);
}

/// Returns the guardian address, if set.
pub fn read_guardian(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, GUARDIAN_KEY))
}

/// Stores the governance address (can call `unpause`).
pub fn write_governance(env: &Env, governance: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, GOVERNANCE_KEY), governance);
}

/// Returns the governance address, if set.
pub fn read_governance(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, GOVERNANCE_KEY))
}

/// Gets the protocol fee in basis points (0 = no fee).
pub fn get_protocol_fee(env: &Env) -> u32 {
    env.storage().instance().get(&Symbol::new(env, PROTOCOL_FEE_KEY)).unwrap_or(0u32)
}

/// Sets the protocol fee in basis points.
pub fn set_protocol_fee(env: &Env, fee_bps: u32) {
    env.storage().instance().set(&Symbol::new(env, PROTOCOL_FEE_KEY), &fee_bps);
}

/// Reads the pending fee proposal (new_fee_bps, unlock_time) if any.
pub fn read_pending_fee_proposal(env: &Env) -> Option<(u32, u64)> {
    env.storage().instance().get(&Symbol::new(env, PENDING_FEE_KEY))
}

/// Writes a pending fee proposal.
pub fn write_pending_fee_proposal(env: &Env, new_fee_bps: u32, unlock_time: u64) {
    env.storage().instance().set(&Symbol::new(env, PENDING_FEE_KEY), &(new_fee_bps, unlock_time));
}

/// Clears the pending fee proposal.
pub fn clear_pending_fee_proposal(env: &Env) {
    env.storage().instance().remove(&Symbol::new(env, PENDING_FEE_KEY));
}

/// Gets the treasury address for protocol fees.
pub fn get_treasury(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, TREASURY_KEY))
}

/// Sets the treasury address for protocol fees.
pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&Symbol::new(env, TREASURY_KEY), treasury);
}

/// Gets the minimum stream duration in seconds (default 3600 if not set).
pub fn read_min_duration(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, MIN_DURATION_KEY))
        .unwrap_or(3600u64)
}

/// Sets the minimum stream duration in seconds.
pub fn write_min_duration(env: &Env, duration: u64) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, MIN_DURATION_KEY), &duration);
}

/// Gets the maximum stream duration in seconds.
///
/// A value of 0 is treated as the protocol hard cap instead of “unlimited,” so
/// stream end times cannot be effectively unbounded.
pub fn read_max_duration(env: &Env) -> u64 {
    let configured = env.storage()
        .instance()
        .get(&Symbol::new(env, MAX_DURATION_KEY))
        .unwrap_or(crate::MAX_STREAM_DURATION_SECONDS);
    if configured == 0 {
        crate::MAX_STREAM_DURATION_SECONDS
    } else {
        configured.min(crate::MAX_STREAM_DURATION_SECONDS)
    }
}

/// Sets the maximum stream duration in seconds.
///
/// `0` and any value above the protocol hard cap are clamped back to the hard
/// cap so a stream cannot live beyond the protocol's safe lifetime.
pub fn write_max_duration(env: &Env, duration: u64) {
    let capped = if duration == 0 || duration > crate::MAX_STREAM_DURATION_SECONDS {
        crate::MAX_STREAM_DURATION_SECONDS
    } else {
        duration
    };
    env.storage()
        .instance()
        .set(&Symbol::new(env, MAX_DURATION_KEY), &capped);
}

/// Gets the maximum allowed future start-time offset in seconds.
///
/// When a caller creates a scheduled stream with an explicit `start_time`,
/// `start_time` must satisfy `start_time <= now + max_future_start_offset`.
/// Defaults to [`DEFAULT_MAX_FUTURE_START_OFFSET`] (365 days) when not set.
pub fn read_max_future_start_offset(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, MAX_FUTURE_OFFSET_KEY))
        .unwrap_or(DEFAULT_MAX_FUTURE_START_OFFSET)
}

/// Sets the maximum allowed future start-time offset in seconds.
/// Only the admin may call this via the contract's `set_max_future_start_offset` instruction.
pub fn write_max_future_start_offset(env: &Env, offset_seconds: u64) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, MAX_FUTURE_OFFSET_KEY), &offset_seconds);
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

// --- Version tracking ---

/// Stores the contract version string.
pub fn write_version(env: &Env, version: &soroban_sdk::String) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, VERSION_KEY), version);
}

/// Reads the contract version string.
pub fn read_version(env: &Env) -> Option<soroban_sdk::String> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, VERSION_KEY))
}

// --- Rate limiting ---

/// Gets the global maximum streams per sender (default: 1000).
pub fn get_max_streams_per_sender(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, MAX_STREAMS_KEY))
        .unwrap_or(1000u32)
}

/// Sets the global maximum streams per sender.
pub fn set_max_streams_per_sender(env: &Env, max_streams: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, MAX_STREAMS_KEY), &max_streams);
}

/// Gets the global withdrawal cooldown in seconds (default: 0).
pub fn get_withdrawal_cooldown(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, WITHDRAWAL_COOLDOWN_KEY))
        .unwrap_or(0u64)
}

/// Sets the global withdrawal cooldown in seconds.
pub fn set_withdrawal_cooldown(env: &Env, cooldown: u64) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, WITHDRAWAL_COOLDOWN_KEY), &cooldown);
}

/// Returns whether recipient whitelisting is enabled.
pub fn is_whitelist_enabled(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&Symbol::new(env, WHITELIST_ENABLED_KEY))
        .unwrap_or(false)
}

/// Enables or disables recipient whitelisting.
pub fn set_whitelist_enabled(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, WHITELIST_ENABLED_KEY), &enabled);
}

fn whitelist_key(env: &Env, recipient: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "wl"), recipient.clone())
}

/// Returns whether a recipient is whitelisted.
pub fn is_whitelisted(env: &Env, recipient: &Address) -> bool {
    env.storage().persistent().get(&whitelist_key(env, recipient)).unwrap_or(false)
}

/// Adds a recipient to the whitelist.
pub fn add_to_whitelist(env: &Env, recipient: &Address) {
    env.storage().persistent().set(&whitelist_key(env, recipient), &true);
}

/// Removes a recipient from the whitelist.
pub fn remove_from_whitelist(env: &Env, recipient: &Address) {
    env.storage().persistent().remove(&whitelist_key(env, recipient));
}

// --- Recipient allowlist (for regulated payment scenarios) ---

const RECIPIENT_ALLOWLIST_ENABLED_KEY: &str = "ral_en";

/// Returns whether recipient allowlisting is globally enabled.
pub fn is_recipient_allowlist_enabled(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&Symbol::new(env, RECIPIENT_ALLOWLIST_ENABLED_KEY))
        .unwrap_or(false)
}

/// Enables or disables recipient allowlisting.
pub fn set_recipient_allowlist_enabled(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, RECIPIENT_ALLOWLIST_ENABLED_KEY), &enabled);
}

fn recipient_allowlist_key(env: &Env, recipient: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "ral"), recipient.clone())
}

/// Returns whether a recipient is on the allowlist.
pub fn is_recipient_allowed(env: &Env, recipient: &Address) -> bool {
    env.storage().persistent().get(&recipient_allowlist_key(env, recipient)).unwrap_or(false)
}

/// Adds a recipient to the allowlist.
pub fn add_to_recipient_allowlist(env: &Env, recipient: &Address) {
    env.storage().persistent().set(&recipient_allowlist_key(env, recipient), &true);
}

/// Removes a recipient from the allowlist.
pub fn remove_from_recipient_allowlist(env: &Env, recipient: &Address) {
    env.storage().persistent().remove(&recipient_allowlist_key(env, recipient));
}

// --- Fee exemption list ---

fn fee_exempt_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "fe"), addr.clone())
}

/// Returns whether `addr` is exempt from the protocol fee.
pub fn is_fee_exempt(env: &Env, addr: &Address) -> bool {
    env.storage().persistent().get(&fee_exempt_key(env, addr)).unwrap_or(false)
}

/// Adds `addr` to the fee exemption list.
pub fn add_fee_exempt(env: &Env, addr: &Address) {
    env.storage().persistent().set(&fee_exempt_key(env, addr), &true);
}

/// Removes `addr` from the fee exemption list.
pub fn remove_fee_exempt(env: &Env, addr: &Address) {
    env.storage().persistent().remove(&fee_exempt_key(env, addr));
}

fn sender_limit_key(env: &Env, sender: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sl"), sender.clone())
}

/// Gets the per-sender stream limit override, if set.
pub fn get_sender_limit(env: &Env, sender: &Address) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&sender_limit_key(env, sender))
}

/// Sets a per-sender stream limit override.
pub fn set_sender_limit(env: &Env, sender: &Address, limit: u32) {
    env.storage()
        .persistent()
        .set(&sender_limit_key(env, sender), &limit);
}

/// Returns the effective stream limit for a sender (per-sender override or global default).
pub fn effective_sender_limit(env: &Env, sender: &Address) -> u32 {
    get_sender_limit(env, sender).unwrap_or_else(|| get_max_streams_per_sender(env))
}

// --- Audit log helpers (circular buffer, capacity = 20) ---

const AUDIT_HEAD_KEY: &str = "al_head";
const AUDIT_LEN_KEY: &str = "al_len";
const AUDIT_CAP: u32 = 20;

fn audit_slot_key(env: &Env, idx: u32) -> (Symbol, u32) {
    (Symbol::new(env, "al"), idx)
}

/// Appends an audit entry to the circular buffer.
pub fn append_audit_entry(env: &Env, entry: &AuditEntry) {
    let head: u32 = env.storage().instance().get(&Symbol::new(env, AUDIT_HEAD_KEY)).unwrap_or(0u32);
    let len: u32 = env.storage().instance().get(&Symbol::new(env, AUDIT_LEN_KEY)).unwrap_or(0u32);

    let write_idx = head % AUDIT_CAP;
    env.storage().instance().set(&audit_slot_key(env, write_idx), entry);

    let new_head = (head + 1) % AUDIT_CAP;
    let new_len = (len + 1).min(AUDIT_CAP);
    env.storage().instance().set(&Symbol::new(env, AUDIT_HEAD_KEY), &new_head);
    env.storage().instance().set(&Symbol::new(env, AUDIT_LEN_KEY), &new_len);
}

/// Returns all audit entries in chronological order (oldest first).
pub fn read_audit_log(env: &Env) -> Vec<AuditEntry> {
    let head: u32 = env.storage().instance().get(&Symbol::new(env, AUDIT_HEAD_KEY)).unwrap_or(0u32);
    let len: u32 = env.storage().instance().get(&Symbol::new(env, AUDIT_LEN_KEY)).unwrap_or(0u32);
    let mut result = Vec::new(env);
    for i in 0..len {
        // oldest entry is at (head - len + i) mod CAP
        let idx = (head + AUDIT_CAP - len + i) % AUDIT_CAP;
        if let Some(entry) = env.storage().instance().get::<(Symbol, u32), AuditEntry>(&audit_slot_key(env, idx)) {
            result.push_back(entry);
        }
    }
    result
}

/// Gets the flat XLM creation fee in stroops (default: 0).
pub fn get_creation_fee_xlm(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, CREATION_FEE_XLM_KEY))
        .unwrap_or(0i128)
}

/// Sets the flat XLM creation fee in stroops.
pub fn set_creation_fee_xlm(env: &Env, fee: i128) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, CREATION_FEE_XLM_KEY), &fee);
}

const XLM_TOKEN_KEY: &str = "xlm_tok";

const ACTIVE_STREAM_COUNT_KEY: &str = "act_cnt";

/// Gets the XLM SAC token contract address used for creation fee collection.
pub fn get_xlm_token(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, XLM_TOKEN_KEY))
}

/// Sets the XLM SAC token contract address.
pub fn set_xlm_token(env: &Env, xlm_token: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, XLM_TOKEN_KEY), xlm_token);
}

/// Returns the current count of active streams.
pub fn get_active_stream_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, ACTIVE_STREAM_COUNT_KEY))
        .unwrap_or(0u32)
}

/// Increments the active stream count by 1.
pub fn increment_active_stream_count(env: &Env) {
    let key = Symbol::new(env, ACTIVE_STREAM_COUNT_KEY);
    let current: u32 = env.storage().instance().get(&key).unwrap_or(0u32);
    env.storage().instance().set(&key, &(current + 1));
}

/// Decrements the active stream count by 1 (saturates at 0).
pub fn decrement_active_stream_count(env: &Env) {
    let key = Symbol::new(env, ACTIVE_STREAM_COUNT_KEY);
    let current: u32 = env.storage().instance().get(&key).unwrap_or(0u32);
    if current > 0 {
        env.storage().instance().set(&key, &(current - 1));
    }
}

/// Sets the active stream count directly (for recalibration).
pub fn set_active_stream_count(env: &Env, count: u32) {
    let key = Symbol::new(env, ACTIVE_STREAM_COUNT_KEY);
    env.storage().instance().set(&key, &count);
}

// --- Reentrancy guard ---

const REENTRANCY_LOCK_KEY: &str = "re_lk";

/// Returns true if the reentrancy lock is currently held.
pub fn is_reentrancy_locked(env: &Env) -> bool {
    env.storage()
        .temporary()
        .get(&Symbol::new(env, REENTRANCY_LOCK_KEY))
        .unwrap_or(false)
}

/// Acquires the reentrancy lock.
pub fn set_reentrancy_lock(env: &Env) {
    env.storage()
        .temporary()
        .set(&Symbol::new(env, REENTRANCY_LOCK_KEY), &true);
}

/// Releases the reentrancy lock.
pub fn clear_reentrancy_lock(env: &Env) {
    env.storage()
        .temporary()
        .remove(&Symbol::new(env, REENTRANCY_LOCK_KEY));
}

// --- Migration helpers ---

const APPLIED_MIGRATIONS_KEY: &str = "migrations";

/// Returns the set of applied migration version strings.
pub fn read_applied_migrations(env: &Env) -> Vec<soroban_sdk::String> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, APPLIED_MIGRATIONS_KEY))
        .unwrap_or_else(|| Vec::new(env))
}

/// Records a migration as applied.
pub fn record_migration(env: &Env, version: &soroban_sdk::String) {
    let mut applied = read_applied_migrations(env);
    applied.push_back(version.clone());
    env.storage().instance().set(&Symbol::new(env, APPLIED_MIGRATIONS_KEY), &applied);
}

// --- Token fee tier helpers ---

fn token_fee_tier_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "tft"), token.clone())
}

/// Gets the fee tier (in basis points) for a specific token, if set.
pub fn get_token_fee_tier(env: &Env, token: &Address) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&token_fee_tier_key(env, token))
}

/// Sets the fee tier (in basis points) for a specific token.
pub fn set_token_fee_tier(env: &Env, token: &Address, fee_bps: u32) {
    env.storage()
        .persistent()
        .set(&token_fee_tier_key(env, token), &fee_bps);
}

/// Removes the fee tier for a specific token (falls back to global default).
pub fn remove_token_fee_tier(env: &Env, token: &Address) {
    env.storage()
        .persistent()
        .remove(&token_fee_tier_key(env, token));
}

/// Gets the effective fee tier for a token (token-specific or global default).
pub fn get_effective_fee_tier(env: &Env, token: &Address) -> u32 {
    get_token_fee_tier(env, token).unwrap_or_else(|| get_protocol_fee(env))
}

// --- Holdback escrow helpers ---

fn holdback_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "hb"), stream_id)
}

/// Returns the holdback escrow amount for a stream (0 if not set).
pub fn get_holdback(env: &Env, stream_id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&holdback_key(env, stream_id))
        .unwrap_or(0i128)
}

/// Stores the holdback escrow amount for a stream.
pub fn set_holdback(env: &Env, stream_id: u64, amount: i128) {
    env.storage()
        .persistent()
        .set(&holdback_key(env, stream_id), &amount);
}

/// Removes the holdback escrow entry (after claim or claw-back).
pub fn remove_holdback(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&holdback_key(env, stream_id));
}

// ---------------------------------------------------------------------------
// Step-vesting tranche helpers
// ---------------------------------------------------------------------------

/// Storage key for a stream's tranche list: ("vt", stream_id).
fn tranche_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "vt"), stream_id)
}

/// Persists the tranche list for a step-vesting stream.
pub fn save_tranches(env: &Env, stream_id: u64, tranches: &Vec<VestingTranche>) {
    env.storage()
        .persistent()
        .set(&tranche_key(env, stream_id), tranches);
}

/// Loads the tranche list for a stream. Returns an empty Vec if not found.
pub fn load_tranches(env: &Env, stream_id: u64) -> Vec<VestingTranche> {
    env.storage()
        .persistent()
        .get(&tranche_key(env, stream_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Removes the tranche list from storage (called on cancel / completion).
pub fn remove_tranches(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&tranche_key(env, stream_id));
}

// --- Rate Limiting ---

const RATE_LIMIT_WINDOW_KEY: &str = "rl_win";
const RATE_LIMIT_MAX_KEY: &str = "rl_max";
const RATE_LIMIT_EXEMPT_KEY: &str = "rl_ex";

/// Key for the ledger-sequence-based rate limit window size.
const RATE_LIMIT_WINDOW_LEDGERS_KEY: &str = "rl_wl";

/// Default rate limit window: 720 ledgers ≈ 1 hour at ~5 s/ledger.
pub const DEFAULT_RATE_LIMIT_WINDOW_LEDGERS: u32 = 720;

/// Gets the rate limit window size in ledgers (default: 720 ≈ 1 hour).
pub fn get_rate_limit_window(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, RATE_LIMIT_WINDOW_LEDGERS_KEY))
        .unwrap_or(DEFAULT_RATE_LIMIT_WINDOW_LEDGERS)
}

/// Sets the rate limit window size in ledgers.
pub fn set_rate_limit_window(env: &Env, window_ledgers: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, RATE_LIMIT_WINDOW_LEDGERS_KEY), &window_ledgers);
}

/// Gets the max creations per window (default: 20).
pub fn get_rate_limit_max_creations(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, RATE_LIMIT_MAX_KEY))
        .unwrap_or(20u32)
}

/// Sets the max creations per window.
pub fn set_rate_limit_max_creations(env: &Env, max_creations: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, RATE_LIMIT_MAX_KEY), &max_creations);
}

fn rate_limit_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "rl"), addr.clone())
}

/// Gets rate-limit state: `(window_start_ledger, count_in_window)` from temporary storage.
///
/// Temporary storage is correct here: if the entry's TTL expires, it is equivalent
/// to the window having fully elapsed — the next call starts a fresh window at `count = 0`.
/// Callers should extend the TTL to `window_ledgers` on every write (see `set_rate_limit_state`).
pub fn get_rate_limit_state(env: &Env, addr: &Address) -> (u32, u32) {
    env.storage()
        .temporary()
        .get(&rate_limit_key(env, addr))
        .unwrap_or((0u32, 0u32))
}

/// Persists rate-limit state `(window_start_ledger, count)` in temporary storage and extends
/// the entry's TTL to `window_ledgers` so the counter survives the full sliding window.
pub fn set_rate_limit_state(env: &Env, addr: &Address, window_start: u32, count: u32) {
    let key = rate_limit_key(env, addr);
    env.storage()
        .temporary()
        .set(&key, &(window_start, count));
    // Keep the entry alive for at least one full window so it cannot expire mid-window.
    let ttl = get_rate_limit_window(env).max(1);
    env.storage()
        .temporary()
        .extend_ttl(&key, ttl, ttl);
}

/// Returns how many more stream creations the given sender may perform in the current window.
///
/// Returns `u32::MAX` if the sender is exempt from rate limiting.
pub fn get_remaining_quota(env: &Env, addr: &Address) -> u32 {
    if is_rate_limit_exempt(env, addr) {
        return u32::MAX;
    }
    let max = get_rate_limit_max_creations(env);
    let window = get_rate_limit_window(env);
    let (ws, count) = get_rate_limit_state(env, addr);
    let current_ledger = env.ledger().sequence();
    // If the window has elapsed the counter resets, so the full quota is available.
    if current_ledger >= ws.saturating_add(window) {
        max
    } else {
        max.saturating_sub(count)
    }
}

fn rate_limit_exempt_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "rle"), addr.clone())
}

/// Returns whether an address is exempt from rate limiting.
pub fn is_rate_limit_exempt(env: &Env, addr: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&rate_limit_exempt_key(env, addr))
        .unwrap_or(false)
}

/// Adds an address to the rate limit exempt list.
pub fn add_rate_limit_exempt(env: &Env, addr: &Address) {
    env.storage()
        .persistent()
        .set(&rate_limit_exempt_key(env, addr), &true);
}

/// Removes an address from the rate limit exempt list.
pub fn remove_rate_limit_exempt(env: &Env, addr: &Address) {
    env.storage()
        .persistent()
        .remove(&rate_limit_exempt_key(env, addr));
}

// --- Token Whitelist (for tokens, not recipients) ---

const TOKEN_WHITELIST_ENABLED_KEY: &str = "twl_en";

fn token_whitelist_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "twl"), token.clone())
}

/// Returns whether token whitelisting is enabled.
pub fn is_token_whitelist_enabled(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&Symbol::new(env, TOKEN_WHITELIST_ENABLED_KEY))
        .unwrap_or(false)
}

/// Enables or disables token whitelisting.
pub fn set_token_whitelist_enabled(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, TOKEN_WHITELIST_ENABLED_KEY), &enabled);
}

/// Returns whether a token is whitelisted.
pub fn is_token_whitelisted(env: &Env, token: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&token_whitelist_key(env, token))
        .unwrap_or(false)
}

/// Adds a token to the whitelist.
pub fn add_token_to_whitelist(env: &Env, token: &Address) {
    env.storage()
        .persistent()
        .set(&token_whitelist_key(env, token), &true);
}

/// Removes a token from the whitelist.
pub fn remove_token_from_whitelist(env: &Env, token: &Address) {
    env.storage()
        .persistent()
        .remove(&token_whitelist_key(env, token));
}

// --- Fee Sweep Tracking ---

const FEES_COLLECTED_KEY: &str = "fees_coll";

fn fees_collected_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, FEES_COLLECTED_KEY), token.clone())
}

/// Gets accumulated fees for a token.
pub fn get_fees_collected(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&fees_collected_key(env, token))
        .unwrap_or(0i128)
}

/// Adds `amount` to the accumulated fees for the given token.
///
/// # Panics
/// Panics on i128 overflow — not reachable in practice.
pub fn accumulate_fees(env: &Env, token: &Address, amount: i128) {
    if amount <= 0 {
        return;
    }
    let current = get_fees_collected(env, token);
    let next = current.checked_add(amount).expect("fees_collected overflow");
    env.storage()
        .persistent()
        .set(&fees_collected_key(env, token), &next);
}

/// Resets accumulated fees for the given token to zero and returns the swept amount.
pub fn drain_fees_collected(env: &Env, token: &Address) -> i128 {
    let amount = get_fees_collected(env, token);
    if amount > 0 {
        env.storage()
            .persistent()
            .remove(&fees_collected_key(env, token));
    }
    amount
}

/// Sets accumulated fees for a token.
pub fn set_fees_collected(env: &Env, token: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&fees_collected_key(env, token), &amount);
}

/// Increments accumulated fees for a token.
pub fn increment_fees_collected(env: &Env, token: &Address, amount: i128) -> Result<(), crate::errors::StreamError> {
    let current = get_fees_collected(env, token);
    let new = current.checked_add(amount).ok_or(crate::errors::StreamError::Overflow)?;
    set_fees_collected(env, token, new);
    Ok(())
}

// --- Slippage Protection ---

fn slippage_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "slip"), stream_id)
}

/// Gets slippage parameters for a stream: (reference_price_bps, max_slippage_bps).
pub fn get_slippage_params(env: &Env, stream_id: u64) -> Option<(i128, u32)> {
    env.storage()
        .persistent()
        .get(&slippage_key(env, stream_id))
}

/// Sets slippage parameters for a stream.
pub fn set_slippage_params(env: &Env, stream_id: u64, reference_price: i128, max_slippage_bps: u32) {
    env.storage()
        .persistent()
        .set(&slippage_key(env, stream_id), &(reference_price, max_slippage_bps));
}

// --- Stream Creation Cooldown ---

const STREAM_CREATION_COOLDOWN_KEY: &str = "sc_cd";

fn sender_last_creation_key(env: &Env, sender: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "lc"), sender.clone())
}

/// Gets the global stream creation cooldown in seconds (0 = disabled).
pub fn get_stream_creation_cooldown(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, STREAM_CREATION_COOLDOWN_KEY))
        .unwrap_or(0u64)
}

/// Sets the global stream creation cooldown in seconds.
pub fn set_stream_creation_cooldown(env: &Env, cooldown_seconds: u64) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, STREAM_CREATION_COOLDOWN_KEY), &cooldown_seconds);
}

/// Gets the last stream creation time for a sender (0 if never created).
pub fn get_sender_last_creation_time(env: &Env, sender: &Address) -> u64 {
    env.storage()
        .persistent()
        .get(&sender_last_creation_key(env, sender))
        .unwrap_or(0u64)
}

/// Updates the last stream creation time for a sender.
pub fn set_sender_last_creation_time(env: &Env, sender: &Address, timestamp: u64) {
    env.storage()
        .persistent()
        .set(&sender_last_creation_key(env, sender), &timestamp);
}

// --- Federation Address Registry (Issue #238) ---

fn federation_registry_key(env: &Env, federation_name: &String) -> (Symbol, String) {
    (Symbol::new(env, "fed"), federation_name.clone())
}

/// Gets the Stellar address registered for a federation name.
pub fn get_federation_address(env: &Env, federation_name: &String) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&federation_registry_key(env, federation_name))
}

/// Registers a federation name to a Stellar address.
pub fn register_federation_address(env: &Env, federation_name: &String, address: &Address) {
    env.storage()
        .persistent()
        .set(&federation_registry_key(env, federation_name), address);
}

/// Unregisters a federation name from the registry.
pub fn unregister_federation_address(env: &Env, federation_name: &String) {
    env.storage()
        .persistent()
        .remove(&federation_registry_key(env, federation_name));
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (a): StreamExpiryWarning
// ═══════════════════════════════════════════════════════════════════════════

const EXPIRY_WARNING_WINDOW_KEY: &str = "exp_win";

/// Gets the expiry warning window in ledgers (default: 17280 = ~24 hours).
pub fn get_expiry_warning_window(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, EXPIRY_WARNING_WINDOW_KEY))
        .unwrap_or(17_280u32)
}

/// Sets the expiry warning window in ledgers.
pub fn set_expiry_warning_window(env: &Env, ledgers: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, EXPIRY_WARNING_WINDOW_KEY), &ledgers);
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (b): Sender reputation cap
// ═══════════════════════════════════════════════════════════════════════════

const NEW_SENDER_STREAM_CAP_KEY: &str = "ns_cap";
const SENDER_PROMOTION_THRESHOLD_KEY: &str = "sp_thr";

fn sender_lifetime_count_key(env: &Env, sender: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sl_cnt"), sender.clone())
}

/// Gets the stream cap for new senders (default: 10).
pub fn get_new_sender_stream_cap(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, NEW_SENDER_STREAM_CAP_KEY))
        .unwrap_or(10u32)
}

/// Sets the stream cap for new senders.
pub fn set_new_sender_stream_cap(env: &Env, cap: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, NEW_SENDER_STREAM_CAP_KEY), &cap);
}

/// Gets the sender promotion threshold (number of streams after which cap no longer applies).
/// Default: 50 streams.
pub fn get_sender_promotion_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, SENDER_PROMOTION_THRESHOLD_KEY))
        .unwrap_or(50u32)
}

/// Sets the sender promotion threshold.
pub fn set_sender_promotion_threshold(env: &Env, threshold: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, SENDER_PROMOTION_THRESHOLD_KEY), &threshold);
}

/// Gets the lifetime stream count for a sender (total streams ever created).
pub fn get_sender_lifetime_count(env: &Env, sender: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&sender_lifetime_count_key(env, sender))
        .unwrap_or(0u32)
}

/// Increments the lifetime stream count for a sender.
pub fn increment_sender_lifetime_count(env: &Env, sender: &Address) {
    let key = sender_lifetime_count_key(env, sender);
    let current = get_sender_lifetime_count(env, sender);
    let next = current.checked_add(1).expect("sender lifetime count overflow");
    env.storage().persistent().set(&key, &next);
}

/// Returns whether a sender is promoted (has crossed the threshold).
pub fn is_sender_promoted(env: &Env, sender: &Address) -> bool {
    get_sender_lifetime_count(env, sender) >= get_sender_promotion_threshold(env)
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (c): Stream redirect
// ═══════════════════════════════════════════════════════════════════════════

// Redirect target is stored in Stream.options.redirect_to_stream_id (no separate storage key needed).

// ═══════════════════════════════════════════════════════════════════════════
// Feature (f): Instance TTL extension
// ═══════════════════════════════════════════════════════════════════════════

/// Extends the contract instance storage TTL to the maximum allowed by Soroban.
/// Should be called on every `create_stream` and `top_up` to prevent contract eviction.
pub fn extend_instance_ttl(env: &Env) {
    let ttl = 535_679u32;
    env.storage().instance().extend_ttl(ttl, ttl);
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (g): Per-token stream count cap
// ═══════════════════════════════════════════════════════════════════════════

const MAX_STREAMS_PER_TOKEN_KEY: &str = "max_tok";

fn token_stream_count_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "tsc"), token.clone())
}

/// Gets the per-token stream cap (0 = unlimited).
pub fn get_max_streams_per_token(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, MAX_STREAMS_PER_TOKEN_KEY))
        .unwrap_or(0u32)
}

/// Sets the per-token stream cap. Setting to 0 disables the cap.
pub fn set_max_streams_per_token(env: &Env, max: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, MAX_STREAMS_PER_TOKEN_KEY), &max);
}

/// Returns the current active stream count for the given token.
pub fn get_token_stream_count(env: &Env, token: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&token_stream_count_key(env, token))
        .unwrap_or(0u32)
}

/// Increments the active stream count for the given token.
pub fn increment_token_stream_count(env: &Env, token: &Address) {
    let key = token_stream_count_key(env, token);
    let current = get_token_stream_count(env, token);
    env.storage().persistent().set(&key, &(current + 1));
}

/// Decrements the active stream count for the given token (saturates at 0).
pub fn decrement_token_stream_count(env: &Env, token: &Address) {
    let key = token_stream_count_key(env, token);
    let current = get_token_stream_count(env, token);
    if current > 0 {
        env.storage().persistent().set(&key, &(current - 1));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (h): Address blocklist
// ═══════════════════════════════════════════════════════════════════════════

fn blocklist_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "bl"), addr.clone())
}

/// Adds an address to the blocklist.
pub fn add_to_blocklist(env: &Env, addr: &Address) {
    env.storage().persistent().set(&blocklist_key(env, addr), &true);
}

/// Removes an address from the blocklist.
pub fn remove_from_blocklist(env: &Env, addr: &Address) {
    env.storage().persistent().remove(&blocklist_key(env, addr));
}

/// Returns true if the address is on the blocklist.
pub fn is_blocked(env: &Env, addr: &Address) -> bool {
    env.storage().persistent().get(&blocklist_key(env, addr)).unwrap_or(false)
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (i): Post-expiry grace period
// ═══════════════════════════════════════════════════════════════════════════

const GRACE_PERIOD_LEDGERS_KEY: &str = "grace";

/// Gets the grace period in ledgers (0 = no grace period).
pub fn get_grace_period_ledgers(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, GRACE_PERIOD_LEDGERS_KEY))
        .unwrap_or(0u32)
}

/// Sets the grace period in ledgers. Zero means no grace period.
pub fn set_grace_period_ledgers(env: &Env, ledgers: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, GRACE_PERIOD_LEDGERS_KEY), &ledgers);
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (j): Per-asset maximum deposit limit
// ═══════════════════════════════════════════════════════════════════════════

fn max_deposit_per_token_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "max_dep"), token.clone())
}

/// Gets the maximum single-stream deposit amount for a token (0 = unlimited).
pub fn get_max_deposit_per_token(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&max_deposit_per_token_key(env, token))
        .unwrap_or(0i128)
}

/// Sets the maximum single-stream deposit amount for a token. Setting to 0 disables the limit.
pub fn set_max_deposit_per_token(env: &Env, token: &Address, max_deposit: i128) {
    env.storage()
        .persistent()
        .set(&max_deposit_per_token_key(env, token), &max_deposit);
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (d): Dual-token streams
// ═══════════════════════════════════════════════════════════════════════════

fn dual_stream_token2_key(env: &Env, stream_id: u64) -> (Symbol, u64, Symbol) {
    (Symbol::new(env, "ds"), stream_id, Symbol::new(env, "tok2"))
}

fn dual_stream_deposit2_key(env: &Env, stream_id: u64) -> (Symbol, u64, Symbol) {
    (Symbol::new(env, "ds"), stream_id, Symbol::new(env, "dep2"))
}

fn dual_stream_withdrawn2_key(env: &Env, stream_id: u64) -> (Symbol, u64, Symbol) {
    (Symbol::new(env, "ds"), stream_id, Symbol::new(env, "wd2"))
}

/// Gets the second token address for a dual stream.
pub fn get_dual_stream_token2(env: &Env, stream_id: u64) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&dual_stream_token2_key(env, stream_id))
}

/// Sets the second token address for a dual stream.
pub fn set_dual_stream_token2(env: &Env, stream_id: u64, token2: &Address) {
    env.storage()
        .persistent()
        .set(&dual_stream_token2_key(env, stream_id), token2);
}

/// Removes the second token address (called on stream completion/cancellation).
pub fn remove_dual_stream_token2(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&dual_stream_token2_key(env, stream_id));
}

/// Gets the second token deposit for a dual stream (in stroops).
pub fn get_dual_stream_deposit2(env: &Env, stream_id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&dual_stream_deposit2_key(env, stream_id))
        .unwrap_or(0i128)
}

/// Sets the second token deposit for a dual stream.
pub fn set_dual_stream_deposit2(env: &Env, stream_id: u64, deposit2: i128) {
    env.storage()
        .persistent()
        .set(&dual_stream_deposit2_key(env, stream_id), &deposit2);
}

/// Removes the second token deposit (called on stream completion/cancellation).
pub fn remove_dual_stream_deposit2(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&dual_stream_deposit2_key(env, stream_id));
}

/// Gets the total amount withdrawn from the second token.
pub fn get_dual_stream_withdrawn2(env: &Env, stream_id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&dual_stream_withdrawn2_key(env, stream_id))
        .unwrap_or(0i128)
}

/// Sets the total amount withdrawn from the second token.
pub fn set_dual_stream_withdrawn2(env: &Env, stream_id: u64, withdrawn2: i128) {
    env.storage()
        .persistent()
        .set(&dual_stream_withdrawn2_key(env, stream_id), &withdrawn2);
}

/// Increments the total amount withdrawn from the second token.
pub fn increment_dual_stream_withdrawn2(env: &Env, stream_id: u64, amount: i128) -> Result<(), crate::errors::StreamError> {
    let current = get_dual_stream_withdrawn2(env, stream_id);
    let new = current.checked_add(amount).ok_or(crate::errors::StreamError::Overflow)?;
    set_dual_stream_withdrawn2(env, stream_id, new);
    Ok(())
}

/// Removes the second token withdrawn counter (called on stream completion/cancellation).
pub fn remove_dual_stream_withdrawn2(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&dual_stream_withdrawn2_key(env, stream_id));
}

/// Cleans up all dual-stream storage entries for a stream.
pub fn cleanup_dual_stream_storage(env: &Env, stream_id: u64) {
    remove_dual_stream_token2(env, stream_id);
    remove_dual_stream_deposit2(env, stream_id);
    remove_dual_stream_withdrawn2(env, stream_id);
}

// ═══════════════════════════════════════════════════════════════════════════
// Expiry warning emitted flag (moved from Stream struct)
// ═══════════════════════════════════════════════════════════════════════════

fn expiry_warning_emitted_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "exp_em"), stream_id)
}

/// Returns whether the expiry warning event has already been emitted for a stream.
pub fn get_expiry_warning_emitted(env: &Env, stream_id: u64) -> bool {
    env.storage()
        .persistent()
        .get(&expiry_warning_emitted_key(env, stream_id))
        .unwrap_or(false)
}

/// Sets the expiry warning emitted flag for a stream.
pub fn set_expiry_warning_emitted(env: &Env, stream_id: u64, val: bool) {
    env.storage()
        .persistent()
        .set(&expiry_warning_emitted_key(env, stream_id), &val);
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream tag (moved from Stream struct)
// ═══════════════════════════════════════════════════════════════════════════

fn stream_tag_key(env: &Env, stream_id: u64) -> (Symbol, u64) {
    (Symbol::new(env, "stag"), stream_id)
}

/// Returns the tag for a stream, or None if not set.
pub fn get_stream_tag(env: &Env, stream_id: u64) -> Option<String> {
    env.storage()
        .persistent()
        .get(&stream_tag_key(env, stream_id))
}

/// Sets the tag for a stream.
pub fn set_stream_tag_storage(env: &Env, stream_id: u64, tag: &String) {
    env.storage()
        .persistent()
        .set(&stream_tag_key(env, stream_id), tag);
}

/// Removes the tag for a stream.
pub fn remove_stream_tag(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&stream_tag_key(env, stream_id));
}

// ═══════════════════════════════════════════════════════════════════════════
// WASM Upgrade Proposal Queue (Issue #497)
// ═══════════════════════════════════════════════════════════════════════════

fn upgrade_proposal_key(env: &Env) -> Symbol {
    Symbol::new(env, "upg_prop")
}

fn upgrade_proposal_expiry_key(env: &Env) -> Symbol {
    Symbol::new(env, "upg_exp")
}

#[derive(Clone, Debug)]
pub struct UpgradeProposal {
    pub wasm_hash: BytesN<32>,
    pub proposed_by: Address,
    pub created_at: u64,
}

pub fn propose_upgrade(
    env: &Env,
    wasm_hash: BytesN<32>,
    proposed_by: &Address,
    expiry_ledger: u64,
) -> Result<(), &'static str> {
    if get_pending_upgrade_proposal(env).is_some() {
        return Err("Upgrade proposal already pending");
    }

    let proposal = UpgradeProposal {
        wasm_hash,
        proposed_by: proposed_by.clone(),
        created_at: env.ledger().timestamp(),
    };

    env.storage()
        .instance()
        .set(&upgrade_proposal_key(env), &(proposal.wasm_hash.clone(), proposal.proposed_by.clone(), proposal.created_at));
    env.storage()
        .instance()
        .set(&upgrade_proposal_expiry_key(env), &expiry_ledger);
    Ok(())
}

pub fn get_pending_upgrade_proposal(env: &Env) -> Option<(BytesN<32>, Address, u64)> {
    env.storage()
        .instance()
        .get(&upgrade_proposal_key(env))
}

pub fn get_upgrade_proposal_expiry(env: &Env) -> Option<u64> {
    env.storage()
        .instance()
        .get(&upgrade_proposal_expiry_key(env))
}

pub fn approve_upgrade_proposal(env: &Env, approver: &Address) -> Result<BytesN<32>, &'static str> {
    let proposal = get_pending_upgrade_proposal(env)
        .ok_or("No pending upgrade proposal")?;

    let expiry = get_upgrade_proposal_expiry(env)
        .ok_or("Proposal expiry not set")?;

    if u64::from(env.ledger().sequence()) > expiry {
        clear_upgrade_proposal(env);
        return Err("Upgrade proposal expired");
    }

    env.storage()
        .instance()
        .remove(&upgrade_proposal_key(env));
    env.storage()
        .instance()
        .remove(&upgrade_proposal_expiry_key(env));

    Ok(proposal.0)
}

pub fn clear_upgrade_proposal(env: &Env) {
    env.storage()
        .instance()
        .remove(&upgrade_proposal_key(env));
    env.storage()
        .instance()
        .remove(&upgrade_proposal_expiry_key(env));
}

// ── Cancellation fee ─────────────────────────────────────────────────────────
const CANCELLATION_FEE_KEY: &str = "cancel_fee";

pub fn get_cancellation_fee_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, CANCELLATION_FEE_KEY))
        .unwrap_or(0)
}

pub fn set_cancellation_fee_bps(env: &Env, bps: i128) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, CANCELLATION_FEE_KEY), &bps);
}

// ── Stake balance ─────────────────────────────────────────────────────────────
pub fn get_stake_balance(env: &Env, staker: &Address, token: &Address) -> i128 {
    let key = (Symbol::new(env, "stk"), staker.clone(), token.clone());
    env.storage()
        .persistent()
        .get::<_, i128>(&key)
        .unwrap_or(0)
}

pub fn add_stake_balance(env: &Env, staker: &Address, token: &Address, amount: i128) {
    let key = (Symbol::new(env, "stk"), staker.clone(), token.clone());
    let current: i128 = env.storage().persistent().get::<_, i128>(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current + amount));
}

pub fn sub_stake_balance(env: &Env, staker: &Address, token: &Address, amount: i128) {
    let key = (Symbol::new(env, "stk"), staker.clone(), token.clone());
    let current: i128 = env.storage().persistent().get::<_, i128>(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current - amount));
}

// ── Minimum stake ─────────────────────────────────────────────────────────────
const MIN_STAKE_KEY: &str = "min_stake";

pub fn get_min_stake(env: &Env, token: &Address) -> i128 {
    let key = (Symbol::new(env, MIN_STAKE_KEY), token.clone());
    env.storage()
        .instance()
        .get::<_, i128>(&key)
        .unwrap_or(0)
}

pub fn set_min_stake(env: &Env, token: &Address, amount: i128) {
    let key = (Symbol::new(env, MIN_STAKE_KEY), token.clone());
    env.storage().instance().set(&key, &amount);
}

/// Lock-up period in seconds before an unstake request can be completed (7 days).
pub const STAKE_UNLOCK_DELAY: u64 = 7 * 24 * 60 * 60;

// ── Per-sender active stream cap (feat/37-sender-stream-cap) ─────────────────
//
// The cap limits the number of *active* streams a single sender address may
// hold at once.  Unlike the existing `get_max_streams_per_sender` (which tracks
// lifetime count using a global default), this cap tracks the *current active*
// count in a dedicated persistent key and can be overridden globally by the
// admin.  When the cap is reached, `create_stream` panics with
// `SenderStreamCapReached`.  The count is decremented on cancellation or
// natural expiry/completion.

const SENDER_STREAM_CAP_KEY: &str = "ss_cap";

fn sender_active_count_key(env: &Env, sender: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sac"), sender.clone())
}

/// Returns the global per-sender active stream cap (default 1000).
pub fn get_sender_stream_cap(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, SENDER_STREAM_CAP_KEY))
        .unwrap_or(1_000u32)
}

/// Sets the global per-sender active stream cap.  Admin-only via contract method.
pub fn set_sender_stream_cap(env: &Env, cap: u32) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, SENDER_STREAM_CAP_KEY), &cap);
}

/// Returns the current number of *active* streams for `sender`.
pub fn get_sender_active_count(env: &Env, sender: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&sender_active_count_key(env, sender))
        .unwrap_or(0u32)
}

/// Increments the active stream count for `sender` by 1.
pub fn increment_sender_active_count(env: &Env, sender: &Address) {
    let key = sender_active_count_key(env, sender);
    let current = get_sender_active_count(env, sender);
    env.storage()
        .persistent()
        .set(&key, &current.saturating_add(1));
}

/// Decrements the active stream count for `sender` by 1 (saturates at 0).
pub fn decrement_sender_active_count(env: &Env, sender: &Address) {
    let key = sender_active_count_key(env, sender);
    let current = get_sender_active_count(env, sender);
    if current > 0 {
        env.storage().persistent().set(&key, &(current - 1));
    }
}
