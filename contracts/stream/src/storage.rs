use crate::types::{AuditEntry, Stream};
use soroban_sdk::{Address, Bytes, Env, Symbol, Vec, xdr::ToXdr};

const ADMIN_KEY: &str = "admin";
const PAUSED_KEY: &str = "paused";
const PROTOCOL_FEE_KEY: &str = "fee_bps";
const TREASURY_KEY: &str = "treasury";
const MIN_DURATION_KEY: &str = "min_dur";
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
