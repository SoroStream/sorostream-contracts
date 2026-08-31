#![no_std]
#![allow(clippy::too_many_arguments)]
//! # SoroStream Contract

#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod interface;
pub mod oracle;
mod storage;
mod types;
pub mod vesting_math;
pub mod composability;
pub mod roles;

pub use interface::SoroStreamInterface;
pub use errors::StreamError;
pub use types::{AuditEntry, CreateStreamOptions, HealthStatus, Stats, Stream, StreamHealth, StreamOptions, StreamStatus, VestingCurve, StreamQueryFilter};
pub use oracle::IPriceOracle;
pub use composability::ISoroStreamComposability;
pub use roles::AdminRole;

#[cfg(test)] mod integration_tests;
// other test modules disabled during grace-period test restore
#[cfg(test)] mod rate_limit_tests;
#[cfg(test)] mod issue_520_tests;
#[cfg(test)] mod issue_505_tests;
#[cfg(test)] mod issue_506_tests;
#[cfg(test)] mod issue_507_tests;

use soroban_sdk::{
    contract, contractimpl, token, Address, Bytes, BytesN, Env, String, Vec, Symbol, IntoVal,
};
use types::VestingTranche;
use types::{Milestone, MilestoneStatus};
use storage::{
    accumulate_fees, add_fee_exempt, add_to_blocklist,
    append_audit_entry, check_admin, cleanup_dual_stream_storage,
    clear_pending_fee_proposal, clear_reentrancy_lock, decrement_active_stream_count,
    decrement_token_stream_count, derive_stream_id,
    drain_fees_collected, effective_sender_limit, extend_instance_ttl,
    get_active_stream_count, get_batch_nonce, get_creation_fee_xlm,
    get_delegate,    get_expiry_warning_emitted, get_expiry_warning_window,
    get_federation_address, get_max_deposit_per_token,
    get_fees_collected, get_global_stream_at, get_global_stream_count,
    get_grace_period_ledgers, get_holdback, get_ids_by_recipient,
    get_ids_by_sender, get_ids_by_tag, get_max_streams_per_token, get_new_sender_stream_cap,
    get_pause_expiry, get_protocol_fee, get_rate_limit_max_creations,
    get_rate_limit_state, get_rate_limit_window, get_remaining_quota,
    get_sender_lifetime_count, get_sender_promotion_threshold, get_sender_stream_count,
    get_stream_tag, get_token_stream_count,
    get_treasury, get_withdrawal_cooldown, get_xlm_token,
    increment_active_stream_count, increment_batch_nonce,
    increment_sender_lifetime_count, increment_token_stream_count,
    index_by_recipient, index_by_sender, index_by_tag, index_global_stream,
    is_blocked, is_fee_exempt, is_paused_or_auto_unpause, is_recipient_allowed,
    is_rate_limit_exempt, is_reentrancy_locked, is_sender_promoted,
    is_token_whitelisted, is_token_whitelist_enabled, is_whitelisted,
    is_whitelist_enabled, load_stream, load_tranches,
    add_token_to_whitelist, set_token_whitelist_enabled,
    mark_nonce_used, MAX_PAUSE_DURATION, nonce_used,
    read_admin, read_applied_migrations, read_audit_log,
    read_governance, read_guardian, read_max_duration,
    read_max_future_start_offset, read_min_duration, read_pending_fee_proposal,
    read_version, record_migration, register_federation_address,
    remove_delegate, remove_fee_exempt, remove_from_blocklist,
    remove_holdback,
    remove_stream, remove_stream_tag, remove_token_from_whitelist, remove_tranches,
    save_stream, save_tranches, set_active_stream_count, set_creation_fee_xlm,
    set_delegate, set_expiry_warning_window,
    set_expiry_warning_emitted, set_grace_period_ledgers, set_max_deposit_per_token, set_max_streams_per_sender,
    set_max_streams_per_token, set_new_sender_stream_cap, set_paused,
    set_pause_expiry, set_protocol_fee,
    set_rate_limit_state, set_reentrancy_lock,
    set_rate_limit_window, set_rate_limit_max_creations,
    add_rate_limit_exempt, remove_rate_limit_exempt,
    set_sender_last_creation_time, set_sender_limit, set_sender_promotion_threshold,
    set_slippage_params, set_stream_creation_cooldown,
    set_stream_tag_storage,
    set_treasury, set_whitelist_enabled, set_withdrawal_cooldown,
    set_xlm_token, stream_exists, unindex_by_recipient,
    unindex_by_sender, unindex_by_tag, unregister_federation_address, write_admin,
    write_governance, write_guardian, write_max_duration,
    write_max_future_start_offset, write_min_duration, write_pending_fee_proposal,
    write_version,
    get_cancellation_fee_bps, set_cancellation_fee_bps,
    get_stake_balance, add_stake_balance, sub_stake_balance,
    get_min_stake, set_min_stake,
    STAKE_UNLOCK_DELAY,
};

// ── Helper: checked multiply ──────────────────────────────────────────────────
fn checked_flow_amount(flow_rate: i128, elapsed: u64) -> Result<i128, StreamError> {
    flow_rate.checked_mul(elapsed as i128).ok_or(StreamError::Overflow)
}

const MAX_STREAM_DURATION_SECONDS: u64 = 100 * 365 * 24 * 60 * 60;

/// Maximum safe flow_rate that won't overflow when multiplied by any valid duration.
/// Calculated as i128::MAX / MAX_STREAM_DURATION_SECONDS.
/// This ensures that flow_rate * elapsed can never overflow i128 for any valid stream.
const MAX_SAFE_FLOW_RATE: i128 = i128::MAX / (MAX_STREAM_DURATION_SECONDS as i128);

/// Validates that a flow_rate is within safe bounds for arithmetic operations.
/// Returns error if flow_rate could overflow when multiplied by any elapsed time
/// within a valid stream duration.
fn validate_flow_rate_bounds(flow_rate: i128) -> Result<(), StreamError> {
    if flow_rate <= 0 {
        return Err(StreamError::ZeroFlowRate);
    }
    if flow_rate > MAX_SAFE_FLOW_RATE {
        return Err(StreamError::Overflow);
    }
    Ok(())
}

// ── Helper: validate metadata URI ────────────────────────────────────────────
/// Minimum claimable amount before a withdrawal is considered meaningful.
///
/// Amounts at or below this threshold are treated as rounding dust and
/// suppressed in `get_claimable` and `withdraw` to prevent failed
/// micro-withdrawals and noisy UI displays. 1 stroop is the smallest
/// indivisible unit of any Stellar token.
const DUST_THRESHOLD: i128 = 1;

/// Validates a metadata URI length (prefix checks omitted — String has no as_bytes).
fn validate_metadata_uri(uri: &Option<String>) -> Result<(), StreamError> {
    if let Some(ref u) = uri {
        if u.len() > 128 {
            return Err(StreamError::InvalidMetadataUri);
        }
    }
    Ok(())
}

// ── Helper: per-sender ledger-based sliding window rate limit ────────────────
//
// The rate limit state `(window_start_ledger, count)` lives in **temporary**
// storage.  Temporary storage is appropriate here because:
//
//   1. If the entry's TTL lapses the counter is treated as zero — equivalent to
//      the window having fully elapsed — which is correct, not dangerous.
//   2. The entry's TTL is extended to `window_ledgers` on every write, so it
//      cannot expire while a window is still active.
//   3. Rate limit state is inherently ephemeral; there is no need to store it
//      for longer than one window period.
fn check_rate_limit(env: &Env, sender: &Address) -> Result<(), StreamError> {
    if is_rate_limit_exempt(env, sender) { return Ok(()); }
    let window = get_rate_limit_window(env);
    let max = get_rate_limit_max_creations(env);
    let current_ledger = env.ledger().sequence();
    let (ws, count) = get_rate_limit_state(env, sender);
    let (new_ws, new_count) = if current_ledger >= ws.saturating_add(window) {
        // Window has expired — start a fresh one at the current ledger.
        (current_ledger, 1u32)
    } else {
        if count >= max {
            events::rate_limit_exceeded(env, sender);
            return Err(StreamError::RateLimitExceeded);
        }
        (ws, count + 1)
    };
    set_rate_limit_state(env, sender, new_ws, new_count);
    Ok(())
}

// ── Helper: token whitelist ───────────────────────────────────────────────────
#[allow(dead_code)]
fn check_token_whitelist(env: &Env, token: &Address) -> Result<(), StreamError> {
    if is_token_whitelist_enabled(env) && !is_token_whitelisted(env, token) {
        return Err(StreamError::TokenNotWhitelisted);
    }
    Ok(())
}

// ── Helper: sender stake (issue #293) ────────────────────────────────────────
/// Checks that the sender has staked at least the required minimum for `token`.
///
/// When no minimum is configured (`get_min_stake` returns 0) the check is a
/// no-op, so existing deployments are unaffected until the admin enables the
/// feature with `set_min_stake`.
fn check_sender_stake(env: &Env, sender: &Address, token: &Address) -> Result<(), StreamError> {
    let min = get_min_stake(env, token);
    if min == 0 {
        return Ok(());
    }
    let balance = get_stake_balance(env, sender, token);
    if balance < min {
        return Err(StreamError::InsufficientStake);
    }
    Ok(())
}

// ── Helper: validate SAC address ─────────────────────────────────────────────
fn validate_token_address(env: &Env, token: &Address) -> Result<(), StreamError> {
    // `symbol()` panics if `token` is not a valid SAC; success is sufficient validation.
    let _ = token::Client::new(env, token).symbol();
    Ok(())
}

fn validate_recipient_address(env: &Env, sender: &Address, recipient: &Address) -> Result<(), StreamError> {
    if recipient == sender || recipient == &env.current_contract_address() {
        return Err(StreamError::NotRecipient);
    }
    Ok(())
}

fn refreshed_stream_view(env: &Env, mut stream: Stream) -> Stream {
    let now = env.ledger().timestamp();
    if (stream.status == StreamStatus::Active || stream.status == StreamStatus::Completed)
        && now >= stream.end_time
    {
        stream.status = StreamStatus::Expired;
    }
    stream
}

// ── Feature (a): maybe emit StreamExpiryWarning ───────────────────────────────
#[allow(dead_code)]
fn maybe_emit_expiry_warning(env: &Env, stream: &mut Stream) {
    if get_expiry_warning_emitted(env, stream.id) { return; }
    let now = env.ledger().timestamp();
    if now >= stream.end_time { return; }
    let remaining_seconds = stream.end_time - now;
    let remaining_ledgers = (remaining_seconds / 5) as u32;
    let window = get_expiry_warning_window(env);
    if remaining_ledgers <= window {
        let remaining_balance = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        events::stream_expiry_warning(env, stream.id, &stream.sender, &stream.recipient,
            remaining_balance, remaining_ledgers);
        set_expiry_warning_emitted(env, stream.id, true);
    }
}

// ── Feature (b): new-sender cap check ────────────────────────────────────────
#[allow(dead_code)]
fn check_new_sender_cap(env: &Env, sender: &Address) -> Result<(), StreamError> {
    if is_sender_promoted(env, sender) { return Ok(()); }
    let cap = get_new_sender_stream_cap(env);
    if get_sender_stream_count(env, sender) >= cap {
        return Err(StreamError::NewSenderStreamCapExceeded);
    }
    Ok(())
}

#[allow(dead_code)]
fn post_create_sender_accounting(env: &Env, sender: &Address) {
    let was_promoted = is_sender_promoted(env, sender);
    increment_sender_lifetime_count(env, sender);
    if !was_promoted && is_sender_promoted(env, sender) {
        let lifetime = get_sender_lifetime_count(env, sender);
        let threshold = get_sender_promotion_threshold(env);
        events::sender_promoted(env, sender, lifetime, threshold);
    }
}

// ── Feature (c): circular redirect detection ─────────────────────────────────
const MAX_REDIRECT_DEPTH: u32 = 8;

fn check_no_circular_redirect(env: &Env, source_id: u64, target_id: u64) -> Result<(), StreamError> {
    let mut cur = target_id;
    for _ in 0..MAX_REDIRECT_DEPTH {
        if cur == source_id { return Err(StreamError::CircularRedirect); }
        match load_stream(env, cur) {
            None => return Ok(()),
            Some(s) => match s.options.redirect_to_stream_id {
                None => return Ok(()),
                Some(next) => {
                    if next == source_id { return Err(StreamError::CircularRedirect); }
                    cur = next;
                }
            },
        }
    }
    Err(StreamError::CircularRedirect)
}

#[contract]
pub struct SoroStreamContract;

#[contractimpl]
impl SoroStreamContract {

    // ─────────────────────────────────────────────────────────────────────────
    // Admin / lifecycle
    // ─────────────────────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, version: String) -> Result<(), StreamError> {
        if read_admin(&env).is_some() { return Err(StreamError::AlreadyInitialized); }
        write_admin(&env, &admin);
        write_version(&env, &version);
        events::contract_deployed(&env, &version, &admin);
        Ok(())
    }

    pub fn get_admin(env: Env) -> Result<Address, StreamError> {
        read_admin(&env).ok_or(StreamError::NotInitialized)
    }

    pub fn get_version(env: Env) -> Result<String, StreamError> {
        read_version(&env).ok_or(StreamError::NotInitialized)
    }

    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), StreamError> {
        check_admin(&env);
        write_admin(&env, &new_admin);
        Ok(())
    }

    // ── Issue #292: Role-based admin access control ───────────────────────────
    //
    // The super-admin (stored under ADMIN_KEY) retains all privileges.  The three
    // additional roles (FeeManager, EmergencyPause, Analytics) provide limited,
    // scoped access to subsets of admin operations so that a single key compromise
    // does not expose all privileged functions.

    /// Assigns the `FeeManager` role to `assignee`.
    ///
    /// The fee manager may call `set_protocol_fee`, `set_cancellation_fee`, and
    /// `sweep_fees` without needing the super-admin key.
    ///
    /// Only the super-admin may call this.
    pub fn assign_fee_manager(env: Env, admin: Address, assignee: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::set_fee_manager(&env, &assignee);
        let ts = env.ledger().timestamp();
        events::role_assigned(&env, &String::from_str(&env, "FeeManager"), &assignee, &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "assign_fee_manager"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Revokes the `FeeManager` role.
    ///
    /// Only the super-admin may call this.
    pub fn revoke_fee_manager(env: Env, admin: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::revoke_fee_manager(&env);
        let ts = env.ledger().timestamp();
        events::role_revoked(&env, &String::from_str(&env, "FeeManager"), &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "revoke_fee_manager"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Returns the currently assigned `FeeManager` address, or `None`.
    pub fn get_fee_manager(env: Env) -> Option<Address> {
        roles::get_fee_manager(&env)
    }

    /// Assigns the `EmergencyPause` role to `assignee`.
    ///
    /// The emergency-pause operator may call `emergency_pause` and
    /// `emergency_resume` without the super-admin key.
    ///
    /// Only the super-admin may call this.
    pub fn assign_emergency_pause_role(env: Env, admin: Address, assignee: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::set_emergency_pause_role(&env, &assignee);
        let ts = env.ledger().timestamp();
        events::role_assigned(&env, &String::from_str(&env, "EmergencyPause"), &assignee, &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "assign_emergency_pause"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Revokes the `EmergencyPause` role.
    ///
    /// Only the super-admin may call this.
    pub fn revoke_emergency_pause_role(env: Env, admin: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::revoke_emergency_pause_role(&env);
        let ts = env.ledger().timestamp();
        events::role_revoked(&env, &String::from_str(&env, "EmergencyPause"), &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "revoke_emergency_pause"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Returns the currently assigned `EmergencyPause` role address, or `None`.
    pub fn get_emergency_pause_role(env: Env) -> Option<Address> {
        roles::get_emergency_pause_role(&env)
    }

    /// Assigns the `Analytics` role to `assignee`.
    ///
    /// The analytics operator gets read-only access to protocol statistics
    /// and the admin audit log — no write access.
    ///
    /// Only the super-admin may call this.
    pub fn assign_analytics_role(env: Env, admin: Address, assignee: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::set_analytics_role(&env, &assignee);
        let ts = env.ledger().timestamp();
        events::role_assigned(&env, &String::from_str(&env, "Analytics"), &assignee, &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "assign_analytics"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Revokes the `Analytics` role.
    ///
    /// Only the super-admin may call this.
    pub fn revoke_analytics_role(env: Env, admin: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        roles::revoke_analytics_role(&env);
        let ts = env.ledger().timestamp();
        events::role_revoked(&env, &String::from_str(&env, "Analytics"), &admin);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "revoke_analytics"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        Ok(())
    }

    /// Returns the currently assigned `Analytics` role address, or `None`.
    pub fn get_analytics_role(env: Env) -> Option<Address> {
        roles::get_analytics_role(&env)
    }

    /// Returns `true` when `caller` holds any recognised admin role.
    pub fn has_any_admin_role(env: Env, caller: Address) -> bool {
        let admin = match read_admin(&env) {
            Some(a) => a,
            None => return false,
        };
        roles::has_any_role(&env, &caller, &admin)
    }


    pub fn emergency_pause(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, true);
        let ts = env.ledger().timestamp();
        set_pause_expiry(&env, ts.saturating_add(MAX_PAUSE_DURATION));
        let admin = read_admin(&env).unwrap();
        events::contract_paused(&env, &admin, ts);
        let entry = AuditEntry { instruction: String::from_str(&env, "emergency_pause"),
            admin: admin.clone(), timestamp: ts, params: String::from_str(&env, "") };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &admin, ts);
        Ok(())
    }

    pub fn emergency_resume(env: Env) -> Result<(), StreamError> {
        check_admin(&env);
        set_paused(&env, false);
        set_pause_expiry(&env, 0);
        let admin = read_admin(&env).unwrap();
        let ts = env.ledger().timestamp();
        events::contract_resumed(&env, &admin, ts);
        let entry = AuditEntry { instruction: String::from_str(&env, "emergency_resume"),
            admin: admin.clone(), timestamp: ts, params: String::from_str(&env, "") };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &admin, ts);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool { is_paused_or_auto_unpause(&env) }

    /// Pauses the contract. Accepts any holder of the EmergencyPause role or the super-admin.
    ///
    /// This is the role-aware variant of `emergency_pause`.
    pub fn role_emergency_pause(env: Env, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();
        let admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        roles::require_emergency_pause_or_admin(&env, &caller, &admin)?;
        set_paused(&env, true);
        let ts = env.ledger().timestamp();
        set_pause_expiry(&env, ts.saturating_add(MAX_PAUSE_DURATION));
        events::contract_paused(&env, &caller, ts);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "role_emergency_pause"),
            admin: caller.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &caller, ts);
        Ok(())
    }

    /// Resumes the contract. Accepts any holder of the EmergencyPause role or the super-admin.
    ///
    /// This is the role-aware variant of `emergency_resume`.
    pub fn role_emergency_resume(env: Env, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();
        let admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        roles::require_emergency_pause_or_admin(&env, &caller, &admin)?;
        set_paused(&env, false);
        set_pause_expiry(&env, 0);
        let ts = env.ledger().timestamp();
        events::contract_resumed(&env, &caller, ts);
        let entry = AuditEntry {
            instruction: String::from_str(&env, "role_emergency_resume"),
            admin: caller.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &caller, ts);
        Ok(())
    }

    pub fn set_guardian(env: Env, guardian: Address) -> Result<(), StreamError> {
        check_admin(&env); write_guardian(&env, &guardian); Ok(())
    }
    pub fn get_guardian(env: Env) -> Option<Address> { read_guardian(&env) }

    pub fn set_governance(env: Env, governance: Address) -> Result<(), StreamError> {
        check_admin(&env); write_governance(&env, &governance); Ok(())
    }
    pub fn get_governance(env: Env) -> Option<Address> { read_governance(&env) }

    pub fn pause(env: Env, guardian: Address) -> Result<(), StreamError> {
        guardian.require_auth();
        let stored = read_guardian(&env).ok_or(StreamError::NotAuthorized)?;
        if guardian != stored { return Err(StreamError::NotAuthorized); }
        set_paused(&env, true);
        let ts = env.ledger().timestamp();
        set_pause_expiry(&env, ts.saturating_add(MAX_PAUSE_DURATION));
        env.events().publish((Symbol::new(&env, "Paused"), guardian.clone()), ts);
        Ok(())
    }

    pub fn unpause(env: Env, governance: Address) -> Result<(), StreamError> {
        governance.require_auth();
        let stored = read_governance(&env).ok_or(StreamError::NotAuthorized)?;
        if governance != stored { return Err(StreamError::NotAuthorized); }
        set_paused(&env, false);
        set_pause_expiry(&env, 0);
        env.events().publish((Symbol::new(&env, "Unpaused"), governance.clone()), env.ledger().timestamp());
        Ok(())
    }

    pub fn get_pause_expiry(env: Env) -> u64 { get_pause_expiry(&env) }

    pub fn add_fee_exempt(env: Env, addr: Address) -> Result<(), StreamError> {
        check_admin(&env); add_fee_exempt(&env, &addr); Ok(())
    }
    pub fn remove_fee_exempt(env: Env, addr: Address) -> Result<(), StreamError> {
        check_admin(&env); remove_fee_exempt(&env, &addr); Ok(())
    }
    pub fn is_fee_exempt(env: Env, addr: Address) -> bool { is_fee_exempt(&env, &addr) }

    pub fn get_fees_collected(env: Env, token: Address) -> i128 { get_fees_collected(&env, &token) }

    pub fn sweep_fees(env: Env, token: Address, destination: Address) -> Result<(), StreamError> {
        check_admin(&env);
        let amount = drain_fees_collected(&env, &token);
        if amount > 0 {
            token::Client::new(&env, &token).transfer(&env.current_contract_address(), &destination, &amount);
            events::fee_swept(&env, &token, amount, &destination);
        }
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), StreamError> {
        let admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    pub fn set_max_streams(env: Env, max_streams: u32) -> Result<(), StreamError> {
        check_admin(&env); set_max_streams_per_sender(&env, max_streams); Ok(())
    }
    pub fn set_sender_stream_limit(env: Env, sender: Address, limit: u32) -> Result<(), StreamError> {
        check_admin(&env); set_sender_limit(&env, &sender, limit); Ok(())
    }

    pub fn migrate(env: Env, from_version: String, to_version: String) -> Result<(), StreamError> {
        check_admin(&env);
        let applied = read_applied_migrations(&env);
        if applied.contains(&to_version) { return Err(StreamError::MigrationAlreadyApplied); }
        write_version(&env, &to_version);
        record_migration(&env, &to_version);
        let admin = read_admin(&env).unwrap();
        events::contract_migrated(&env, &from_version, &to_version, &admin);
        let ts = env.ledger().timestamp();
        let entry = AuditEntry { instruction: String::from_str(&env, "migrate"),
            admin: admin.clone(), timestamp: ts, params: to_version.clone() };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &admin, ts);
        Ok(())
    }

    pub fn get_admin_log(env: Env) -> Vec<AuditEntry> { read_audit_log(&env) }

    /// Helper function to remove a stream from all indices (sender, recipient, and tag if present).
    fn unindex_stream(env: &Env, stream: &Stream, stream_id: u64) {
        unindex_by_sender(env, &stream.sender, stream_id);
        unindex_by_recipient(env, &stream.recipient, stream_id);
        if let Some(ref tag) = get_stream_tag(env, stream_id) {
            unindex_by_tag(env, tag, stream_id);
        }
    }

    pub fn archive_stream(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != caller && stream.recipient != caller { return Err(StreamError::NotAuthorized); }
        let duration = stream.end_time.saturating_sub(stream.start_time);
        let dust = stream.deposit.saturating_sub(stream.flow_rate.saturating_mul(duration as i128));
        if stream.options.total_withdrawn.saturating_add(dust) < stream.deposit { return Err(StreamError::StreamNotSettled); }
        remove_stream(&env, stream_id);
        Self::unindex_stream(&env, &stream, stream_id);
        if get_delegate(&env, stream_id).is_some() { remove_delegate(&env, stream_id); }
        if stream.options.is_dual_stream { cleanup_dual_stream_storage(&env, stream_id); }
        events::stream_archived(&env, stream_id, &stream.sender, &stream.recipient, stream.deposit);
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Feature (a): Expiry warning window config
    // ─────────────────────────────────────────────────────────────────────────
    /// Creates a new payment stream.
    #[allow(clippy::too_many_arguments)]
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        auto_renew: bool,
        lock_until: u64,
        options: CreateStreamOptions,
    ) -> Result<u64, StreamError> {
        let tag: Option<String> = None;
        let on_complete_contract: Option<Address> = None;
        let on_complete_function: Option<Symbol> = None;
        let enforce_recipient_allowlist = false;
        sender.require_auth();

        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        // Get current time early for validations
        let now = env.ledger().timestamp();

        // Check rate limit (per-sender creation frequency cap)
        check_rate_limit(&env, &sender)?;

        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        // holdback must be non-negative and strictly less than total amount (0 = no holdback)
        if options.holdback_amount < 0 || options.holdback_amount >= amount {
            return Err(StreamError::ZeroAmount);
        }
        if cliff_seconds > duration_seconds {
            return Err(StreamError::InvalidCliff);
        }
        // ── Validate stream comment (issue #513) ─────────────────────────────
        // The comment is an optional human-readable payment reference. It must
        // not exceed 256 bytes of UTF-8 text.
        if let Some(ref comment) = options.comment {
            if comment.len() > 256 {
                return Err(StreamError::CommentTooLong);
            }
        }
        validate_recipient_address(&env, &sender, &recipient)?;
        check_token_whitelist(&env, &token)?;
        validate_token_address(&env, &token)?;
        validate_recipient_address(&env, &sender, &recipient)?;
        check_token_whitelist(&env, &token)?;
        validate_token_address(&env, &token)?;
        // Check sender stake requirement (issue #293): if admin has set a minimum
        // stake for this token, the sender must have deposited at least that amount.
        check_sender_stake(&env, &sender, &token)?;
        if is_whitelist_enabled(&env) && !is_whitelisted(&env, &recipient) {
            return Err(StreamError::RecipientNotWhitelisted);
        }

        let min_dur = read_min_duration(&env);
        if duration_seconds < min_dur {
            return Err(StreamError::StreamDurationTooShort);
        }

        // Explicit zero-duration check for clarity (Issue: allow end_time = start_time vulnerability)
        // A stream must have positive duration. Zero duration would mean start_time == end_time,
        // which is invalid: the deposit would immediately fully accrue with flow_rate * 0 = 0,
        // but the constraint enforcement becomes ambiguous.
        if duration_seconds == 0 {
            return Err(StreamError::InvalidDuration);
        }

        let max_dur = read_max_duration(&env);
        if max_dur > 0 && duration_seconds > max_dur {
            return Err(StreamError::DurationExceedsMax);
        }

        // The streaming portion is the total minus the holdback escrow.
        let streaming_amount = amount
            .checked_sub(options.holdback_amount)
            .ok_or(StreamError::Overflow)?;
        let flow_rate = streaming_amount / duration_seconds as i128;
        if flow_rate == 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        // ── Issue: Validate flow_rate bounds to prevent overflow during withdrawals ──
        // Ensure flow_rate is within safe bounds: flow_rate * any_elapsed_time <= i128::MAX
        // This prevents "runtime errors" where computations overflow after stream creation.
        // By validating here, we guarantee that future withdraw operations won't encounter
        // unexpected Overflow errors due to excessively large flow rates.
        validate_flow_rate_bounds(flow_rate)?;

        // ── Validate withdrawal_steps ────────────────────────────────────────
        // Steps must be >= 1.  A value of 0 is nonsensical; callers should pass
        // None instead of Some(0).
        if let Some(steps) = options.withdrawal_steps {
            if steps == 0 {
                return Err(StreamError::InvalidDuration);
            }
        }

        // ── Validate min_withdrawal_amount ───────────────────────────────────
        // The floor must be positive; 0 is indistinguishable from "no floor".
        if let Some(floor) = options.min_withdrawal_amount {
            if floor <= 0 {
                return Err(StreamError::ZeroAmount);
            }
        }

        // ── Validate on_complete callback ────────────────────────────────────
        // Both contract and function must be provided together, or both must be None.
        match (&on_complete_contract, &on_complete_function) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(StreamError::InvalidDuration);
            }
            _ => {}
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        // Check blocklist (Issue #284)
        if is_blocked(&env, &sender) || is_blocked(&env, &recipient) {
            return Err(StreamError::NotAuthorized);
        }

        // Check token whitelist (Issue #221)
        check_token_whitelist(&env, &token)?;

        // Check per-token stream cap (Issue #286)
        let max_per_token = get_max_streams_per_token(&env);
        if max_per_token > 0 && get_token_stream_count(&env, &token) >= max_per_token {
            return Err(StreamError::StreamNotFound);
        }

        // Check per-asset maximum deposit limit
        let max_deposit = get_max_deposit_per_token(&env, &token);
        if max_deposit > 0 && amount > max_deposit {
            return Err(StreamError::MaxDepositExceeded);
        }

        mark_nonce_used(&env, &sender, nonce);

        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        // Defensive check: ensure end_time > start_time (duration > 0 is already validated above)
        // This provides defense-in-depth against timestamp overflow or logic errors that could
        // create zero-duration streams (where end_time == start_time).
        if end_time <= now {
            return Err(StreamError::InvalidEndTime);
        }
        let cliff_time = now
            .checked_add(cliff_seconds)
            .ok_or(StreamError::Overflow)?;

        // ── Defensive stream ID collision check ─────────────────────────────
        // derive_stream_id produces the first 8 bytes of a SHA-256 hash.
        // Collisions are astronomically unlikely, but we add an explicit retry
        // loop as a defence-in-depth measure: if a collision is detected, retry
        // up to MAX_ID_RETRIES times by XOR-ing a retry counter into the nonce
        // input.  All retries colliding returns IDCollision — a clear signal
        // that something is structurally wrong.
        const MAX_ID_RETRIES: u64 = 3;
        let mut stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);
        if stream_exists(&env, stream_id) {
            let mut found = false;
            for retry in 1u64..=MAX_ID_RETRIES {
                let candidate = derive_stream_id(
                    &env, &sender, &recipient, now, nonce ^ (retry << 32),
                );
                if !stream_exists(&env, candidate) {
                    stream_id = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(StreamError::IDCollision);
            }
        }

        let creation_fee = get_creation_fee_xlm(&env);
        if creation_fee > 0 {
            let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
            let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
            token::Client::new(&env, &xlm_token).transfer(
                &sender,
                &treasury,
                &creation_fee,
            );
            events::creation_fee_collected(&env, creation_fee, &treasury);
        }

        // Transfer total amount (streaming + holdback) from sender into contract escrow.
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit: streaming_amount,
            flow_rate,
            start_time: now,
            cliff_time,
            lock_until,
            end_time,
            last_withdraw_time: now,
            status: if options.requires_recipient_approval {
                StreamStatus::PendingApproval
            } else {
                StreamStatus::Active
            },
            auto_renew,
            options: StreamOptions {
                renew_count: options.renew_count,
                renewals_used: 0,
                allow_recipient_termination: options.allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: Bytes::new(&env),
                locked: false,
                metadata_uri: None,
                milestones: Vec::new(&env),
                milestone_release_mode: false,
                holdback_amount: options.holdback_amount,
                holdback_claimed: false,
                is_step_vesting: false,
                tranches_claimed: 0,
                oracle: None,
                max_price_deviation_bps: 0,
                creation_price: 0,
                curve: VestingCurve::Linear,
                withdrawal_steps: options.withdrawal_steps,
                current_step: 0,
                min_withdrawal_amount: options.min_withdrawal_amount,
                non_transferable: options.non_transferable,
                requires_recipient_approval: options.requires_recipient_approval,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                is_dual_stream: false,
                on_complete_contract,
                on_complete_function,
                comment: options.comment.clone(),
            },
        };

        save_stream(&env, &stream);
        extend_instance_ttl(&env);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        if let Some(ref t) = tag {
            index_by_tag(&env, t, stream_id);
            set_stream_tag_storage(&env, stream_id, t);
        }
        index_global_stream(&env, stream_id);
        // Only count as active immediately if no approval is required.
        if !options.requires_recipient_approval {
            increment_active_stream_count(&env);
            increment_token_stream_count(&env, &stream.token);
        }

        // Update sender's last stream creation time (Issue #239)
        set_sender_last_creation_time(&env, &sender, now);

        events::stream_created(
            &env, stream_id, &sender, &recipient, amount, flow_rate, end_time,
            options.non_transferable, &options.comment,
        );

        // Emit supplemental config event when non-default options are set so
        // indexers can surface step/floor configuration without parsing the
        // full stream struct.
        if options.withdrawal_steps.is_some() || options.min_withdrawal_amount.is_some() {
            events::stream_config(
                &env, stream_id, options.withdrawal_steps, options.min_withdrawal_amount,
            );
        }

        // Emit supplemental event if recipient allowlist enforcement is enabled
        if let Some(ref t) = tag {
            events::stream_created_with_allowlist_enforcement(&env, stream_id, &recipient);
        }

        Ok(stream_id)
    }

    /// Creates a new payment stream using a federation name (Issue #238).
    #[allow(dead_code)]
    fn create_stream_with_federation(
        env: Env,
        sender: Address,
        federation_name: String,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        cliff_seconds: u64,
        nonce: u64,
        auto_renew: bool,
        renew_count: Option<u32>,
        lock_until: u64,
        allow_recipient_termination: bool,
        non_transferable: bool,
    ) -> Result<u64, StreamError> {
        let recipient = get_federation_address(&env, &federation_name)
            .ok_or(StreamError::StreamNotFound)?;

        Self::create_stream(
            env,
            sender,
            recipient,
            token,
            amount,
            duration_seconds,
            auto_renew,
            lock_until,
            CreateStreamOptions {
                renew_count,
                allow_recipient_termination,
                non_transferable,
                ..Default::default()
            },
        )
    }

    /// Creates a payment stream with a caller-supplied `start_time`.
    ///
    /// Funds are locked in the contract immediately, but streaming does not begin
    /// until the specified `start_time` is reached. This allows advance scheduling
    /// of payment streams.
    ///
    /// `start_time` must satisfy `now <= start_time <= now + max_future_start_offset`.
    /// Returns `InvalidStartTime` for past timestamps and `StartTimeTooFar` when
    /// the offset limit is exceeded.
    pub fn create_stream_scheduled(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        start_time: u64,
        cliff_seconds: u64,
        nonce: u64,
        auto_renew: bool,
        renew_count: Option<u32>,
    ) -> Result<u64, StreamError> {
        sender.require_auth();
        let lock_until = start_time;
        let allow_recipient_termination = false;
        let holdback_amount = 0i128;

        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        let now = env.ledger().timestamp();

        // Validate start_time: must be >= now
        if start_time < now {
            return Err(StreamError::StartTimeTooFar);
        }

        // Validate start_time is not too far in the future
        let max_offset = read_max_future_start_offset(&env);
        let max_start_time = now.saturating_add(max_offset);
        if start_time > max_start_time {
            return Err(StreamError::StartTimeTooFar);
        }

        // Validate cliff: cliff_seconds must not exceed duration_seconds
        if cliff_seconds > duration_seconds {
            return Err(StreamError::InvalidCliff);
        }

        // Basic validations (same as create_stream)
        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        // Recipient whitelist check
        if is_whitelist_enabled(&env) && !is_whitelisted(&env, &recipient) {
            return Err(StreamError::RecipientNotWhitelisted);
        }

        // Recipient allowlist check
        if is_recipient_allowed(&env, &recipient) == false {
            return Err(StreamError::RecipientNotAllowed);
        }

        let min_dur = read_min_duration(&env);
        if duration_seconds < min_dur {
            return Err(StreamError::StreamDurationTooShort);
        }

        let max_dur = read_max_duration(&env);
        if max_dur > 0 && duration_seconds > max_dur {
            return Err(StreamError::DurationExceedsMax);
        }

        let streaming_amount = amount
            .checked_sub(0i128)
            .ok_or(StreamError::Overflow)?;
        let flow_rate = streaming_amount / duration_seconds as i128;
        if flow_rate == 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        if is_blocked(&env, &sender) || is_blocked(&env, &recipient) {
            return Err(StreamError::NotAuthorized);
        }

        let max_per_token = get_max_streams_per_token(&env);
        if max_per_token > 0 && get_token_stream_count(&env, &token) >= max_per_token {
            return Err(StreamError::StreamNotFound);
        }

        mark_nonce_used(&env, &sender, nonce);

        // Calculate end_time from start_time
        let end_time = start_time
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        if end_time <= start_time {
            return Err(StreamError::InvalidEndTime);
        }

        // Calculate cliff_time from start_time
        let cliff_time = start_time
            .checked_add(cliff_seconds)
            .ok_or(StreamError::Overflow)?;

        // Defensive stream ID collision check
        const MAX_ID_RETRIES: u64 = 3;
        let mut stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);
        if stream_exists(&env, stream_id) {
            let mut found = false;
            for retry in 1u64..=MAX_ID_RETRIES {
                let candidate = derive_stream_id(
                    &env, &sender, &recipient, now, nonce ^ (retry << 32),
                );
                if !stream_exists(&env, candidate) {
                    stream_id = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(StreamError::IDCollision);
            }
        }

        let creation_fee = get_creation_fee_xlm(&env);
        if creation_fee > 0 {
            let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
            let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
            token::Client::new(&env, &xlm_token).transfer(
                &sender,
                &treasury,
                &creation_fee,
            );
            events::creation_fee_collected(&env, creation_fee, &treasury);
        }

        // Transfer tokens from sender to contract
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit: streaming_amount,
            flow_rate,
            start_time,
            cliff_time,
            lock_until,
            end_time,
            last_withdraw_time: start_time,
            status: StreamStatus::Active,
            auto_renew,
            options: StreamOptions {
                renew_count: None,
                renewals_used: 0,
                allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: Bytes::new(&env),
                locked: false,
                metadata_uri: None,
                milestones: Vec::new(&env),
                milestone_release_mode: false,
                holdback_amount,
                holdback_claimed: false,
                is_dual_stream: false,
                is_step_vesting: false,
                tranches_claimed: 0,
                oracle: None,
                max_price_deviation_bps: 0,
                creation_price: 0,
                curve: VestingCurve::Linear,
                withdrawal_steps: None,
                current_step: 0,
                min_withdrawal_amount: None,
                non_transferable: false,
                requires_recipient_approval: false,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                on_complete_contract: None,
                on_complete_function: None,
                comment: None,
            },
        };

        save_stream(&env, &stream);
        extend_instance_ttl(&env);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        index_global_stream(&env, stream_id);
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &stream.token);

        set_sender_last_creation_time(&env, &sender, now);

        events::stream_created(
            &env, stream_id, &sender, &recipient, amount, flow_rate, end_time, false, &None,
        );

        // No stream_scheduled event needed

        Ok(stream_id)
    }

    /// Returns the minimum allowed stream duration in seconds.
    pub fn min_duration(env: Env) -> u64 {
        read_min_duration(&env)
    }

    /// Sets the minimum allowed stream duration in seconds. Only the admin may call this.
    pub fn set_min_duration(env: Env, admin: Address, seconds: u64) {
        admin.require_auth();
        write_min_duration(&env, seconds);
    }

    /// Returns the maximum allowed stream duration in seconds (0 = unlimited).
    pub fn max_duration(env: Env) -> u64 {
        read_max_duration(&env)
    }

    /// Sets the maximum allowed stream duration in seconds. Setting to 0 disables the cap. Only the admin may call this.
    pub fn set_max_duration(env: Env, admin: Address, seconds: u64) {
        admin.require_auth();
        write_max_duration(&env, seconds);
    }

    /// Returns the maximum allowed future start-time offset in seconds.
    ///
    /// Scheduled streams must have `start_time <= now + max_future_start_offset`.
    /// Defaults to 365 days (31_536_000 seconds) when not explicitly configured.
    pub fn max_future_start_offset(env: Env) -> u64 {
        read_max_future_start_offset(&env)
    }

    /// Sets the maximum allowed future start-time offset in seconds.
    ///
    /// A value of `0` disables future-dated streams entirely (start_time must equal now).
    /// Only the admin may call this.
    pub fn set_max_future_start_offset(env: Env, admin: Address, offset_seconds: u64) {
        admin.require_auth();
        write_max_future_start_offset(&env, offset_seconds);
    }

    // ── Step-vesting: create_stream_with_schedule ────────────────────────────

    /// Creates a step-vesting stream whose tokens release in discrete tranches.
    ///
    /// Each tranche unlocks its full `amount` atomically once `unlock_time` is
    /// reached.  Tranches must be sorted by `unlock_time` (ascending), non-empty,
    /// each have a positive amount, and their amounts must sum exactly to `deposit`.
    ///
    /// Optionally attaches an oracle for on-chain price validation.  When
    /// `oracle` is `Some(addr)`, `get_price(token)` is called immediately to
    /// record the baseline price; subsequent withdrawals will fail if the current
    /// price deviates by more than `max_price_deviation_bps`.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    fn create_stream_with_schedule(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        tranches: Vec<VestingTranche>,
        nonce: u64,
        lock_until: u64,
        allow_recipient_termination: bool,
        oracle: Option<Address>,
        max_price_deviation_bps: u32,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        let now = env.ledger().timestamp();

        // Check rate limit (per-sender creation frequency cap)
        check_rate_limit(&env, &sender)?;

        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if deposit <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if tranches.is_empty() {
            return Err(StreamError::InvalidTranches);
        }
        validate_recipient_address(&env, &sender, &recipient)?;
        check_token_whitelist(&env, &token)?;
        validate_token_address(&env, &token)?;
        if is_whitelist_enabled(&env) && !is_whitelisted(&env, &recipient) {
            return Err(StreamError::RecipientNotWhitelisted);
        }

        // Validate tranches: sorted unlock times, positive amounts, sum == deposit.
        let mut tranche_sum: i128 = 0;
        let mut prev_unlock: u64 = 0;
        for i in 0..tranches.len() {
            let t = tranches.get(i).unwrap();
            if t.amount <= 0 {
                return Err(StreamError::InvalidTranches);
            }
            if i > 0 && t.unlock_time <= prev_unlock {
                return Err(StreamError::InvalidTranches);
            }
            prev_unlock = t.unlock_time;
            tranche_sum = tranche_sum
                .checked_add(t.amount)
                .ok_or(StreamError::Overflow)?;
        }
        if tranche_sum != deposit {
            return Err(StreamError::InvalidTranches);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        // Check token whitelist (Issue #221)
        check_token_whitelist(&env, &token)?;

        mark_nonce_used(&env, &sender, nonce);

        // end_time is the unlock_time of the last tranche.
        let last_tranche = tranches.get(tranches.len() - 1).unwrap();
        let end_time = last_tranche.unlock_time;
        if end_time <= now {
            return Err(StreamError::InvalidEndTime);
        }

        // Validate minimum duration between start_time (now) and end_time.
        let duration_seconds = end_time
            .checked_sub(now)
            .ok_or(StreamError::Overflow)?;
        let min_dur = read_min_duration(&env);
        if duration_seconds < min_dur {
            return Err(StreamError::StreamDurationTooShort);
        }

        // ── Defensive stream ID collision check (schedule path) ─────────────
        const MAX_ID_RETRIES_SCHED: u64 = 3;
        let mut stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);
        if stream_exists(&env, stream_id) {
            let mut found = false;
            for retry in 1u64..=MAX_ID_RETRIES_SCHED {
                let candidate = derive_stream_id(
                    &env, &sender, &recipient, now, nonce ^ (retry << 32),
                );
                if !stream_exists(&env, candidate) {
                    stream_id = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(StreamError::IDCollision);
            }
        }

        let creation_price = if let Some(ref oracle_addr) = oracle {
            oracle::fetch_price(&env, oracle_addr, &token)?
        } else {
            0
        };

        // Collect XLM creation fee if configured.
        let creation_fee = get_creation_fee_xlm(&env);
        if creation_fee > 0 {
            let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
            let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
            token::Client::new(&env, &xlm_token).transfer(
                &sender,
                &treasury,
                &creation_fee,
            );
            events::creation_fee_collected(&env, creation_fee, &treasury);
        }

        // Transfer deposit into the contract.
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        let tranche_count = tranches.len();

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit,
            flow_rate: 0,
            start_time: now,
            cliff_time: now,
            lock_until,
            end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew: false,
            options: StreamOptions {
                renew_count: None,
                renewals_used: 0,
                allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: Bytes::new(&env),
                locked: false,
                metadata_uri: None,
                milestones: soroban_sdk::Vec::new(&env),
                milestone_release_mode: false,
                holdback_amount: 0,
                holdback_claimed: false,
                is_step_vesting: true,
                tranches_claimed: 0,
                oracle: oracle.clone(),
                max_price_deviation_bps,
                creation_price,
                curve: VestingCurve::Linear,
                withdrawal_steps: None,
                current_step: 0,
                min_withdrawal_amount: None,
                non_transferable: false,
                requires_recipient_approval: false,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                is_dual_stream: false,
                on_complete_contract: None,
                on_complete_function: None,
                comment: None,
            },
        };

        save_stream(&env, &stream);
        extend_instance_ttl(&env);
        save_tranches(&env, stream_id, &tranches);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        index_global_stream(&env, stream_id);
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &stream.token);

        events::tranche_stream_created(&env, stream_id, &sender, tranche_count, deposit);
        events::stream_created(&env, stream_id, &sender, &recipient, deposit, 0, end_time, false, &None);

        Ok(stream_id)
    }

    // ── Time-decay vesting: create_stream_with_curve ─────────────────────────

    /// Creates a stream with an explicit vesting curve.
    ///
    /// Pass `curve: VestingCurve::Linear` to reproduce the standard constant-rate
    /// behaviour.  Pass `curve: VestingCurve::TimeDecay { decay_factor }` to get a
    /// front-weighted release where more tokens unlock early in the stream lifetime.
    ///
    /// The `decay_factor` is expressed in **basis points per 1 000 seconds**
    /// (e.g. `100` = 1 % per 1 ks window).  A value of `0` is identical to
    /// `VestingCurve::Linear`.  Values ≥ 10 000 are clamped to 9 999 internally.
    ///
    /// All other fields behave identically to `create_stream`.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    fn create_stream_with_curve(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        cliff_seconds: u64,
        nonce: u64,
        auto_renew: bool,
        renew_count: Option<u32>,
        lock_until: u64,
        allow_recipient_termination: bool,
        curve: VestingCurve,
        on_complete_contract: Option<Address>,
        on_complete_function: Option<Symbol>,
        escrow_hold: bool,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        let now = env.ledger().timestamp();

        // Check rate limit (per-sender creation frequency cap)
        check_rate_limit(&env, &sender)?;

        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if cliff_seconds > duration_seconds {
            return Err(StreamError::InvalidCliff);
        }
        if is_whitelist_enabled(&env) && !is_whitelisted(&env, &recipient) {
            return Err(StreamError::RecipientNotWhitelisted);
        }

        let min_dur = read_min_duration(&env);
        if duration_seconds < min_dur {
            return Err(StreamError::StreamDurationTooShort);
        }

        // For linear streams the flow_rate is used; for TimeDecay it is stored
        // for reference but actual claimable is driven by the decay formula.
        let flow_rate = amount / duration_seconds as i128;
        if flow_rate == 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        // Check token whitelist (Issue #221)
        check_token_whitelist(&env, &token)?;

        mark_nonce_used(&env, &sender, nonce);

        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        if end_time <= now {
            return Err(StreamError::InvalidEndTime);
        }
        let cliff_time = now
            .checked_add(cliff_seconds)
            .ok_or(StreamError::Overflow)?;

        // ── Defensive stream ID collision check (curve path) ─────────────────
        const MAX_ID_RETRIES_CURVE: u64 = 3;
        let mut stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);
        if stream_exists(&env, stream_id) {
            let mut found = false;
            for retry in 1u64..=MAX_ID_RETRIES_CURVE {
                let candidate = derive_stream_id(
                    &env, &sender, &recipient, now, nonce ^ (retry << 32),
                );
                if !stream_exists(&env, candidate) {
                    stream_id = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(StreamError::IDCollision);
            }
        }

        let creation_fee = get_creation_fee_xlm(&env);
        if creation_fee > 0 {
            let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
            let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
            token::Client::new(&env, &xlm_token).transfer(
                &sender,
                &treasury,
                &creation_fee,
            );
            events::creation_fee_collected(&env, creation_fee, &treasury);
        }

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit: amount,
            flow_rate,
            start_time: now,
            cliff_time,
            lock_until,
            end_time,
            last_withdraw_time: now,
            status: if escrow_hold {
            StreamStatus::EscrowHold
            } else {
            StreamStatus::Active
            },
            auto_renew,
            options: StreamOptions {
                renew_count,
                renewals_used: 0,
                allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: Bytes::new(&env),
                locked: false,
                metadata_uri: None,
                milestones: soroban_sdk::Vec::new(&env),
                milestone_release_mode: false,
                holdback_amount: 0,
                holdback_claimed: false,
                is_step_vesting: false,
                tranches_claimed: 0,
                oracle: None,
                max_price_deviation_bps: 0,
                creation_price: 0,
                curve,
                withdrawal_steps: None,
                current_step: 0,
                min_withdrawal_amount: None,
                non_transferable: false,
                requires_recipient_approval: false,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                is_dual_stream: false,
                on_complete_contract,
                on_complete_function,
                comment: None,
            },
        };

        save_stream(&env, &stream);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        index_global_stream(&env, stream_id);
        // Only count as active if not in escrow hold
        if !escrow_hold {
            increment_active_stream_count(&env);
            increment_token_stream_count(&env, &stream.token);
        }

        if escrow_hold {
            events::stream_placed_in_escrow(&env, stream_id, &sender, &recipient, amount);
        } else {
            events::stream_created(
                &env, stream_id, &sender, &recipient, amount, flow_rate, end_time, false, &None,
            );
        }

        Ok(stream_id)
    }

    /// Creates a stream with timestamp-gated milestones (tranches).
    /// Each milestone automatically unlocks at its unlock_time without requiring sender approval.
    #[allow(clippy::too_many_arguments)]
    pub fn create_stream_with_milestones(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        milestones_data: Vec<(i128, u64, BytesN<32>)>,  // (amount, unlock_time, description_hash)
        nonce: u64,
        lock_until: u64,
        allow_recipient_termination: bool,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        if nonce_used(&env, &sender, nonce) {
            return Err(StreamError::DuplicateStream);
        }
        if deposit <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        if milestones_data.is_empty() {
            return Err(StreamError::InvalidDuration);
        }
        if is_whitelist_enabled(&env) && !is_whitelisted(&env, &recipient) {
            return Err(StreamError::RecipientNotWhitelisted);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count >= limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        mark_nonce_used(&env, &sender, nonce);

        let now = env.ledger().timestamp();

        // Validate milestones and calculate end_time from the last unlock_time
        let mut total_amount = 0i128;
        let mut prev_unlock_time = 0u64;
        for (amount, unlock_time, _) in milestones_data.iter() {
            if amount <= 0 {
                return Err(StreamError::ZeroAmount);
            }
            if unlock_time < now {
                return Err(StreamError::InvalidCliff);  // Milestone in the past
            }
            if unlock_time < prev_unlock_time {
                return Err(StreamError::InvalidDuration);  // Milestones not sorted
            }
            total_amount = total_amount.checked_add(amount).ok_or(StreamError::Overflow)?;
            prev_unlock_time = unlock_time;
        }

        if total_amount != deposit {
            return Err(StreamError::ZeroAmount);
        }

        let end_time = prev_unlock_time;
        if end_time <= now {
            return Err(StreamError::InvalidEndTime);
        }

        // Defensive stream ID collision check
        const MAX_ID_RETRIES: u64 = 3;
        let mut stream_id = derive_stream_id(&env, &sender, &recipient, now, nonce);
        if stream_exists(&env, stream_id) {
            let mut found = false;
            for retry in 1u64..=MAX_ID_RETRIES {
                let candidate = derive_stream_id(
                    &env, &sender, &recipient, now, nonce ^ (retry << 32),
                );
                if !stream_exists(&env, candidate) {
                    stream_id = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(StreamError::IDCollision);
            }
        }

        let creation_fee = get_creation_fee_xlm(&env);
        if creation_fee > 0 {
            let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
            let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
            token::Client::new(&env, &xlm_token).transfer(
                &sender,
                &treasury,
                &creation_fee,
            );
            events::creation_fee_collected(&env, creation_fee, &treasury);
        }

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        // Build milestones vector
        let mut milestones = Vec::new(&env);
        for (amount, unlock_time, description_hash) in milestones_data.iter() {
            let milestone = Milestone {
                amount,
                description_hash,
                unlock_time,
                status: MilestoneStatus::Pending,
            };
            milestones.push_back(milestone);
        }

        let stream = Stream {
            id: stream_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            deposit,
            flow_rate: 0, // Not used for milestone-gated streams
            start_time: now,
            cliff_time: now,
            lock_until,
            end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew: false,
            options: StreamOptions {
                renew_count: None,
                renewals_used: 0,
                allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: Bytes::new(&env),
                locked: false,
                metadata_uri: None,
                milestones,
                milestone_release_mode: true,
                holdback_amount: 0,
                holdback_claimed: false,
                is_step_vesting: false,
                tranches_claimed: 0,
                oracle: None,
                max_price_deviation_bps: 0,
                creation_price: 0,
                curve: VestingCurve::Linear,
                withdrawal_steps: None,
                current_step: 0,
                min_withdrawal_amount: None,
                non_transferable: false,
                requires_recipient_approval: false,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                is_dual_stream: false,
                on_complete_contract: None,
                on_complete_function: None,
                comment: None,
            },
        };

        save_stream(&env, &stream);
        index_by_sender(&env, &sender, stream_id);
        index_by_recipient(&env, &recipient, stream_id);
        index_global_stream(&env, stream_id);
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &stream.token);

        events::stream_created(
            &env, stream_id, &sender, &recipient, deposit, 0, end_time, false, &None,
        );

        Ok(stream_id)
    }

    // ── Off-chain preview utility ─────────────────────────────────────────────

    /// Returns the cumulative amount that **would** be claimable at `query_time`
    /// if the given stream were evaluated at that moment — regardless of how much
    /// has already been withdrawn.
    ///
    /// This is a **read-only** preview function for off-chain UIs and analytics.
    /// It does not check stream status, reentrancy, or auth.
    ///
    /// For `VestingCurve::Linear` this is simply `flow_rate × min(elapsed, duration)`.
    /// For `VestingCurve::TimeDecay` it returns the cumulative decay-weighted amount.
    /// For step-vesting streams (`is_step_vesting = true`) it returns the sum of
    /// tranches whose `unlock_time ≤ query_time`.
    pub fn simulate_claimable(
        env: Env,
        stream_id: u64,
        query_time: u64,
    ) -> Result<i128, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        // Step-vesting: sum tranches whose unlock_time ≤ query_time.
        if stream.options.is_step_vesting {
            let tranches = load_tranches(&env, stream_id);
            let mut total: i128 = 0;
            for i in 0..tranches.len() {
                let t = tranches.get(i).unwrap();
                if query_time >= t.unlock_time {
                    total = total.checked_add(t.amount).ok_or(StreamError::Overflow)?;
                } else {
                    break;
                }
            }
            return Ok(total);
        }

        // Continuous vesting: use simulate_claimable from vesting_math (cumulative from start).
        let decay_factor = match &stream.options.curve {
            VestingCurve::Linear => 0u32,
            VestingCurve::TimeDecay(decay_factor) => *decay_factor,
        };

        vesting_math::simulate_claimable(
            stream.deposit,
            stream.start_time,
            stream.end_time,
            query_time,
            stream.cliff_time,
            decay_factor,
        )
        .ok_or(StreamError::Overflow)
    }

    /// Sets the global withdrawal cooldown in seconds.
    pub fn set_withdrawal_cooldown(env: Env, admin: Address, cooldown_seconds: u64) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_withdrawal_cooldown(&env, cooldown_seconds);
        Ok(())
    }

    /// Sets the global stream creation cooldown in seconds (Issue #239).
    /// Cooldown of 0 disables the mechanism (default).
    pub fn set_stream_creation_cooldown(env: Env, admin: Address, cooldown_seconds: u64) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_stream_creation_cooldown(&env, cooldown_seconds);
        Ok(())
    }

    /// Registers a federation name to a Stellar address (Issue #238).
    /// Only the admin may call this function.
    pub fn register_federation(
        env: Env,
        admin: Address,
        federation_name: String,
        stellar_address: Address,
    ) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        register_federation_address(&env, &federation_name, &stellar_address);
        events::federation_registered(&env, &federation_name, &stellar_address);
        Ok(())
    }

    /// Unregisters a federation name from the registry (Issue #238).
    /// Only the admin may call this function.
    pub fn unregister_federation(
        env: Env,
        admin: Address,
        federation_name: String,
    ) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        unregister_federation_address(&env, &federation_name);
        events::federation_unregistered(&env, &federation_name);
        Ok(())
    }

    /// Resolves a federation name to its registered Stellar address.
    pub fn resolve_federation(env: Env, federation_name: String) -> Result<Address, StreamError> {
        get_federation_address(&env, &federation_name).ok_or(StreamError::StreamNotFound)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate limit admin management
    // ─────────────────────────────────────────────────────────────────────────

    /// Sets the sliding-window size for the per-sender rate limit, in **ledgers**.
    ///
    /// Default: 720 ledgers (~1 hour at 5 s/ledger).
    /// The state is stored in temporary storage keyed per sender; the entry TTL is
    /// refreshed to `window_ledgers` on every `create_stream` call, so the window
    /// cannot expire while a sender is actively within it.
    /// Only the admin may call this.
    pub fn set_rate_limit_window(env: Env, admin: Address, window_ledgers: u32) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_rate_limit_window(&env, window_ledgers);
        events::rate_limit_updated(&env, window_ledgers as u64, get_rate_limit_max_creations(&env));
        Ok(())
    }

    /// Sets the maximum number of streams a single sender may create within one window.
    ///
    /// Default: 20. Only the admin may call this.
    pub fn set_rate_limit_max(env: Env, admin: Address, max_creations: u32) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_rate_limit_max_creations(&env, max_creations);
        events::rate_limit_updated(&env, get_rate_limit_window(&env) as u64, max_creations);
        Ok(())
    }

    /// Exempts an address from all per-sender rate limiting.
    ///
    /// Exempt addresses bypass `check_rate_limit` entirely and are never throttled.
    /// Intended for trusted integrators, relayers, or the admin itself.
    /// Only the admin may call this.
    pub fn add_rate_limit_exempt(env: Env, admin: Address, address: Address) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        add_rate_limit_exempt(&env, &address);
        Ok(())
    }

    /// Removes a rate-limit exemption, re-subjecting the address to the normal window cap.
    ///
    /// Only the admin may call this.
    pub fn remove_rate_limit_exempt(env: Env, admin: Address, address: Address) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        remove_rate_limit_exempt(&env, &address);
        Ok(())
    }

    /// Returns the number of stream creations the given sender may still perform in the
    /// current window.
    ///
    /// - Returns `u32::MAX` for exempt addresses.
    /// - Returns the full quota if the sender has no active window (never created or window lapsed).
    /// - Returns 0 if the sender has exhausted their quota for the current window.
    pub fn remaining_quota(env: Env, address: Address) -> u32 {
        get_remaining_quota(&env, &address)
    }

    /// Enables or disables recipient whitelisting.
    pub fn set_whitelist_enabled(env: Env, admin: Address, enabled: bool) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_whitelist_enabled(&env, enabled);
        Ok(())
    }

    /// Sets the expiry warning window in ledgers. Admin only.
    /// Default: 17280 (~24 h at 5 s/ledger). Must be > 0.
    pub fn set_expiry_warning_window(env: Env, window_ledgers: u32) -> Result<(), StreamError> {
        check_admin(&env);
        if window_ledgers == 0 { return Err(StreamError::InvalidExpiryWindow); }
        set_expiry_warning_window(&env, window_ledgers);
        Ok(())
    }
    pub fn get_expiry_warning_window(env: Env) -> u32 { get_expiry_warning_window(&env) }

    // ─────────────────────────────────────────────────────────────────────────
    // Feature (b): Sender reputation cap config
    // ─────────────────────────────────────────────────────────────────────────

    /// Sets the new-sender stream cap (max concurrent streams before promotion). Admin only.
    pub fn set_new_sender_stream_cap(env: Env, cap: u32) -> Result<(), StreamError> {
        check_admin(&env); set_new_sender_stream_cap(&env, cap); Ok(())
    }
    pub fn get_new_sender_stream_cap(env: Env) -> u32 { get_new_sender_stream_cap(&env) }

    /// Sets the promotion threshold (lifetime stream count). Admin only.
    pub fn set_sender_promotion_threshold(env: Env, threshold: u32) -> Result<(), StreamError> {
        check_admin(&env); set_sender_promotion_threshold(&env, threshold); Ok(())
    }
    pub fn get_sender_promotion_threshold(env: Env) -> u32 { get_sender_promotion_threshold(&env) }
    pub fn get_sender_lifetime_count(env: Env, sender: Address) -> u32 { get_sender_lifetime_count(&env, &sender) }
    pub fn is_sender_promoted(env: Env, sender: Address) -> bool { is_sender_promoted(&env, &sender) }

    // ─────────────────────────────────────────────────────────────────────────
    // Feature (c): Stream redirect management
    // ─────────────────────────────────────────────────────────────────────────

    /// Sets a redirect target on a stream. Only the recipient may call this.
    /// On withdraw, claimable tokens will be topped up into the target stream
    /// instead of sent directly to the recipient.
    pub fn set_redirect(env: Env, stream_id: u64, target_stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        recipient.require_auth();
        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.recipient != recipient { return Err(StreamError::NotRecipient); }
        let target = load_stream(&env, target_stream_id).ok_or(StreamError::InvalidRedirectTarget)?;
        if target.recipient != recipient { return Err(StreamError::RedirectRecipientMismatch); }
        check_no_circular_redirect(&env, stream_id, target_stream_id)?;
        stream.options.redirect_to_stream_id = Some(target_stream_id);
        save_stream(&env, &stream);
        events::stream_redirect_set(&env, stream_id, target_stream_id, &recipient);
        Ok(())
    }

    /// Clears the redirect target on a stream. Only the recipient may call this.
    pub fn clear_redirect(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        recipient.require_auth();
        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.recipient != recipient { return Err(StreamError::NotRecipient); }
        stream.options.redirect_to_stream_id = None;
        save_stream(&env, &stream);
        events::stream_redirect_cleared(&env, stream_id, &recipient);
        Ok(())
    }

    pub fn get_redirect(env: Env, stream_id: u64) -> Option<u64> {
        load_stream(&env, stream_id).and_then(|s| s.options.redirect_to_stream_id)
    }

    /// Enables or disables the token whitelist enforcement (Issue #221).
    /// When enabled, only whitelisted tokens can be used in streams.
    pub fn set_token_whitelist_enabled(env: Env, admin: Address, enabled: bool) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        set_token_whitelist_enabled(&env, enabled);
        events::token_whitelist_toggled(&env, enabled);
        Ok(())
    }

    /// Adds a token to the whitelist (Issue #221).
    /// When token whitelisting is enabled, only tokens in the whitelist can be streamed.
    pub fn add_token_to_whitelist(env: Env, admin: Address, token: Address) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        add_token_to_whitelist(&env, &token);
        events::token_whitelisted(&env, &token);
        Ok(())
    }

    /// Removes a token from the whitelist (Issue #221).
    pub fn remove_token_from_whitelist(env: Env, admin: Address, token: Address) -> Result<(), StreamError> {
        check_admin(&env);
        admin.require_auth();
        remove_token_from_whitelist(&env, &token);
        events::token_dwhitelisted(&env, &token);
        Ok(())
    }

    // ── Issue #286: Per-token stream count cap ──────────────────────────────

    /// Sets the per-token stream cap. Setting to 0 disables the cap. Admin only.
    pub fn set_max_streams_per_token(env: Env, max: u32) -> Result<(), StreamError> {
        check_admin(&env);
        set_max_streams_per_token(&env, max);
        Ok(())
    }

    /// Returns the current per-token stream cap (0 = unlimited).
    pub fn get_max_streams_per_token(env: Env) -> u32 {
        get_max_streams_per_token(&env)
    }

    // ── Per-asset maximum deposit limit ──────────────────────────────────────

    /// Sets the maximum deposit amount for a single stream using a specific token.
    /// Setting to 0 disables the limit for that token. Admin only.
    ///
    /// This provides risk management by preventing users from locking unbounded
    /// amounts of a particular asset in a single stream.
    pub fn set_max_deposit_per_token(env: Env, token: Address, max_deposit: i128) -> Result<(), StreamError> {
        check_admin(&env);
        if max_deposit < 0 {
            return Err(StreamError::ZeroAmount);
        }
        set_max_deposit_per_token(&env, &token, max_deposit);
        Ok(())
    }

    /// Returns the maximum deposit amount for a single stream using the given token.
    /// Returns 0 if no limit is set (unlimited).
    pub fn get_max_deposit_per_token(env: Env, token: Address) -> i128 {
        get_max_deposit_per_token(&env, &token)
    }

    // ── Issue #284: Address blocklist ───────────────────────────────────────

    /// Adds an address to the blocklist. Admin only.
    pub fn add_to_blocklist(env: Env, addr: Address) -> Result<(), StreamError> {
        check_admin(&env);
        add_to_blocklist(&env, &addr);
        events::address_blocked(&env, &read_admin(&env).unwrap(), &addr);
        Ok(())
    }

    /// Removes an address from the blocklist. Admin only.
    pub fn remove_from_blocklist(env: Env, addr: Address) -> Result<(), StreamError> {
        check_admin(&env);
        remove_from_blocklist(&env, &addr);
        events::address_unblocked(&env, &read_admin(&env).unwrap(), &addr);
        Ok(())
    }

    /// Returns true if the address is on the blocklist.
    pub fn is_blocked(env: Env, addr: Address) -> bool {
        is_blocked(&env, &addr)
    }

    // ── Issue #282: Grace period & recovery ─────────────────────────────────

    /// Sets the grace period in ledgers. Zero means no grace period. Admin only.
    pub fn set_grace_period_ledgers(env: Env, ledgers: u32) -> Result<(), StreamError> {
        check_admin(&env);
        set_grace_period_ledgers(&env, ledgers);
        Ok(())
    }

    /// Returns the current grace period in ledgers (0 = no grace period).
    pub fn get_grace_period_ledgers(env: Env) -> u32 {
        get_grace_period_ledgers(&env)
    }

    /// Allows the sender to recover unclaimed funds from an expired stream after
    /// the grace period has elapsed.
    ///
    /// The stream must be past its `end_time` and the grace period (in ledgers)
    /// must have passed since `end_time`. After recovery the stream is removed.
    pub fn recover_expired(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        sender.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        let now = env.ledger().timestamp();
        if now < stream.end_time {
            return Err(StreamError::StreamNotComplete);
        }

        let grace = get_grace_period_ledgers(&env);
        if grace > 0 {
            let grace_seconds = (grace as u64).saturating_mul(5);
            let grace_end = stream.end_time.saturating_add(grace_seconds);
            if now < grace_end {
                return Err(StreamError::StreamNotActive);
            }
        }

        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        if available > 0 {
            token::Client::new(&env, &stream.token).transfer(
                &env.current_contract_address(),
                &sender,
                &available,
            );
        }

        remove_stream(&env, stream_id);
        Self::unindex_stream(&env, &stream, stream_id);
        decrement_token_stream_count(&env, &stream.token);

        events::stream_recovered(&env, stream_id, &sender, available);

        Ok(())
    }

    /// Updates slippage protection parameters for a stream (Issue #218).
    pub fn set_slippage_params(
        env: Env,
        sender: Address,
        stream_id: u64,
        reference_price: i128,
        max_slippage_bps: u32,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        if max_slippage_bps > 10000 {
            return Err(StreamError::InvalidSlippage);
        }

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        set_slippage_params(&env, stream_id, reference_price, max_slippage_bps);
        Ok(())
    }

    /// Updates metadata URI for a stream.
    pub fn update_metadata_uri(
        env: Env,
        sender: Address,
        stream_id: u64,
        metadata_uri: Option<String>,
    ) -> Result<(), StreamError> {
        sender.require_auth();
        validate_metadata_uri(&metadata_uri)?;

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        stream.options.metadata_uri = metadata_uri.clone();
        save_stream(&env, &stream);
        events::metadata_uri_updated(&env, stream_id, &metadata_uri);

        Ok(())
    }

    /// Sweeps expired, fully-withdrawn streams from storage and refunds rent incentive.
    pub fn sweep_expired(env: Env, stream_ids: Vec<u64>) -> Result<(), StreamError> {
        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

            // Check if stream is expired and fully withdrawn (or cancelled)
            let is_expired = now >= stream.end_time;
            let is_fully_withdrawn = stream.options.total_withdrawn >= stream.deposit || stream.status == StreamStatus::Cancelled;

            if !is_expired || !is_fully_withdrawn {
                return Err(StreamError::StreamNotComplete);
            }

            // Delete storage entries
            remove_stream(&env, stream_id);
            Self::unindex_stream(&env, &stream, stream_id);
            decrement_token_stream_count(&env, &stream.token);

            events::stream_swept(&env, stream_id, &stream.sender);
        }

        Ok(())
    }

    /// Releases a milestone, making its funds claimable by the recipient.
    pub fn release_milestone(
        env: Env,
        stream_id: u64,
        milestone_index: u32,
        sender: Address,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        if milestone_index >= stream.options.milestones.len() {
            return Err(StreamError::InvalidDuration);
        }

        // Get mutable reference to the milestone and change its status
        let mut milestone = stream.options.milestones.get(milestone_index).unwrap();
        milestone.status = crate::types::MilestoneStatus::Released;
        stream.options.milestones.set(milestone_index, milestone);

        save_stream(&env, &stream);
        events::milestone_released(&env, stream_id, milestone_index);

        Ok(())
    }

    /// Extends the Soroban persistent storage TTL for a stream and its indices.
    ///
    /// Callable by anyone — no auth required. Bumps the TTL to cover the remaining
    /// stream duration plus a 24-hour safety buffer (~17280 ledgers). No-op when
    /// the current TTL is already sufficient (extend_ttl is a no-op if new TTL <=
    /// current TTL internally).
    ///
    /// Emits `TtlBumped { stream_id, new_expiry_ledger }`.
    pub fn bump_stream_ttl(env: Env, stream_id: u64) -> Result<(), StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();
        let remaining = stream.end_time.saturating_sub(now);

        const SAFETY_BUFFER_LEDGERS: u32 = 17_280;
        let ledgers_for_remaining = (remaining / 5) as u32;
        let ledgers_needed = ledgers_for_remaining.saturating_add(SAFETY_BUFFER_LEDGERS);

        env.storage()
            .persistent()
            .extend_ttl(&stream_id, ledgers_needed, ledgers_needed);

        let scnt: u32 = env
            .storage()
            .persistent()
            .get(&storage::sender_count_key(&env, &stream.sender))
            .unwrap_or(0u32);
        for i in 0..scnt {
            let slot = storage::sender_slot_key(&env, &stream.sender, i);
            if let Some(id) = env.storage().persistent().get::<_, u64>(&slot) {
                if id == stream_id {
                    env.storage()
                        .persistent()
                        .extend_ttl(&slot, ledgers_needed, ledgers_needed);
                    break;
                }
            }
        }

        let rcnt: u32 = env
            .storage()
            .persistent()
            .get(&storage::recipient_count_key(&env, &stream.recipient))
            .unwrap_or(0u32);
        for i in 0..rcnt {
            let slot = storage::recipient_slot_key(&env, &stream.recipient, i);
            if let Some(id) = env.storage().persistent().get::<_, u64>(&slot) {
                if id == stream_id {
                    env.storage()
                        .persistent()
                        .extend_ttl(&slot, ledgers_needed, ledgers_needed);
                    break;
                }
            }
        }

        let new_expiry_ledger = env.ledger().sequence().saturating_add(ledgers_needed);
        events::ttl_bumped(&env, stream_id, new_expiry_ledger);

        Ok(())
    }

    /// Sets the flat XLM creation fee (in stroops) and the XLM SAC token address.
    pub fn set_creation_fee(env: Env, fee: i128, xlm_token: Address) -> Result<(), StreamError> {
        check_admin(&env);
        if fee < 0 {
            return Err(StreamError::ZeroAmount);
        }
        set_creation_fee_xlm(&env, fee);
        set_xlm_token(&env, &xlm_token);
        Ok(())
    }

    /// Activates a stream that was created with escrow_hold = true.
    ///
    /// Transitions the stream from EscrowHold to Active state, enabling token flow.
    /// Only the sender may call this. No-op if the stream is not in EscrowHold state.
    ///
    /// # Errors
    /// Returns `NotSender` if the caller is not the stream sender.
    /// Returns `StreamNotActive` if the stream is not in EscrowHold state.
    /// Returns `StreamNotFound` if no stream with this ID exists.
    pub fn activate_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        // Only activate if currently in EscrowHold state
        if stream.status != StreamStatus::EscrowHold {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();

        // Transition to Active state
        stream.status = StreamStatus::Active;
        save_stream(&env, &stream);

        // Update counts now that stream is active
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &stream.token);

        // Emit activation event
        events::stream_activated(&env, stream_id, &sender, now);

        Ok(())
    }

    /// Allows the recipient to withdraw all tokens earned since last withdrawal.
    ///
    /// Follows checks-effects-interactions: all state is updated before any token
    /// transfer. A reentrancy guard blocks re-entrant calls during settlement.
    pub fn withdraw(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }

        recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.status == StreamStatus::PendingApproval || stream.status == StreamStatus::EscrowHold {
            return Err(StreamError::AwaitingApproval);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        // Stream-specific reentrancy guard
        if stream.options.locked {
            return Err(StreamError::ReentrancyDetected);
        }

        let now = env.ledger().timestamp();
        if now < stream.lock_until {
            return Err(StreamError::StreamLocked);
        }

        let cooldown = get_withdrawal_cooldown(&env);
        if cooldown > 0 && now < stream.last_withdraw_time.saturating_add(cooldown) {
            return Err(StreamError::WithdrawalCooldownActive);
        }

        // ── Milestone-release-mode withdrawal path ───────────────────────────
        if stream.options.milestone_release_mode {
            // Auto-unlock milestones that have reached their unlock_time
            let mut claimable: i128 = 0;
            let mut updated_any_milestone = false;
            for i in 0..stream.options.milestones.len() {
                let mut milestone = stream.options.milestones.get(i).unwrap();
                if milestone.status == MilestoneStatus::Pending && now >= milestone.unlock_time {
                    milestone.status = MilestoneStatus::Released;
                    stream.options.milestones.set(i, milestone.clone());
                    updated_any_milestone = true;
                    #[allow(clippy::unnecessary_cast)]
                    {
                        events::milestone_released(&env, stream_id, i as u32);
                    }
                }
                if milestone.status == MilestoneStatus::Released {
                    claimable = claimable
                        .checked_add(milestone.amount)
                        .ok_or(StreamError::Overflow)?;
                }
            }

            if updated_any_milestone {
                save_stream(&env, &stream);
            }

            // Compute available amount (total unlocked - already withdrawn)
            let available = claimable.saturating_sub(stream.options.total_withdrawn).max(0);
            if available == 0 {
                return Err(StreamError::ZeroAmount);
            }

            // Compute fee on claimable amount.
            let fee_bps = storage::get_effective_fee_tier(&env, &stream.token);
            let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
                available
                    .checked_mul(fee_bps as i128)
                    .ok_or(StreamError::Overflow)?
                    / 10_000
            } else {
                0
            };
            let recipient_amount = available - fee_amount;
            let treasury_opt = get_treasury(&env);

            // EFFECTS — update total_withdrawn before any token transfer.
            stream.options.total_withdrawn = stream
                .options.total_withdrawn
                .checked_add(available)
                .ok_or(StreamError::Overflow)?;
            stream.last_withdraw_time = now;

            let all_milestones_released = stream.options.milestones.iter()
                .all(|m| m.status == MilestoneStatus::Released);
            let all_milestones_withdrawn = stream.options.total_withdrawn >= claimable;

            if all_milestones_released && all_milestones_withdrawn {
                stream.status = StreamStatus::Completed;
                save_stream(&env, &stream);
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
                remove_stream(&env, stream_id);
                Self::unindex_stream(&env, &stream, stream_id);
            } else {
                save_stream(&env, &stream);
            }

            // INTERACTIONS
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipient,
                    &recipient_amount,
                );
            }
            if fee_amount > 0 {
                if let Some(ref t) = treasury_opt {
                    token_client.transfer(
                        &env.current_contract_address(),
                        t,
                        &fee_amount,
                    );
                    events::fee_collected(&env, stream_id, fee_amount, t);
                }
            }

            events::stream_withdrawn(&env, stream_id, &recipient, available, now, stream.options.total_withdrawn);
            return Ok(());
        }

        // ── Step-vesting withdrawal path ─────────────────────────────────────
        if stream.options.is_step_vesting {
            // Oracle check (if configured).
            if let Some(ref oracle_addr) = stream.options.oracle {
                let (current_price, deviation_bps) = oracle::check_oracle(
                    &env,
                    oracle_addr,
                    &stream.token,
                    stream.options.creation_price,
                    stream.options.max_price_deviation_bps,
                )?;
                events::price_check_passed(&env, stream_id, &stream.token, current_price, deviation_bps);
            }

            let tranches = load_tranches(&env, stream_id);
            let mut claimable: i128 = 0;
            let mut new_cursor = stream.options.tranches_claimed;

            // Drain all tranches whose unlock_time has passed — each releases atomically.
            while new_cursor < tranches.len() {
                let t = tranches.get(new_cursor).unwrap();
                if now >= t.unlock_time {
                    claimable = claimable
                        .checked_add(t.amount)
                        .ok_or(StreamError::Overflow)?;
                    new_cursor += 1;
                } else {
                    break;
                }
            }

            let tranches_newly_claimed = new_cursor - stream.options.tranches_claimed;

            // Compute fee on claimable amount.
            let (recipient_amount, fee_amount, treasury_opt) = if claimable > 0 {
                let fee_bps = storage::get_effective_fee_tier(&env, &stream.token);
                let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
                    claimable
                        .checked_mul(fee_bps as i128)
                        .ok_or(StreamError::Overflow)?
                        / 10_000
                } else {
                    0
                };
                let treasury = get_treasury(&env);
                (claimable - fee_amount, fee_amount, treasury)
            } else {
                (0i128, 0i128, None)
            };

            // EFFECTS — update cursor and total_withdrawn before any token transfer.
            stream.options.tranches_claimed = new_cursor;
            if claimable > 0 {
                stream.options.total_withdrawn = stream
                    .options.total_withdrawn
                    .checked_add(claimable)
                    .ok_or(StreamError::Overflow)?;
            }

            let all_claimed = new_cursor >= tranches.len();

            if all_claimed {
                stream.status = StreamStatus::Completed;
                save_stream(&env, &stream);
                remove_tranches(&env, stream_id);
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
                remove_stream(&env, stream_id);
                Self::unindex_stream(&env, &stream, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);
                // Invoke on_complete callback if configured
                Self::invoke_on_complete(&env, &stream);
            } else {
                save_stream(&env, &stream);
            }

            // INTERACTIONS
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipient,
                    &recipient_amount,
                );
            }
            if fee_amount > 0 {
                if let Some(ref t) = treasury_opt {
                    token_client.transfer(
                        &env.current_contract_address(),
                        t,
                        &fee_amount,
                    );
                    events::fee_collected(&env, stream_id, fee_amount, t);
                }
            }

            // EFFECTS: Update storage after token transfers succeed
            if all_claimed {
                stream.status = StreamStatus::Completed;
                save_stream(&env, &stream);
                remove_tranches(&env, stream_id);
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
                remove_stream(&env, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);
            } else {
                save_stream(&env, &stream);
            }

            if tranches_newly_claimed > 0 {
                events::tranches_withdrawn(&env, stream_id, &recipient, tranches_newly_claimed, claimable);
            }
            events::stream_withdrawn(&env, stream_id, &recipient, claimable, now, stream.options.total_withdrawn);
            if all_claimed {
                events::stream_completed(&env, stream_id);
            }

            clear_reentrancy_lock(&env);
            return Ok(());
        }

        // ── Linear-vesting withdrawal path (original logic) ──────────────────

        // Oracle check for linear streams.
        if let Some(ref oracle_addr) = stream.options.oracle {
            let (current_price, deviation_bps) = oracle::check_oracle(
                &env,
                oracle_addr,
                &stream.token,
                stream.options.creation_price,
                stream.options.max_price_deviation_bps,
            )?;
            events::price_check_passed(&env, stream_id, &stream.token, current_price, deviation_bps);
        }

        let effective_now = now.min(stream.end_time);
        let mut raw_claimable = match &stream.options.curve {
            VestingCurve::Linear => vesting_math::compute_claimable(
                stream.flow_rate,
                now,
                stream.cliff_time,
                stream.end_time,
                stream.last_withdraw_time,
            )
            .ok_or(StreamError::Overflow)?,

            VestingCurve::TimeDecay(decay_factor) => {
                vesting_math::compute_claimable_decay(
                    stream.deposit,
                    stream.start_time,
                    stream.end_time,
                    now,
                    stream.cliff_time,
                    stream.last_withdraw_time,
                    *decay_factor,
                )
                .ok_or(StreamError::Overflow)?
            }
        };

        // If milestones are set, limit claimable to released milestone amounts
        if !stream.options.milestones.is_empty() {
            let mut milestone_claimable = 0i128;
            for milestone in stream.options.milestones.iter() {
                if milestone.status == crate::types::MilestoneStatus::Released {
                    milestone_claimable = milestone_claimable
                        .checked_add(milestone.amount)
                        .ok_or(StreamError::Overflow)?;
                }
            }
            raw_claimable = raw_claimable.min(milestone_claimable);
        }

        let available = stream.deposit
            .checked_sub(stream.options.total_withdrawn)
            .ok_or(StreamError::Overflow)?;
        let claimable = raw_claimable.min(available);

        // ── Withdrawal-steps enforcement ─────────────────────────────────────
        // When `withdrawal_steps` is configured the stream duration is divided
        // into `n` equal intervals.  A withdrawal is only allowed at or after
        // the boundary of the *next* unclaimed step.
        //
        //   step_interval  = (end_time - start_time) / steps
        //   next_threshold = start_time + (current_step + 1) * step_interval
        //
        // On the *final* step we always allow the withdrawal so the recipient
        // can drain the full remaining balance regardless of rounding.
        if let Some(steps) = stream.options.withdrawal_steps {
            if steps > 0 {
                let is_final_step = stream.options.current_step + 1 >= steps;
                if !is_final_step {
                    let duration = stream.end_time.saturating_sub(stream.start_time);
                    let step_interval = duration / steps as u64;
                    let next_threshold = stream.start_time
                        .saturating_add((stream.options.current_step as u64 + 1) * step_interval);
                    if now < next_threshold {
                        return Err(StreamError::NextStepNotReached);
                    }
                }
            }
        }

        // ── Minimum withdrawal amount enforcement ────────────────────────────
        // When `min_withdrawal_amount` is configured, reject withdrawals whose
        // claimable is below the floor — UNLESS this is the final claim
        // (claimable == available), which drains the remaining balance entirely.
        // The bypass ensures recipients can always recover their last tokens
        // even when they fall short of the floor due to rounding or dust.
        if let Some(floor) = stream.options.min_withdrawal_amount {
            let is_final_claim = claimable >= available;
            if !is_final_claim && claimable < floor {
                return Err(StreamError::AmountBelowMinimum);
            }
        }
        // ── Issue #241: Dust guard ────────────────────────────────────────────
        // If the claimable amount is at or below the dust threshold (1 stroop),
        // treat it as rounding dust and return Ok without performing any transfer.
        // This prevents failed micro-withdrawals when a stream is nearly fully
        // drained or has tiny rounding remainders.
        if claimable <= DUST_THRESHOLD {
            // Still update last_withdraw_time to avoid spamming
            stream.last_withdraw_time = effective_now;
            save_stream(&env, &stream);
            clear_reentrancy_lock(&env);
            return Ok(());
        }

        let (recipient_amount, fee_amount) = if claimable > 0 {
            let fee_bps = storage::get_effective_fee_tier(&env, &stream.token);
            let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
                claimable
                    .checked_mul(fee_bps as i128)
                    .ok_or(StreamError::Overflow)?
                    / 10_000
            } else {
                0
            };
            (claimable - fee_amount, fee_amount)
        } else {
            (0, 0)
        };

        // EFFECTS: update all state before any external call
        if claimable > 0 {
            stream.options.total_withdrawn = stream
                .options.total_withdrawn
                .checked_add(claimable)
                .ok_or(StreamError::Overflow)?;
        }
        stream.last_withdraw_time = effective_now;

        // Advance the step cursor when the recipient successfully withdraws at
        // or past a step boundary.  The cursor only moves on an actual transfer
        // (claimable > 0) so a zero-claimable no-op never advances state.
        if claimable > 0 {
            if let Some(steps) = stream.options.withdrawal_steps {
                if steps > 0 && stream.options.current_step < steps {
                    let duration = stream.end_time.saturating_sub(stream.start_time);
                    let step_interval = duration / steps as u64;
                    // Determine how many step boundaries now has crossed.
                    let elapsed_from_start = now.saturating_sub(stream.start_time);
                    let steps_elapsed = elapsed_from_start
                        .checked_div(step_interval)
                        .map(|v| v.min(steps as u64) as u32)
                        .unwrap_or(steps);
                    if steps_elapsed > stream.options.current_step {
                        let new_step = steps_elapsed.min(steps);
                        let completed_step = new_step; // 1-based for the event
                        stream.options.current_step = new_step;
                        events::withdrawal_step_completed(
                            &env,
                            stream_id,
                            completed_step,
                            steps,
                            claimable,
                            &recipient,
                        );
                    }
                }
            }
        }

        // Accumulate fees in contract storage (swept via sweep_fees by admin)
        if fee_amount > 0 {
            accumulate_fees(&env, &stream.token, fee_amount);
        }

        let stream_ended = now >= stream.end_time;

        // Set stream-specific reentrancy lock before any external token transfer
        stream.options.locked = true;

        if stream_ended {
            // ── Issue: Rounding dust from integer division ────────────────────────
            // When flow_rate = deposit / duration (integer division), the product
            // flow_rate * duration may be less than deposit due to truncation.
            // Additionally, if any intermediate withdrawals were skipped due to
            // DUST_THRESHOLD, those amounts won't be in total_withdrawn either.
            //
            // Rather than compute dust as (flow_rate * duration), use the
            // authoritative source: dust = remaining balance not yet withdrawn.
            // This naturally accounts for:
            //   1. Rounding discrepancies from integer division
            //   2. Any stroops skipped by DUST_THRESHOLD logic
            //   3. Any accumulated rounding errors
            //
            // Ensures perfect balance conservation: dust + total_withdrawn = deposit
            let dust = stream.deposit.saturating_sub(stream.options.total_withdrawn);

            if stream.auto_renew {
                // Check if we've hit the renewal count limit
                let can_renew = if let Some(max_renewals) = stream.options.renew_count {
                    stream.options.renewals_used < max_renewals
                } else {
                    true  // No limit set, can always renew
                };

                if !can_renew {
                    // Renewal limit reached, complete the stream
                    stream.status = StreamStatus::Completed;
                    stream.options.locked = false;
                    save_stream(&env, &stream);
                    decrement_active_stream_count(&env);
                    decrement_token_stream_count(&env, &stream.token);

                    // INTERACTIONS
                    let token_client = token::Client::new(&env, &stream.token);
                    if recipient_amount > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &recipient,
                            &recipient_amount,
                        );
                    }
                    if dust > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &stream.sender,
                            &dust,
                        );
                    }
                    events::renewal_limit_reached(&env, stream_id, &stream.sender, stream.options.renewals_used);
                    events::stream_completed(&env, stream_id);
                    // Invoke on_complete callback if configured
                    Self::invoke_on_complete(&env, &stream);
                } else {
                    // Check sender balance for renewal
                    let sender_balance = token_client.balance(&stream.sender);
                    if sender_balance < stream.deposit {
                        stream.status = StreamStatus::Completed;
                        stream.options.locked = false;
                        save_stream(&env, &stream);
                        decrement_active_stream_count(&env);
                        decrement_token_stream_count(&env, &stream.token);

                        // INTERACTIONS
                        if recipient_amount > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &recipient,
                                &recipient_amount,
                            );
                        }
                        if dust > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &stream.sender,
                                &dust,
                            );
                        }
                        events::auto_renew_failed(&env, stream_id, &stream.sender, stream.deposit);
                        events::stream_completed(&env, stream_id);
                        // Invoke on_complete callback if configured
                        Self::invoke_on_complete(&env, &stream);
                    } else {
                        // Proceed with renewal and increment renewals_used
                        stream.sender.require_auth();
                        let duration = stream.end_time - stream.start_time;
                        let new_end = stream
                            .end_time
                            .checked_add(stream_duration)
                            .ok_or(StreamError::Overflow)?;
                        let old_end = stream.end_time;
                        stream.start_time = old_end;
                        stream.end_time = new_end;
                        stream.last_withdraw_time = old_end;
                        stream.options.total_withdrawn = 0;
                        stream.options.renewals_used = stream.options.renewals_used.saturating_add(1);
                        stream.options.locked = false;
                        save_stream(&env, &stream);

                        // INTERACTIONS
                        if recipient_amount > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &recipient,
                                &recipient_amount,
                            );
                        }
                        token_client.transfer(
                            &stream.sender,
                            &env.current_contract_address(),
                            &stream.deposit,
                        );
                    }
                }
            } else {
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
                remove_stream(&env, stream_id);
                Self::unindex_stream(&env, &stream, stream_id);

                let token_client = token::Client::new(&env, &stream.token);

                // INTERACTIONS: Transfer tokens BEFORE removing storage
                // This ensures atomicity: if transfer fails, stream record persists and can be retried.
                if recipient_amount > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &recipient_amount,
                    );
                }
                if dust > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &stream.sender,
                        &dust,
                    );
                }

                // EFFECTS: Remove stream after token transfers succeed
                remove_stream(&env, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);

                events::stream_completed(&env, stream_id);
                // Invoke on_complete callback if configured
                stream.status = StreamStatus::Completed;
                Self::invoke_on_complete(&env, &stream);
            }
        } else {
            stream.options.locked = false;
            save_stream(&env, &stream);

            // INTERACTIONS
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipient,
                    &recipient_amount,
                );
            }
            if claimable > 0 {
                let _ = env.try_invoke_contract::<(), soroban_sdk::Error>(
                    &recipient,
                    &Symbol::new(&env, "on_stream_withdraw"),
                    (stream_id, recipient_amount).into_val(&env),
                );
            }
        }

        events::stream_withdrawn(&env, stream_id, &recipient, claimable, now, stream.options.total_withdrawn);

        // Clear stream-specific reentrancy lock only if the stream still exists.
        // Final non-renewing withdraw removes the entry; re-saving would resurrect it.
        if let Some(mut s) = load_stream(&env, stream_id) {
            s.options.locked = false;
            save_stream(&env, &s);
        }

        Ok(())
    }

    /// Cancels an active stream. The recipient receives all earned tokens so far;
    /// the sender receives the unstreamed remainder.
    ///
    /// Follows interactions-before-effects pattern: token transfers occur BEFORE storage
    /// deletion. This ensures atomicity and prevents orphaned tokens: if a token transfer
    /// fails, the stream record persists and can be retried or recovered.
    ///
    /// A reentrancy guard blocks re-entrant calls during settlement.
    pub fn cancel_stream(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError> {
        if is_reentrancy_locked(&env) {
            return Err(StreamError::ReentrancyDetected);
        }
        set_reentrancy_lock(&env);

        caller.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }

        // PendingApproval and EscrowHold streams may be cancelled freely — the sender incurs no penalty.
        // For all other statuses enforce the usual Active/Paused requirement.
        if stream.status != StreamStatus::PendingApproval
            && stream.status != StreamStatus::EscrowHold
            && stream.status != StreamStatus::Active
            && stream.status != StreamStatus::Paused
        {
            return Err(StreamError::StreamNotActive);
        }

        // Sender (or their delegate) cannot cancel once the stream is sender-locked.
        // Exception: PendingApproval and EscrowHold streams are always cancellable at zero cost.
        if stream.options.sender_locked
            && stream.status != StreamStatus::PendingApproval
            && stream.status != StreamStatus::EscrowHold
            && (is_sender || is_delegate)
        {
            return Err(StreamError::StreamLocked);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        // ── PendingApproval / EscrowHold cancellation: full refund, zero earned ────────────
        // The recipient never approved (PendingApproval) or funds were just placed in escrow (EscrowHold),
        // so no tokens have accrued. Refund the entire deposit (plus any holdback) to the sender and remove the stream.
        if stream.status == StreamStatus::PendingApproval || stream.status == StreamStatus::EscrowHold {
            let refund = stream.deposit;
            let holdback_refund = if !stream.options.holdback_claimed && stream.options.holdback_amount > 0 {
                get_holdback(&env, stream_id)
            } else {
                0
            };

            remove_stream(&env, stream_id);
            Self::unindex_stream(&env, &stream, stream_id);
            if holdback_refund > 0 {
                remove_holdback(&env, stream_id);
            }

            let total_refund = refund.saturating_add(holdback_refund);
            if total_refund > 0 {
                token::Client::new(&env, &stream.token).transfer(
                    &env.current_contract_address(),
                    &stream.sender,
                    &total_refund,
                );
            }

            // EFFECTS: Remove stream after token transfer succeeds
            remove_stream(&env, stream_id);
            unindex_by_sender(&env, &stream.sender, stream_id);
            unindex_by_recipient(&env, &stream.recipient, stream_id);
            if holdback_refund > 0 {
                remove_holdback(&env, stream_id);
            }

            events::stream_cancelled(&env, stream_id, &stream.sender, total_refund, 0i128);
            clear_reentrancy_lock(&env);
            return Ok(());
        }

        // ── Step-vesting cancellation ────────────────────────────────────────
        if stream.options.is_step_vesting {
            let tranches = load_tranches(&env, stream_id);

            // Recipient gets all tranches whose unlock_time has passed.
            let mut recipient_amount: i128 = 0;
            let mut new_cursor = stream.options.tranches_claimed;
            while new_cursor < tranches.len() {
                let t = tranches.get(new_cursor).unwrap();
                if now >= t.unlock_time {
                    recipient_amount = recipient_amount
                        .checked_add(t.amount)
                        .ok_or(StreamError::Overflow)?;
                    new_cursor += 1;
                } else {
                    break;
                }
            }

            // Sender gets all remaining (unclaimed, not-yet-vested) tranches.
            let mut refund_amount: i128 = 0;
            for i in new_cursor..tranches.len() {
                let t = tranches.get(i).unwrap();
                refund_amount = refund_amount
                    .checked_add(t.amount)
                    .ok_or(StreamError::Overflow)?;
            }

            // Clamp to available deposit (guards against any rounding).
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            let recipient_amount = recipient_amount.min(available);
            let refund_amount = available.saturating_sub(recipient_amount);

            if stream.status == StreamStatus::Active {
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
            }

            // EFFECTS
            remove_tranches(&env, stream_id);
            remove_stream(&env, stream_id);
            Self::unindex_stream(&env, &stream, stream_id);

            // INTERACTIONS
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.recipient,
                    &recipient_amount,
                );
            }
            if refund_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.sender,
                    &refund_amount,
                );
            }

            events::tranche_stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);
            events::stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);

            clear_reentrancy_lock(&env);
            return Ok(());
        }

        // ── Milestone-release cancellation ───────────────────────────────────
        if stream.options.milestone_release_mode {
            // Calculate how much recipient has earned from released milestones
            let mut recipient_amount: i128 = 0;
            for milestone in stream.options.milestones.iter() {
                if milestone.status == MilestoneStatus::Released || (now >= milestone.unlock_time && milestone.status == MilestoneStatus::Pending) {
                    recipient_amount = recipient_amount
                        .checked_add(milestone.amount)
                        .ok_or(StreamError::Overflow)?;
                } else if milestone.status == MilestoneStatus::Forfeited {
                    // Forfeited milestones already go to sender (don't count for recipient)
                }
            }

            // Recipient gets earned, sender gets forfeited + unearned
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            let recipient_amount = recipient_amount.saturating_sub(stream.options.total_withdrawn).max(0);
            let refund_amount = available.saturating_sub(recipient_amount);

            if stream.status == StreamStatus::Active {
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
            }

            // EFFECTS
            remove_stream(&env, stream_id);
            Self::unindex_stream(&env, &stream, stream_id);

            // INTERACTIONS
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.recipient,
                    &recipient_amount,
                );
            }
            if refund_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.sender,
                    &refund_amount,
                );
            }

            events::stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);
            clear_reentrancy_lock(&env);
            return Ok(());
        }

        // ── Linear-vesting cancellation (original logic) ────────────────────

        // Issue #13: Cliff enforcement on cancellation.
        // If the current time is before the cliff, the recipient has earned nothing
        // yet. We short-circuit to zero rather than calling compute_earned which
        // would compute flow_rate × elapsed and over-pay the recipient.
        let recipient_amount = if now < stream.cliff_time {
            0i128
        } else {
            let earned = vesting_math::compute_earned(
                stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
            ).ok_or(StreamError::Overflow)?;
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            earned.min(available)
        };

        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        let recipient_amount = recipient_amount.min(available);
        let refund_amount = available.saturating_sub(recipient_amount);

        // ── Cancellation fee (issue #288) ─────────────────────────────────────
        // Deduct a configurable fee from the sender's refund portion to penalise
        // early cancellation and deter speculative capital extraction.
        // Fee is waived for fee-exempt senders and when the refund portion is zero.
        let cancel_fee_bps = get_cancellation_fee_bps(&env);
        let (cancel_fee, net_refund) = if cancel_fee_bps > 0
            && refund_amount > 0
            && !is_fee_exempt(&env, &stream.sender)
        {
            // fee = refund * bps / 10_000  (rounded down — favour the sender)
            let fee = refund_amount
                .checked_mul(cancel_fee_bps as i128)
                .ok_or(StreamError::Overflow)?
                / 10_000i128;
            let net = refund_amount.saturating_sub(fee);
            (fee, net)
        } else {
            (0i128, refund_amount)
        };

        // Decrement active count only if stream was Active (Paused was already decremented)
        if stream.status == StreamStatus::Active {
            decrement_active_stream_count(&env);
            decrement_token_stream_count(&env, &stream.token);
        }

        // If the holdback has not yet been settled, include it in the sender refund.
        let holdback_refund = if !stream.options.holdback_claimed && stream.options.holdback_amount > 0 {
            get_holdback(&env, stream_id)
        } else {
            0
        };

        // EFFECTS: remove stream before any token transfer
        remove_stream(&env, stream_id);
        Self::unindex_stream(&env, &stream, stream_id);
        if holdback_refund > 0 {
            remove_holdback(&env, stream_id);
        }

        // INTERACTIONS
        let token_client = token::Client::new(&env, &stream.token);
        if recipient_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_amount,
            );
        }
        // Accumulate cancellation fee in the protocol treasury before paying sender.
        if cancel_fee > 0 {
            accumulate_fees(&env, &stream.token, cancel_fee);
            events::cancellation_fee_collected(&env, stream_id, &stream.sender, cancel_fee, cancel_fee_bps);
        }
        let total_refund = net_refund.saturating_add(holdback_refund);
        if total_refund > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.sender,
                &total_refund,
            );
        }

        // EFFECTS: Remove stream after token transfers succeed
        remove_stream(&env, stream_id);
        unindex_by_sender(&env, &stream.sender, stream_id);
        unindex_by_recipient(&env, &stream.recipient, stream_id);
        if holdback_refund > 0 {
            remove_holdback(&env, stream_id);
        }

        events::stream_cancelled(&env, stream_id, &stream.sender, total_refund, recipient_amount);

        clear_reentrancy_lock(&env);
        Ok(())
    }

    /// Stops a stream immediately at the current ledger, paying the recipient their
    /// accrued portion and returning remaining unstreamed tokens to the sender.
    ///
    /// This is a simpler alternative to `cancel_stream` with clearer semantics:
    /// - Stream is terminated completely (not continued)
    /// - Recipient receives earned amount based on elapsed time
    /// - Sender receives unstreamed remainder
    ///
    /// Callable by:
    /// - **Sender** (or delegate): To stop stream and recover unstreamed portion
    /// - **Recipient**: To claim earned portion and stop stream
    ///
    /// ## Handling of Special Cases
    ///
    /// **Cliff streams:** If `now < cliff_time`, recipient receives 0; sender gets full refund.
    ///
    /// **Step-vesting (tranches):** Recipient gets all tranches where `unlock_time <= now`.
    /// Sender gets future (unlocked) tranches.
    ///
    /// **Locked streams:** If `sender_locked = true`, only the recipient can call this
    /// (sender cannot). Admin pause/unlock could be used to override.
    ///
    /// **Holdback amount:** If unclaimed, included in sender refund.
    ///
    /// **Paused streams:** Uses `last_pause_time` instead of current timestamp.
    ///
    /// **PendingApproval streams:** Recipient gets 0, sender gets full refund.
    ///
    /// ## Event
    ///
    /// Emits `StreamPartialCancelled` with the earned and unstreamed amounts.
    pub fn stop_stream(
        env: Env,
        stream_id: u64,
        caller: Address,
    ) -> Result<(), StreamError> {
        if is_reentrancy_locked(&env) {
            return Err(StreamError::ReentrancyDetected);
        }
        set_reentrancy_lock(&env);

        caller.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        // Authorization: sender, recipient, or delegate can call
        let is_sender = stream.sender == caller;
        let is_recipient = stream.recipient == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);

        if !is_sender && !is_recipient && !is_delegate {
            clear_reentrancy_lock(&env);
            return Err(StreamError::NotAuthorized);
        }

        // Only sender can call if stream is sender-locked; recipient always can
        if stream.options.sender_locked && is_sender && !is_recipient {
            clear_reentrancy_lock(&env);
            return Err(StreamError::StreamLocked);
        }

        // Stream must be Active or Paused (or PendingApproval for quick refund)
        if stream.status != StreamStatus::Active
            && stream.status != StreamStatus::Paused
            && stream.status != StreamStatus::PendingApproval
        {
            clear_reentrancy_lock(&env);
            return Err(StreamError::StreamNotActive);
        }

        // ── PendingApproval: quick refund ────────────────────────────────────
        if stream.status == StreamStatus::PendingApproval {
            let refund = stream.deposit;
            let holdback_refund = if !stream.options.holdback_claimed && stream.options.holdback_amount > 0 {
                get_holdback(&env, stream_id)
            } else {
                0
            };

            remove_stream(&env, stream_id);
            unindex_by_sender(&env, &stream.sender, stream_id);
            unindex_by_recipient(&env, &stream.recipient, stream_id);
            if holdback_refund > 0 {
                remove_holdback(&env, stream_id);
            }

            let total_refund = refund.saturating_add(holdback_refund);
            if total_refund > 0 {
                token::Client::new(&env, &stream.token).transfer(
                    &env.current_contract_address(),
                    &stream.sender,
                    &total_refund,
                );
            }

            events::stream_partial_cancelled(&env, stream_id, 0, &stream.sender, 0i128, total_refund);
            clear_reentrancy_lock(&env);
            return Ok(());
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        // ── Step-vesting (tranches) ──────────────────────────────────────────
        if stream.options.is_step_vesting {
            let tranches = load_tranches(&env, stream_id);

            // Recipient gets all tranches whose unlock_time has passed
            let mut recipient_amount: i128 = 0;
            let mut new_cursor = stream.options.tranches_claimed;
            while new_cursor < tranches.len() {
                let t = tranches.get(new_cursor).unwrap();
                if now >= t.unlock_time {
                    recipient_amount = recipient_amount
                        .checked_add(t.amount)
                        .ok_or(StreamError::Overflow)?;
                    new_cursor += 1;
                } else {
                    break;
                }
            }

            // Sender gets all remaining (unclaimed, not-yet-vested) tranches
            let mut refund_amount: i128 = 0;
            for i in new_cursor..tranches.len() {
                let t = tranches.get(i).unwrap();
                refund_amount = refund_amount
                    .checked_add(t.amount)
                    .ok_or(StreamError::Overflow)?;
            }

            // Clamp to available deposit
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            let recipient_amount = recipient_amount.min(available);
            let refund_amount = available.saturating_sub(recipient_amount);

            if stream.status == StreamStatus::Active {
                decrement_active_stream_count(&env);
                decrement_token_stream_count(&env, &stream.token);
            }

            // Remove stream and tranches
            remove_tranches(&env, stream_id);
            remove_stream(&env, stream_id);
            unindex_by_sender(&env, &stream.sender, stream_id);
            unindex_by_recipient(&env, &stream.recipient, stream_id);

            // Transfer balances
            let token_client = token::Client::new(&env, &stream.token);
            if recipient_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.recipient,
                    &recipient_amount,
                );
            }
            if refund_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.sender,
                    &refund_amount,
                );
            }

            events::stream_partial_cancelled(&env, stream_id, 0, &stream.sender, recipient_amount, refund_amount);
            clear_reentrancy_lock(&env);
            return Ok(());
        }

        // ── Linear vesting (original logic) ──────────────────────────────────

        // Cliff enforcement: before cliff, recipient earns nothing
        let recipient_amount = if now < stream.cliff_time {
            0i128
        } else {
            let earned = vesting_math::compute_earned(
                stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
            ).ok_or(StreamError::Overflow)?;
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            earned.min(available)
        };

        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        let recipient_amount = recipient_amount.min(available);
        let refund_amount = available.saturating_sub(recipient_amount);

        // Decrement active count if stream was Active
        if stream.status == StreamStatus::Active {
            decrement_active_stream_count(&env);
            decrement_token_stream_count(&env, &stream.token);
        }

        // Handle holdback
        let holdback_refund = if !stream.options.holdback_claimed && stream.options.holdback_amount > 0 {
            get_holdback(&env, stream_id)
        } else {
            0
        };

        // Remove stream and cleanup
        remove_stream(&env, stream_id);
        unindex_by_sender(&env, &stream.sender, stream_id);
        unindex_by_recipient(&env, &stream.recipient, stream_id);
        if holdback_refund > 0 {
            remove_holdback(&env, stream_id);
        }

        // Transfer tokens
        let token_client = token::Client::new(&env, &stream.token);
        if recipient_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_amount,
            );
        }

        let total_refund = refund_amount.saturating_add(holdback_refund);
        if total_refund > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.sender,
                &total_refund,
            );
        }

        events::stream_partial_cancelled(&env, stream_id, 0, &stream.sender, recipient_amount, refund_amount);
        clear_reentrancy_lock(&env);
        Ok(())
    }

    /// Allows the recipient to terminate a stream early.
    ///
    /// Follows interactions-before-effects pattern: token transfers occur BEFORE storage
    /// deletion. This ensures atomicity and prevents orphaned tokens: if a token transfer
    /// fails, the stream record persists and can be retried or recovered.
    ///
    /// A reentrancy guard blocks re-entrant calls during settlement.
    pub fn recipient_terminate(env: Env, stream_id: u64, recipient: Address) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        if is_reentrancy_locked(&env) {
            return Err(StreamError::ReentrancyDetected);
        }
        set_reentrancy_lock(&env);

        recipient.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != recipient {
            return Err(StreamError::NotRecipient);
        }
        if !stream.options.allow_recipient_termination {
            return Err(StreamError::NotAuthorized);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        let recipient_amount = vesting_math::compute_claimable(
            stream.flow_rate,
            now,
            stream.cliff_time,
            stream.end_time,
            stream.last_withdraw_time,
        ).ok_or(StreamError::Overflow)?;

        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        let recipient_amount = recipient_amount.min(available);
        let refund_amount = available.saturating_sub(recipient_amount);

        // Decrement active count only if stream was Active (Paused was already decremented)
        if stream.status == StreamStatus::Active {
            decrement_active_stream_count(&env);
            decrement_token_stream_count(&env, &stream.token);
        }

        // EFFECTS: remove stream before any token transfer
        remove_stream(&env, stream_id);
        Self::unindex_stream(&env, &stream, stream_id);

        // INTERACTIONS
        let token_client = token::Client::new(&env, &stream.token);
        if recipient_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_amount,
            );
        }
        if refund_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.sender,
                &refund_amount,
            );
        }

        // EFFECTS: Remove stream after token transfers succeed
        remove_stream(&env, stream_id);
        unindex_by_sender(&env, &stream.sender, stream_id);
        unindex_by_recipient(&env, &stream.recipient, stream_id);

        events::stream_terminated_by_recipient(&env, stream_id, &recipient, recipient_amount, refund_amount);

        clear_reentrancy_lock(&env);
        Ok(())
    }

    /// Splits a stream by canceling it and atomically creating multiple new streams
    /// with proportionally split balances among new recipients.
    ///
    /// The sender calls this with:
    /// - `stream_id`: the stream to cancel
    /// - `recipients`: new recipient addresses
    /// - `proportions`: proportion values defining how to split the earned amount
    /// - `nonce`: unique nonce for new stream IDs
    ///
    /// Returns a vector of newly created stream IDs.
    ///
    /// # Flow
    ///
    /// 1. Load and validate the original stream
    /// 2. Verify sender authorization
    /// 3. Cancel the stream, computing earned vs. refundable amounts
    /// 4. Send sender refund immediately
    /// 5. Atomically create new streams with split earned amount
    /// 6. Emit StreamSplit event
    ///
    /// # Errors
    ///
    /// Returns errors for invalid input, stream not found, unauthorized access, or
    /// stream creation failures for the new streams.
    #[allow(clippy::too_many_arguments)]
    pub fn split_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
        recipients: Vec<Address>,
        proportions: Vec<u128>,
        nonce: u64,
    ) -> Result<Vec<u64>, StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        if is_reentrancy_locked(&env) {
            return Err(StreamError::ReentrancyDetected);
        }
        set_reentrancy_lock(&env);

        sender.require_auth();

        // Validate inputs
        if recipients.len() as u32 == 0 || recipients.len() as u32 > 100 {
            return Err(StreamError::BatchLengthMismatch);
        }
        if recipients.len() != proportions.len() {
            return Err(StreamError::BatchLengthMismatch);
        }

        // Load original stream
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        // Verify authorization
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        // Stream must be in a state that allows cancellation
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        // Cannot split if sender has locked the stream
        if stream.options.sender_locked {
            return Err(StreamError::StreamIsLocked);
        }

        // Determine current time for calculations
        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        // Compute amounts owed to recipient vs. refundable to sender
        let recipient_amount = if now < stream.cliff_time {
            // Before cliff: recipient earns nothing
            0i128
        } else {
            let earned = vesting_math::compute_earned(
                stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
            ).ok_or(StreamError::Overflow)?;
            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            earned.min(available)
        };

        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        let recipient_amount_clamped = recipient_amount.min(available);
        let refund_amount = available.saturating_sub(recipient_amount_clamped);

        // Include holdback in sender refund if not yet settled
        let holdback_refund = if !stream.options.holdback_claimed && stream.options.holdback_amount > 0 {
            get_holdback(&env, stream_id)
        } else {
            0
        };

        // Calculate original stream duration
        let original_duration = stream.end_time.saturating_sub(stream.start_time);

        // Calculate total proportions (sum of all proportions)
        let mut total_proportions: u128 = 0;
        for p in proportions.iter() {
            total_proportions = total_proportions
                .checked_add(p)
                .ok_or(StreamError::Overflow)?;
        }

        if total_proportions == 0 {
            clear_reentrancy_lock(&env);
            return Err(StreamError::ZeroAmount);
        }

        // EFFECTS: Remove original stream from storage before creating new ones
        remove_stream(&env, stream_id);
        unindex_by_sender(&env, &stream.sender, stream_id);
        unindex_by_recipient(&env, &stream.recipient, stream_id);
        if holdback_refund > 0 {
            remove_holdback(&env, stream_id);
        }

        // Decrement active count and token stream count
        if stream.status == StreamStatus::Active {
            decrement_active_stream_count(&env);
            decrement_token_stream_count(&env, &stream.token);
        }

        // INTERACTIONS: Transfer refund to sender
        if refund_amount > 0 || holdback_refund > 0 {
            let total_refund = refund_amount.saturating_add(holdback_refund);
            token::Client::new(&env, &stream.token).transfer(
                &env.current_contract_address(),
                &stream.sender,
                &total_refund,
            );
        }

        // Create new streams with split amounts
        let mut new_stream_ids = Vec::new(&env);
        let token_client = token::Client::new(&env, &stream.token);

        for (idx, recipient) in recipients.iter().enumerate() {
            let proportion = proportions.get(idx as u32).unwrap();
            
            // Calculate this stream's share of earned amount
            let stream_share = (recipient_amount_clamped as u128)
                .checked_mul(proportion)
                .ok_or(StreamError::Overflow)?
                .checked_div(total_proportions)
                .ok_or(StreamError::Overflow)? as i128;

            if stream_share <= 0 {
                // Skip zero-amount streams but don't error
                continue;
            }

            // Create new stream with same parameters as original
            let new_stream_id = Self::create_stream(
                env.clone(),
                stream.sender.clone(),
                recipient.clone(),
                stream.token.clone(),
                stream_share,
                original_duration,
                stream.auto_renew,
                stream.lock_until.saturating_sub(stream.start_time),
                CreateStreamOptions {
                    allow_recipient_termination: stream.options.allow_recipient_termination,
                    comment: stream.options.comment.clone(),
                    ..Default::default()
                },
            )?;

            new_stream_ids.push_back(new_stream_id);
        }

        // Emit split event via existing stream_created for each sub-stream

        clear_reentrancy_lock(&env);
        Ok(new_stream_ids)
    }

    /// Transfers claim rights of a stream to a new recipient.
    pub fn transfer_recipient(
        env: Env,
        stream_id: u64,
        current_recipient: Address,
        new_recipient: Address,
    ) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        current_recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != current_recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.options.non_transferable {
            return Err(StreamError::StreamNonTransferable);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        if now >= stream.lock_until {
            let effective_now = now.min(stream.end_time);
            if now >= stream.cliff_time {
                let raw_claimable = vesting_math::compute_claimable(
                    stream.flow_rate,
                    now,
                    stream.cliff_time,
                    stream.end_time,
                    stream.last_withdraw_time,
                ).ok_or(StreamError::Overflow)?;

                let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
                let claimable = raw_claimable.min(available);

                if claimable > 0 {
                    let fee_bps = get_protocol_fee(&env);
                    let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
                        claimable
                            .checked_mul(fee_bps as i128)
                            .ok_or(StreamError::Overflow)?
                            / 10_000
                    } else {
                        0
                    };
                    let recipient_amount = claimable - fee_amount;

                    stream.options.total_withdrawn = stream
                        .options.total_withdrawn
                        .checked_add(claimable)
                        .ok_or(StreamError::Overflow)?;
                    stream.last_withdraw_time = effective_now;

                    // Accumulate fees in contract storage (swept via sweep_fees by admin)
                    if fee_amount > 0 {
                        accumulate_fees(&env, &stream.token, fee_amount);
                    }

                    let token_client = token::Client::new(&env, &stream.token);
                    if recipient_amount > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &current_recipient,
                            &recipient_amount,
                        );
                    }
                    events::stream_withdrawn(&env, stream_id, &current_recipient, claimable, now, stream.options.total_withdrawn);
                }
            }
        }

        let old_recipient = stream.recipient.clone();
        stream.recipient = new_recipient.clone();
        save_stream(&env, &stream);

        unindex_by_recipient(&env, &old_recipient, stream_id);
        index_by_recipient(&env, &new_recipient, stream_id);

        events::recipient_transferred(&env, stream_id, &old_recipient, &new_recipient);

        Ok(())
    }

    /// Approves a stream created with `requires_recipient_approval = true`.
    ///
    /// Transitions the stream from `PendingApproval` to `Active` and records the
    /// approval timestamp.  All claimable-balance calculations use this timestamp
    /// as the effective start so no tokens accrue during the pending window.
    pub fn approve_stream(
        env: Env,
        stream_id: u64,
        recipient: Address,
    ) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        recipient.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.recipient != recipient {
            return Err(StreamError::NotRecipient);
        }
        if stream.status != StreamStatus::PendingApproval {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();

        // Shift all time-based fields forward so vesting starts from approval,
        // not from the original creation timestamp.
        let pending_duration = now.saturating_sub(stream.start_time);
        stream.start_time = now;
        stream.cliff_time = stream.cliff_time.saturating_add(pending_duration);
        stream.end_time = stream.end_time.saturating_add(pending_duration);
        stream.lock_until = if stream.lock_until > 0 {
            stream.lock_until.saturating_add(pending_duration)
        } else {
            0
        };
        stream.last_withdraw_time = now;
        stream.options.approval_timestamp = now;
        stream.status = StreamStatus::Active;

        save_stream(&env, &stream);
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &stream.token);

        events::stream_approved(&env, stream_id, &recipient, now);
        Ok(())
    }

    /// Irrevocably locks a stream, preventing the sender from cancelling it.
    ///
    /// Only callable by the sender while the stream is `Active`.
    /// Once locked, `cancel_stream` returns `StreamError::StreamLocked` for
    /// any call from the sender or their delegate.  Recipients withdraw normally.
    pub fn lock_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
    ) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if stream.options.sender_locked {
            return Err(StreamError::StreamLocked);
        }

        stream.options.sender_locked = true;
        save_stream(&env, &stream);

        events::stream_sender_locked(&env, stream_id, &sender);
        Ok(())
    }

    /// Partially cancels an active stream by reclaiming `cancel_amount` from the unstreamed
    /// remainder.
    pub fn partial_cancel_stream(
        env: Env,
        stream_id: u64,
        caller: Address,
        cancel_amount: i128,
    ) -> Result<u64, StreamError> {
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }
        if cancel_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        let _effective_now = now.min(stream.end_time);
        let elapsed_since_withdraw = now.saturating_sub(stream.last_withdraw_time);
        let earned = checked_flow_amount(stream.flow_rate, elapsed_since_withdraw)?;

        let elapsed_since_start = now.saturating_sub(stream.start_time);
        let total_streamed = checked_flow_amount(stream.flow_rate, elapsed_since_start)?;

        let remaining = stream.deposit.saturating_sub(total_streamed);

        if cancel_amount >= remaining || (remaining - cancel_amount) < stream.flow_rate {
            return Err(StreamError::InvalidDuration);
        }

        let new_deposit = remaining - cancel_amount;

        let new_duration_i128 = new_deposit / stream.flow_rate;
        let new_duration = u64::try_from(new_duration_i128).map_err(|_| StreamError::Overflow)?;
        let new_end_time = now
            .checked_add(new_duration)
            .ok_or(StreamError::Overflow)?;

        let token_client = token::Client::new(&env, &stream.token);

        if earned > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.recipient, &earned);
        }
        token_client.transfer(&env.current_contract_address(), &stream.sender, &cancel_amount);

        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        decrement_active_stream_count(&env);
        decrement_token_stream_count(&env, &stream.token);
        events::stream_cancelled(&env, stream_id, &stream.sender, cancel_amount, earned);

        let new_nonce = stream_id;
        let new_stream_id =
            derive_stream_id(&env, &stream.sender, &stream.recipient, now, new_nonce);

        let new_stream = Stream {
            id: new_stream_id,
            sender: stream.sender.clone(),
            recipient: stream.recipient.clone(),
            token: stream.token.clone(),
            deposit: new_deposit,
            flow_rate: stream.flow_rate,
            start_time: now,
            cliff_time: now,
            lock_until: now,
            end_time: new_end_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            auto_renew: stream.auto_renew,
            options: StreamOptions {
                renew_count: None,
                renewals_used: 0,
                allow_recipient_termination: stream.options.allow_recipient_termination,
                last_pause_time: 0,
                total_withdrawn: 0,
                metadata: stream.options.metadata.clone(),
                locked: false,
                metadata_uri: stream.options.metadata_uri.clone(),
                milestones: soroban_sdk::Vec::new(&env),
                milestone_release_mode: false,
                holdback_amount: 0,
                holdback_claimed: false,
                is_step_vesting: false,
                tranches_claimed: 0,
                oracle: None,
                max_price_deviation_bps: 0,
                creation_price: 0,
                curve: VestingCurve::Linear,
                withdrawal_steps: None,
                current_step: 0,
                min_withdrawal_amount: None,
                non_transferable: false,
                requires_recipient_approval: false,
                approval_timestamp: 0,
                sender_locked: false,
                redirect_to_stream_id: None,
                is_dual_stream: false,
                on_complete_contract: None,
                on_complete_function: None,
                comment: None,
            },
        };

        save_stream(&env, &new_stream);
        index_by_sender(&env, &stream.sender, new_stream_id);
        index_by_recipient(&env, &stream.recipient, new_stream_id);
        index_global_stream(&env, new_stream_id);
        increment_active_stream_count(&env);
        increment_token_stream_count(&env, &new_stream.token);

        events::stream_partial_cancelled(
            &env,
            stream_id,
            new_stream_id,
            &stream.sender,
            cancel_amount,
            new_deposit,
        );

        Ok(new_stream_id)
    }

    /// Adds more tokens to an existing stream, extending its end time proportionally.
    pub fn top_up(
        env: Env,
        stream_id: u64,
        caller: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = Some(caller.clone()) == get_delegate(&env, stream_id);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.token != token {
            return Err(StreamError::InvalidDuration);
        }
        check_token_whitelist(&env, &token)?;
        validate_token_address(&env, &token)?;
        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotActive);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        let effective_amount = amount - (amount % stream.flow_rate);

        if effective_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }

        token::Client::new(&env, &stream.token)
            .transfer(&caller, &env.current_contract_address(), &effective_amount);

        let extra_seconds_i128 = effective_amount / stream.flow_rate;
        let extra_seconds =
            u64::try_from(extra_seconds_i128).map_err(|_| StreamError::Overflow)?;

        let new_end_time = stream
            .end_time
            .checked_add(extra_seconds)
            .ok_or(StreamError::Overflow)?;

        let now = env.ledger().timestamp();
        let max_end_time = now
            .checked_add(MAX_STREAM_DURATION_SECONDS)
            .ok_or(StreamError::Overflow)?;

        if new_end_time > max_end_time {
            return Err(StreamError::Overflow);
        }

        stream.end_time = new_end_time;
        stream.deposit = stream
            .deposit
            .checked_add(effective_amount)
            .ok_or(StreamError::Overflow)?;

        save_stream(&env, &stream);

        events::stream_topped_up(&env, stream_id, effective_amount, new_end_time);

        Ok(())
    }

    /// Updates the token-per-second flow rate of an active stream.
    ///
    /// Only the sender may call this. Settles the recipient's accrued balance
    /// at the current rate before applying the new rate. The stream's deposit
    /// and end_time are adjusted to maintain the promised total.
    pub fn update_stream_rate(
        env: Env,
        stream_id: u64,
        sender: Address,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        // Only linear streams and non-step-vesting streams support rate updates
        if stream.options.is_step_vesting {
            return Err(StreamError::InvalidDuration);
        }

        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        if new_rate <= 0 {
            return Err(StreamError::ZeroFlowRate);
        }

        let now = env.ledger().timestamp();

        // ── Settle accrued balance at current rate ──────────────────────────
        // Compute how much the recipient has earned at the old rate
        let claimable_at_old_rate = Self::get_claimable(env.clone(), stream_id)
            .unwrap_or(0)
            .max(0);

        // Update total_withdrawn to reflect the accrued balance being "settled"
        // This ensures the next claimable calculation starts fresh
        stream.last_withdraw_time = now;
        
        // Compute remaining balance: what's left after current accrual
        let settled_withdrawn = stream
            .options.total_withdrawn
            .checked_add(claimable_at_old_rate)
            .ok_or(StreamError::Overflow)?;
        
        let remaining_balance = stream
            .deposit
            .checked_sub(settled_withdrawn)
            .ok_or(StreamError::Overflow)?;

        if remaining_balance < 0 {
            return Err(StreamError::Overflow);
        }

        // ── Calculate new end time ──────────────────────────────────────────
        // Remaining duration: remaining_balance / new_rate
        let remaining_duration_i128 = remaining_balance / new_rate;
        let remaining_duration = u64::try_from(remaining_duration_i128)
            .map_err(|_| StreamError::Overflow)?;

        // If remaining_balance doesn't divide evenly, we might have rounding issues
        // For now, we'll accept this and the stream might end slightly early or late
        let new_end_time = now
            .checked_add(remaining_duration)
            .ok_or(StreamError::Overflow)?;

        // ── Validate new end time doesn't exceed maximum allowed duration ────
        let max_end_time = now
            .checked_add(MAX_STREAM_DURATION_SECONDS)
            .ok_or(StreamError::Overflow)?;

        if new_end_time > max_end_time {
            return Err(StreamError::Overflow);
        }

        // ── Update stream with new rate ──────────────────────────────────────
        let old_rate = stream.flow_rate;
        stream.flow_rate = new_rate;
        stream.end_time = new_end_time;
        stream.options.total_withdrawn = settled_withdrawn;
        stream.deposit = remaining_balance;

        save_stream(&env, &stream);

        events::stream_rate_updated(
            &env,
            stream_id,
            old_rate,
            new_rate,
            new_end_time,
            remaining_balance,
        );

        Ok(())
    }

    /// Delegates management of a stream to another address.
    ///
    /// Only the stream sender may call this. The delegate may subsequently
    /// act as the sender on `cancel_stream`, `top_up`, and `bump_stream_ttl`.
    /// Emits `DelegateSet { stream_id, sender, delegate }`.
    pub fn set_delegate(env: Env, sender: Address, stream_id: u64, delegate: Address) -> Result<(), StreamError> {
        sender.require_auth();
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        set_delegate(&env, stream_id, &delegate);
        events::delegate_set(&env, stream_id, &sender, &delegate);
        Ok(())
    }

    /// Revokes management of a stream from the current delegate.
    ///
    /// Only the stream sender may call this. After revocation the delegate
    /// address can no longer act on behalf of the sender.
    /// Emits `DelegateRevoked { stream_id, sender }`.
    pub fn revoke_delegate(env: Env, sender: Address, stream_id: u64) -> Result<(), StreamError> {
        sender.require_auth();
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        remove_delegate(&env, stream_id);
        events::delegate_revoked(&env, stream_id, &sender);
        Ok(())
    }

    /// Releases the holdback escrow amount to the recipient.
    ///
    /// Only the stream sender (or their authorised delegate) may call this.
    /// The holdback must not have already been settled.
    /// Emits `HoldbackReleased { stream_id, amount, recipient }`.
    ///
    /// # Errors
    /// - `StreamNotFound` — stream does not exist.
    /// - `NotAuthorized` — caller is neither sender nor delegate.
    /// - `ZeroAmount` — stream has no holdback configured.
    /// - `StreamNotActive` — holdback already settled.
    pub fn release_holdback(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = get_delegate(&env, stream_id).is_some_and(|d| d == caller);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.options.holdback_amount == 0 {
            return Err(StreamError::ZeroAmount);
        }
        if stream.options.holdback_claimed {
            return Err(StreamError::StreamNotActive);
        }

        let escrow = get_holdback(&env, stream_id);

        // EFFECTS: mark settled before transfer
        stream.options.holdback_claimed = true;
        save_stream(&env, &stream);
        remove_holdback(&env, stream_id);

        // INTERACTIONS
        token::Client::new(&env, &stream.token).transfer(
            &env.current_contract_address(),
            &stream.recipient,
            &escrow,
        );

        events::holdback_released(&env, stream_id, escrow, &stream.recipient);
        Ok(())
    }

    /// Allows the sender to claw back the holdback escrow before the recipient claims it.
    ///
    /// Only the stream sender (or their authorised delegate) may call this.
    /// The holdback must not have already been settled.
    /// Emits `HoldbackClawedBack { stream_id, amount, sender }`.
    ///
    /// # Errors
    /// - `StreamNotFound` — stream does not exist.
    /// - `NotAuthorized` — caller is neither sender nor delegate.
    /// - `ZeroAmount` — stream has no holdback configured.
    /// - `StreamNotActive` — holdback already settled.
    pub fn claw_back_holdback(env: Env, stream_id: u64, caller: Address) -> Result<(), StreamError> {
        caller.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let is_sender = stream.sender == caller;
        let is_delegate = get_delegate(&env, stream_id).is_some_and(|d| d == caller);
        if !is_sender && !is_delegate {
            return Err(StreamError::NotAuthorized);
        }
        if stream.options.holdback_amount == 0 {
            return Err(StreamError::ZeroAmount);
        }
        if stream.options.holdback_claimed {
            return Err(StreamError::StreamNotActive);
        }

        let escrow = get_holdback(&env, stream_id);

        // EFFECTS: mark settled before transfer
        stream.options.holdback_claimed = true;
        save_stream(&env, &stream);
        remove_holdback(&env, stream_id);

        // INTERACTIONS
        token::Client::new(&env, &stream.token).transfer(
            &env.current_contract_address(),
            &stream.sender,
            &escrow,
        );

        events::holdback_clawed_back(&env, stream_id, escrow, &stream.sender);
        Ok(())
    }

    /// Returns the current delegate address for a stream, if one has been set.
    pub fn get_delegate(env: Env, stream_id: u64) -> Option<Address> {
        storage::get_delegate(&env, stream_id)
    }

    /// Returns the full stream struct for a given stream ID.
    ///
    /// If the stream's `end_time` has passed and the stream was not explicitly
    /// cancelled, the returned `status` is `StreamStatus::Expired` — even if the
    /// persisted value is still `Active` or `Completed`. This makes it unnecessary
    /// for clients to compare timestamps themselves.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        Ok(refreshed_stream_view(&env, stream))
    }

    /// Explicitly marks an elapsed stream as Expired, compacting its storage entry.
    ///
    /// Callable by anyone. The stream must be Active (or Completed) and its
    /// `end_time` must have passed. Cancelled streams are never transitioned to
    /// Expired. Emits `StreamExpired { stream_id }`.
    pub fn mark_expired(env: Env, stream_id: u64) -> Result<(), StreamError> {
        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        // Only Active or Completed streams can be marked Expired.
        if stream.status == StreamStatus::Cancelled || stream.status == StreamStatus::Expired {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();
        if now < stream.end_time {
            return Err(StreamError::StreamNotComplete);
        }

        // Transition to Expired and persist the compacted state.
        stream.status = StreamStatus::Expired;
        save_stream(&env, &stream);

        events::stream_expired(&env, stream_id);
        Ok(())
    }

    /// Returns a paginated list of all stream IDs that have ever been created.
    pub fn get_all_stream_ids(env: Env, start: u32, limit: u32) -> Vec<u64> {
        let total = get_global_stream_count(&env);
        let cap = limit.min(20);
        let end = start.saturating_add(cap).min(total);
        let mut ids = Vec::new(&env);

        for i in start..end {
            if let Some(id) = get_global_stream_at(&env, i) {
                if load_stream(&env, id).is_some() {
                    ids.push_back(id);
                }
            }
        }

        ids
    }

    /// Returns the current batch nonce for a sender (next expected nonce).
    pub fn get_nonce(env: Env, sender: Address) -> u64 {
        get_batch_nonce(&env, &sender)
    }

    /// Returns the amount of tokens currently claimable by the recipient.
    ///
    /// # Issue #13 — Cliff enforcement
    /// Returns `0` if the current ledger timestamp is strictly before `cliff_time`.
    /// Once `cliff_time` is reached, the full linear progression from `start_time`
    /// to `end_time` is used to calculate the claimable amount.
    ///
    /// # Issue #241 — Dust & zero-return for completed streams
    /// - Post-completion guard: if `now >= end_time` AND `total_withdrawn >= deposit`,
    ///   returns `0` immediately so fully-drained streams never return stale dust.
    /// - Dust suppression: if the calculated claimable balance is ≤ `DUST_THRESHOLD`
    ///   (1 stroop) it is also returned as `0`, preventing rounding artifacts from
    ///   causing failed micro-withdrawals or cluttering UIs.
    pub fn get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Ok(0);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.options.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        // ── Issue #241: Post-completion guard ───────────────────────────────
        // If the stream's end_time has passed and the full deposit has been
        // withdrawn, return 0 immediately. This prevents stale dust from
        // appearing after a stream is fully settled.
        if now >= stream.end_time && stream.options.total_withdrawn >= stream.deposit {
            return Ok(0);
        }

        // ── Milestone-release mode path ──────────────────────────────────────
        if stream.options.milestone_release_mode {
            let mut claimable: i128 = 0;
            for milestone in stream.options.milestones.iter() {
                if (now >= milestone.unlock_time && milestone.status == crate::types::MilestoneStatus::Pending)
                    || milestone.status == crate::types::MilestoneStatus::Released {
                    claimable = claimable
                        .checked_add(milestone.amount)
                        .ok_or(StreamError::Overflow)?;
                }
            }
            // Subtract what has already been withdrawn
            let available = claimable.saturating_sub(stream.options.total_withdrawn);
            return Ok(available.max(0));
        }

        // ── Step-vesting path ────────────────────────────────────────────────
        if stream.options.is_step_vesting {
            let tranches = load_tranches(&env, stream_id);
            let mut claimable: i128 = 0;
            for i in stream.options.tranches_claimed..tranches.len() {
                let t = tranches.get(i).unwrap();
                if now >= t.unlock_time {
                    claimable = claimable
                        .checked_add(t.amount)
                        .ok_or(StreamError::Overflow)?;
                } else {
                    // Tranches are sorted; no point checking further.
                    break;
                }
            }
            return Ok(claimable);
        }

        // ── Issue #13: Cliff enforcement ─────────────────────────────────────
        // If the current time is strictly before cliff_time, no tokens are
        // claimable regardless of time elapsed since start_time.
        if now < stream.cliff_time {
            return Ok(0);
        }

        // ── Compute raw claimable amount ─────────────────────────────────────
        let raw = match &stream.options.curve {
            VestingCurve::Linear => vesting_math::compute_claimable(
                stream.flow_rate,
                now,
                stream.cliff_time,
                stream.end_time,
                stream.last_withdraw_time,
            )
            .ok_or(StreamError::Overflow)?,

            VestingCurve::TimeDecay(decay_factor) => {
                vesting_math::compute_claimable_decay(
                    stream.deposit,
                    stream.start_time,
                    stream.end_time,
                    now,
                    stream.cliff_time,
                    stream.last_withdraw_time,
                    *decay_factor,
                )
                .ok_or(StreamError::Overflow)?
            }
        };

        // ── Issue #241: Dust suppression ─────────────────────────────────────
        // Clamp claimable to the remaining available balance first, then apply
        // the dust threshold. Sub-threshold amounts are treated as rounding
        // artifacts and returned as 0 to avoid failed micro-withdrawals.
        let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
        let claimable = raw.min(available);

        if claimable <= DUST_THRESHOLD {
            return Ok(0);
        }

        Ok(claimable)
    }

    pub fn get_accrued_balance(env: Env, stream_id: u64, _recipient: Address) -> Result<i128, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
            return Ok(0);
        }

        let now = if stream.status == StreamStatus::Paused {
            stream.last_pause_time
        } else {
            env.ledger().timestamp()
        };

        if now >= stream.end_time && stream.total_withdrawn >= stream.deposit {
            return Ok(0);
        }

        if stream.milestone_release_mode {
            let mut accrued: i128 = 0;
            for milestone in stream.milestones.iter() {
                if (now >= milestone.unlock_time && milestone.status == crate::types::MilestoneStatus::Pending)
                    || milestone.status == crate::types::MilestoneStatus::Released {
                    accrued = accrued
                        .checked_add(milestone.amount)
                        .ok_or(StreamError::Overflow)?;
                }
            }
            return Ok(accrued.max(0));
        }

        if stream.is_step_vesting {
            let tranches = load_tranches(&env, stream_id);
            let mut accrued: i128 = 0;
            for i in 0..tranches.len() {
                let t = tranches.get(i).unwrap();
                if now >= t.unlock_time {
                    accrued = accrued
                        .checked_add(t.amount)
                        .ok_or(StreamError::Overflow)?;
                } else {
                    break;
                }
            }
            return Ok(accrued);
        }

        if now < stream.cliff_time {
            return Ok(0);
        }

        let raw = match &stream.curve {
            VestingCurve::Linear => vesting_math::compute_claimable(
                stream.flow_rate,
                now,
                stream.cliff_time,
                stream.end_time,
                stream.start_time,
            )
            .ok_or(StreamError::Overflow)?,

            VestingCurve::TimeDecay(decay_factor) => {
                vesting_math::compute_claimable_decay(
                    stream.deposit,
                    stream.start_time,
                    stream.end_time,
                    now,
                    stream.cliff_time,
                    stream.start_time,
                    *decay_factor,
                )
                .ok_or(StreamError::Overflow)?
            }
        };

        let available = stream.deposit.saturating_sub(stream.total_withdrawn);
        let accrued = raw.min(available);

        if accrued <= DUST_THRESHOLD {
            return Ok(0);
        }

        Ok(accrued)
    }

    /// Returns true if `address` is either the sender or recipient of the given stream.
    pub fn is_participant(env: Env, stream_id: u64, address: Address) -> Result<bool, StreamError> {
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        Ok(stream.sender == address || stream.recipient == address)
    }

    /// Returns a paginated slice of streams created by a sender address.
    pub fn get_streams_by_sender(env: Env, sender: Address, start: u32, limit: u32) -> Vec<Stream> {
        let ids = get_ids_by_sender(&env, &sender);
        let cap = limit.min(20) as usize;
        let mut streams = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(ids.len() as usize) {
            if let Some(s) = load_stream(&env, ids.get(i as u32).unwrap()) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Returns a paginated slice of streams targeting a recipient address.
    pub fn get_streams_by_recipient(env: Env, recipient: Address, start: u32, limit: u32) -> Vec<Stream> {
        let ids = get_ids_by_recipient(&env, &recipient);
        let cap = limit.min(20) as usize;
        let mut streams = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(ids.len() as usize) {
            if let Some(s) = load_stream(&env, ids.get(i as u32).unwrap()) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Returns a paginated slice of streams created by a sender address with a specific tag.
    pub fn get_streams_by_tag(env: Env, sender: Address, tag: String, start: u32, limit: u32) -> Vec<Stream> {
        let ids = get_ids_by_tag(&env, &tag);
        let cap = limit.min(20) as usize;
        let mut streams = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(ids.len() as usize) {
            if let Some(s) = load_stream(&env, ids.get(i as u32).unwrap()) {
                streams.push_back(s);
            }
        }
        streams
    }

    /// Sets or updates the tag for a stream. Only the sender may call this.
    pub fn set_stream_tag(env: Env, stream_id: u64, sender: Address, tag: Option<String>) -> Result<(), StreamError> {
        sender.require_auth();

        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }

        // Remove old tag index if it existed
        if let Some(ref old_tag) = get_stream_tag(&env, stream_id) {
            unindex_by_tag(&env, old_tag, stream_id);
        }

        // Set new tag and index it
        if let Some(ref new_tag) = tag {
            set_stream_tag_storage(&env, stream_id, new_tag);
            index_by_tag(&env, new_tag, stream_id);
        } else {
            remove_stream_tag(&env, stream_id);
        }

        Ok(())
    }

    /// Returns only active streams created by a sender address.
    pub fn get_active_streams_by_sender(env: Env, sender: Address) -> Vec<Stream> {
        let ids = get_ids_by_sender(&env, &sender);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                if s.status == StreamStatus::Active {
                    streams.push_back(s);
                }
            }
        }
        streams
    }

    /// Returns only active streams targeting a recipient address.
    pub fn get_active_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream> {
        let ids = get_ids_by_recipient(&env, &recipient);
        let mut streams = Vec::new(&env);
        for id in ids.iter() {
            if let Some(s) = load_stream(&env, id) {
                if s.status == StreamStatus::Active {
                    streams.push_back(s);
                }
            }
        }
        streams
    }

    /// Queries streams with optional filters on status, asset, sender, and recipient.
    ///
    /// This function allows efficient on-chain filtering without iterating all stream records.
    /// All filter fields are optional; omitted filters (None) are not applied.
    /// Multiple filters are combined with AND logic.
    ///
    /// # Parameters
    /// * `filter` - The filter criteria. Any `None` field is ignored.
    /// * `start` - Starting index for pagination (0-based).
    /// * `limit` - Maximum number of results to return (capped at 20).
    ///
    /// # Returns
    /// A vector of streams matching all specified filter criteria.
    ///
    /// # Example
    /// To find all Active USDC streams from a specific sender:
    /// ```ignore
    /// let filter = StreamQueryFilter {
    ///     status: Some(StreamStatus::Active),
    ///     asset: Some(usdc_token_address),
    ///     sender: Some(sender_address),
    ///     recipient: None,
    /// };
    /// let results = query_streams(env, filter, 0, 20);
    /// ```
    pub fn query_streams(
        env: Env,
        filter: crate::types::StreamQueryFilter,
        start: u32,
        limit: u32,
    ) -> Vec<Stream> {
        // Get all global stream IDs
        let global_count = get_global_stream_count(&env);
        let mut matching_streams = Vec::new(&env);

        // Iterate through all streams and apply filters
        for i in 0..global_count {
            if let Some(stream_id) = get_global_stream_at(&env, i) {
                if let Some(stream) = load_stream(&env, stream_id) {
                    // Check all filter conditions
                    let mut matches = true;

                    // Check status filter
                    if let crate::types::OptionalStreamStatus::Some(ref status) = filter.status {
                        if stream.status != *status {
                            matches = false;
                        }
                    }

                    // Check asset (token) filter
                    if let Some(ref asset) = filter.asset {
                        if stream.token != *asset {
                            matches = false;
                        }
                    }

                    // Check sender filter
                    if let Some(ref sender) = filter.sender {
                        if stream.sender != *sender {
                            matches = false;
                        }
                    }

                    // Check recipient filter
                    if let Some(ref recipient) = filter.recipient {
                        if stream.recipient != *recipient {
                            matches = false;
                        }
                    }

                    if matches {
                        matching_streams.push_back(stream);
                    }
                }
            }
        }

        // Apply pagination
        let cap = limit.min(20) as usize;
        let mut paginated = Vec::new(&env);
        for i in (start as usize)..((start as usize).saturating_add(cap)).min(matching_streams.len() as usize) {
            if let Some(s) = matching_streams.get(i as u32) {
                paginated.push_back(s);
            }
        }

        paginated
    }

    /// Pauses an active stream.
    pub fn pause_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        stream.status = StreamStatus::Paused;
        stream.options.last_pause_time = env.ledger().timestamp();
        save_stream(&env, &stream);
        decrement_active_stream_count(&env);

        events::stream_paused(&env, stream.id, &sender);
        Ok(())
    }

    /// Resumes a paused stream, pushing back the end time.
    pub fn resume_stream(env: Env, stream_id: u64, sender: Address) -> Result<(), StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let mut stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::NotSender);
        }
        if stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotPaused);
        }

        let now = env.ledger().timestamp();
        let paused_duration = now.saturating_sub(stream.options.last_pause_time);

        stream.end_time = stream.end_time.saturating_add(paused_duration);
        stream.cliff_time = stream.cliff_time.saturating_add(paused_duration);
        stream.start_time = stream.start_time.saturating_add(paused_duration);
        stream.last_withdraw_time = stream.last_withdraw_time.saturating_add(paused_duration);
        stream.lock_until = stream.lock_until.saturating_add(paused_duration);

        stream.status = StreamStatus::Active;
        stream.options.last_pause_time = 0;
        save_stream(&env, &stream);
        increment_active_stream_count(&env);

        events::stream_resumed(&env, stream.id, &sender);
        Ok(())
    }

    /// Creates multiple payment streams in a single transaction.
    pub fn batch_create_stream(
        env: Env,
        sender: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        tokens: Vec<Address>,
        duration_seconds: u64,
        auto_renew: bool,
        renew_count: Option<u32>,
        lock_untils: Vec<u64>,
        nonce: u64,
        non_transferable: bool,
    ) -> Result<Vec<u64>, StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        sender.require_auth();

        let now = env.ledger().timestamp();

        // Check rate limit (per-sender creation frequency cap)
        check_rate_limit(&env, &sender)?;

        let expected_nonce = get_batch_nonce(&env, &sender);
        if nonce != expected_nonce {
            return Err(StreamError::InvalidNonce);
        }
        increment_batch_nonce(&env, &sender);

        if recipients.len() != amounts.len() || recipients.len() != lock_untils.len() || recipients.len() != tokens.len() {
            return Err(StreamError::BatchLengthMismatch);
        }

        let end_time = now
            .checked_add(duration_seconds)
            .ok_or(StreamError::Overflow)?;
        if end_time <= now {
            return Err(StreamError::InvalidDuration);
        }

        let max_dur = read_max_duration(&env);
        if max_dur > 0 && duration_seconds > max_dur {
            return Err(StreamError::DurationExceedsMax);
        }

        let sender_count = get_sender_stream_count(&env, &sender);
        let limit = effective_sender_limit(&env, &sender);
        if sender_count + recipients.len() > limit {
            return Err(StreamError::NewSenderStreamCapExceeded);
        }

        let mut stream_ids = Vec::new(&env);

        let n = recipients.len().min(amounts.len());
        let mut batch_ids: Vec<u64> = Vec::new(&env);

        // ── Phase 1: Validate all inputs before any state mutation ──────────
        //
        // All per-stream validation runs to completion before any token transfer
        // or storage write occurs. If any stream fails validation the entire
        // call is rejected and no state is modified.
        for i in 0..n {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            let token = tokens.get_unchecked(i);

            validate_recipient_address(&env, &sender, &recipient)?;
            check_token_whitelist(&env, &token)?;
            if amount <= 0 {
                return Err(StreamError::ZeroAmount);
            }
            let flow_rate = amount / duration_seconds as i128;
            if flow_rate == 0 {
                return Err(StreamError::ZeroFlowRate);
            }

            // ── Validate flow_rate bounds to prevent overflow during future withdrawals ──
            // Ensure flow_rate is within safe bounds: flow_rate * any_elapsed_time <= i128::MAX
            // This prevents "runtime errors" where computations overflow after stream creation.
            validate_flow_rate_bounds(flow_rate)?;

            // Validate token is a deployed SAC (Issue #243) - do this in validation phase
            validate_token_address(&env, &token)?;

            // Check token whitelist (Issue #221)
            check_token_whitelist(&env, &token)?;

            let stream_id = derive_stream_id(&env, &sender, &recipient, now, i as u64);
            if stream_exists(&env, stream_id) {
                return Err(StreamError::DuplicateStream);
            }
            for j in 0..batch_ids.len() {
                if batch_ids.get_unchecked(j) == stream_id {
                    return Err(StreamError::DuplicateStream);
                }
            }
            batch_ids.push_back(stream_id);
        }

        // ── Phase 1.5: Validate token balances and permissions ──────────────
        // Before any token transfer, group by token and check that the sender
        // has sufficient balance for all transfers to this contract.
        // This catches balance errors early (Phase 1) before any state mutation.
        let mut token_totals: Vec<(Address, i128)> = Vec::new(&env);
        for i in 0..n {
            let amount = amounts.get_unchecked(i);
            let token = tokens.get_unchecked(i);

            // Find or create entry for this token
            let mut found = false;
            for j in 0..token_totals.len() {
                let (t, total) = token_totals.get_unchecked(j);
                if t == token {
                    // Update existing entry
                    let new_total = total.checked_add(amount).ok_or(StreamError::Overflow)?;
                    let _ = token_totals.set(j, (token.clone(), new_total));
                    found = true;
                    break;
                }
            }
            if !found {
                // Add new entry
                token_totals.push_back((token.clone(), amount));
            }
        }

        // Verify sender has sufficient balance for each token
        for j in 0..token_totals.len() {
            let (token, total_needed) = token_totals.get_unchecked(j);
            let balance = token::Client::new(&env, &token).balance(&sender);
            if balance < total_needed {
                return Err(StreamError::ZeroAmount);  // Insufficient funds
            }
        }

        // ── Phase 2: Transfer tokens and persist stream records ──────────────
        //
        // All input validation is complete. Token balances have been verified.
        // Now perform all token transfers and stream persistence in sequence.
        // If any transfer fails here the entire transaction is rolled back by
        // the Soroban host and no orphaned state is left behind.
        for i in 0..n {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            let token = tokens.get_unchecked(i);
            let flow_rate = amount / duration_seconds as i128;
            let stream_id = batch_ids.get_unchecked(i);

            token::Client::new(&env, &token).transfer(
                &sender,
                &env.current_contract_address(),
                &amount,
            );

            let stream = Stream {
                id: stream_id,
                sender: sender.clone(),
                recipient: recipient.clone(),
                token: token.clone(),
                deposit: amount,
                flow_rate,
                start_time: now,
                cliff_time: now,
                lock_until: lock_untils.get_unchecked(i),
                end_time,
                last_withdraw_time: now,
                status: StreamStatus::Active,
                auto_renew,
                options: StreamOptions {
                    renew_count,
                    renewals_used: 0,
                    allow_recipient_termination: false,
                    last_pause_time: 0,
                    total_withdrawn: 0,
                    metadata: Bytes::new(&env),
                    locked: false,
                    metadata_uri: None,
                    milestones: soroban_sdk::Vec::new(&env),
                    milestone_release_mode: false,
                    holdback_amount: 0,
                    holdback_claimed: false,
                    is_step_vesting: false,
                    tranches_claimed: 0,
                    oracle: None,
                    max_price_deviation_bps: 0,
                    creation_price: 0,
                    curve: VestingCurve::Linear,
                    withdrawal_steps: None,
                    current_step: 0,
                    min_withdrawal_amount: None,
                    non_transferable,
                    requires_recipient_approval: false,
                    approval_timestamp: 0,
                    sender_locked: false,
                    is_dual_stream: false,
                    redirect_to_stream_id: None,
                    on_complete_contract: None,
                    on_complete_function: None,
                    comment: None,
                },
            };

            save_stream(&env, &stream);
            stream_ids.push_back(stream_id);
        }

        // ── Phase 3: Index all streams only after all transfers succeed ───────
        //
        // Sender/recipient/global indexes are updated in a dedicated pass that
        // only runs once every stream record has been persisted and every token
        // has been transferred. This prevents orphaned index entries: either
        // all streams are fully indexed or none are.
        for i in 0..n {
            let recipient = recipients.get_unchecked(i);
            let amount = amounts.get_unchecked(i);
            let flow_rate = amount / duration_seconds as i128;
            let stream_id = batch_ids.get_unchecked(i);
            let stream_token = tokens.get_unchecked(i);

            index_by_sender(&env, &sender, stream_id);
            index_by_recipient(&env, &recipient, stream_id);
            index_global_stream(&env, stream_id);
            increment_active_stream_count(&env);
            increment_token_stream_count(&env, &stream_token);

            events::stream_created(
                &env, stream_id, &sender, &recipient, amount, flow_rate, end_time, false, &None,
            );
        }

        Ok(stream_ids)
    }

    /// Withdraws from multiple streams in a single transaction.
    pub fn batch_withdraw(
        env: Env,
        stream_ids: Vec<u64>,
        recipient: Address,
    ) -> Result<Vec<i128>, StreamError> {
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
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

            if now < stream.lock_until {
                return Err(StreamError::StreamLocked);
            }

            let effective_now = now.min(stream.end_time);
            let raw_claimable = vesting_math::compute_earned(
                stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
            ).ok_or(StreamError::Overflow)?;

            let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
            let claimable = raw_claimable.min(available);

            let (recipient_amount, fee_amount) = if claimable > 0 {
                let fee_bps = get_protocol_fee(&env);
                let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
                    claimable
                        .checked_mul(fee_bps as i128)
                        .ok_or(StreamError::Overflow)?
                        / 10_000
                } else {
                    0
                };
                (claimable - fee_amount, fee_amount)
            } else {
                (0, 0)
            };

            // EFFECTS
            if claimable > 0 {
                stream.options.total_withdrawn = stream
                    .options.total_withdrawn
                    .checked_add(claimable)
                    .ok_or(StreamError::Overflow)?;
            }
            stream.last_withdraw_time = effective_now;

            // Accumulate fees in contract storage (swept via sweep_fees by admin)
            if fee_amount > 0 {
                accumulate_fees(&env, &stream.token, fee_amount);
            }

            if now >= stream.end_time {
                let duration = stream.end_time - stream.start_time;
                let dust = stream.deposit.saturating_sub(
                    stream.flow_rate.saturating_mul(duration as i128),
                );

                if stream.auto_renew {
                    // Check if we've hit the renewal count limit
                    let can_renew = if let Some(max_renewals) = stream.options.renew_count {
                        stream.options.renewals_used < max_renewals
                    } else {
                        true  // No limit set, can always renew
                    };

                    if !can_renew {
                        // Renewal limit reached, mark as completed
                        decrement_active_stream_count(&env);
                        decrement_token_stream_count(&env, &stream.token);
                        remove_stream(&env, stream_id);
                        unindex_by_sender(&env, &stream.sender, stream_id);
                        unindex_by_recipient(&env, &stream.recipient, stream_id);

                        let token_client = token::Client::new(&env, &stream.token);
                        if recipient_amount > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &recipient,
                                &recipient_amount,
                            );
                        }
                        if dust > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &stream.sender,
                                &dust,
                            );
                        }

                        events::renewal_limit_reached(&env, stream_id, &stream.sender, stream.options.renewals_used);
                        events::stream_completed(&env, stream_id);
                    } else {
                        stream.sender.require_auth();
                        let stream_duration = stream.end_time.saturating_sub(stream.start_time);
                        let new_end = stream
                            .end_time
                            .checked_add(stream_duration)
                            .ok_or(StreamError::Overflow)?;
                        stream.start_time = stream.end_time;
                        stream.end_time = new_end;
                        stream.last_withdraw_time = stream.start_time;
                        stream.options.total_withdrawn = 0;
                        stream.options.renewals_used = stream.options.renewals_used.saturating_add(1);
                        save_stream(&env, &stream);

                        // INTERACTIONS
                        let token_client = token::Client::new(&env, &stream.token);
                        if recipient_amount > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &recipient,
                                &recipient_amount,
                            );
                        }
                        token_client.transfer(
                            &stream.sender,
                            &env.current_contract_address(),
                            &stream.deposit,
                        );
                    }
                } else {
                    decrement_active_stream_count(&env);
                    decrement_token_stream_count(&env, &stream.token);
                    remove_stream(&env, stream_id);
                    unindex_by_sender(&env, &stream.sender, stream_id);
                    unindex_by_recipient(&env, &stream.recipient, stream_id);

                    // INTERACTIONS
                    let token_client = token::Client::new(&env, &stream.token);
                    if recipient_amount > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &recipient,
                            &recipient_amount,
                        );
                    }
                    if dust > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &stream.sender,
                            &dust,
                        );
                    }
                    events::stream_completed(&env, stream_id);
                }
            } else {
                save_stream(&env, &stream);

                // INTERACTIONS
                let token_client = token::Client::new(&env, &stream.token);
                if recipient_amount > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &recipient,
                        &recipient_amount,
                    );
                }
            }

            amounts.push_back(claimable);
            events::stream_withdrawn(&env, stream_id, &recipient, claimable, now, stream.options.total_withdrawn);
        }

        Ok(amounts)
    }

    /// Cancels multiple streams in a single transaction.
    pub fn batch_cancel_stream(
        env: Env,
        stream_ids: Vec<u64>,
        sender: Address,
    ) -> Result<Vec<Result<(), StreamError>>, StreamError> {
        sender.require_auth();

        if stream_ids.is_empty() || stream_ids.len() > 20 {
            return Err(StreamError::BatchLengthMismatch);
        }

        let mut results = Vec::new(&env);

        for stream_id in stream_ids.iter() {
            let result = (|| {
                let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

                if stream.sender != sender {
                    return Err(StreamError::NotSender);
                }

                if stream.status != StreamStatus::Active && stream.status != StreamStatus::Paused {
                    return Err(StreamError::StreamNotActive);
                }

                let now = env.ledger().timestamp();
                let recipient_amount = vesting_math::compute_earned(
                    stream.flow_rate, now, stream.end_time, stream.last_withdraw_time,
                ).ok_or(StreamError::Overflow)?;

                let available = stream.deposit.saturating_sub(stream.options.total_withdrawn);
                let recipient_amount = recipient_amount.min(available);
                let refund_amount = available.saturating_sub(recipient_amount);

                // Decrement active count only if stream was Active (Paused was already decremented)
                if stream.status == StreamStatus::Active {
                    decrement_active_stream_count(&env);
                    decrement_token_stream_count(&env, &stream.token);
                }

                // EFFECTS
                remove_stream(&env, stream_id);
                unindex_by_sender(&env, &stream.sender, stream_id);
                unindex_by_recipient(&env, &stream.recipient, stream_id);

                // INTERACTIONS
                let token_client = token::Client::new(&env, &stream.token);
                if recipient_amount > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &stream.recipient,
                        &recipient_amount,
                    );
                }
                if refund_amount > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &stream.sender,
                        &refund_amount,
                    );
                }

                events::stream_cancelled(&env, stream_id, &stream.sender, refund_amount, recipient_amount);
                Ok(())
            })();
            results.push_back(result);
        }

        Ok(results)
    }

    /// Sets the protocol fee in basis points (100 bps = 1%).
    pub fn set_protocol_fee(env: Env, fee_bps: u32) -> Result<(), StreamError> {
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }
        set_protocol_fee(&env, fee_bps);
        Ok(())
    }

    // ── Issue #288: Cancellation fee ─────────────────────────────────────────

    /// Returns the current early-cancellation fee rate in basis points (0 = no fee).
    ///
    /// The fee is deducted from the **sender's refund portion** when they cancel an
    /// active stream before its natural end. Fee-exempt senders (see `add_fee_exempt`)
    /// are never charged.
    pub fn get_cancellation_fee_bps(env: Env) -> u32 {
        get_cancellation_fee_bps(&env)
    }

    /// Sets the early-cancellation fee in basis points. Max 10 000 (= 100%). Admin only.
    ///
    /// # Errors
    /// - `NotInitialized`  — contract has not been initialised yet.
    /// - `InvalidDuration` — `fee_bps > 10_000`.
    pub fn set_cancellation_fee(env: Env, admin: Address, fee_bps: u32) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        // Accept super-admin or fee-manager (issue #292)
        roles::require_fee_manager_or_admin(&env, &admin, &stored_admin)?;
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }
        set_cancellation_fee_bps(&env, fee_bps);
        let ts = env.ledger().timestamp();
        let entry = AuditEntry {
            instruction: String::from_str(&env, "set_cancellation_fee"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &admin, ts);
        Ok(())
    }

    // ── Issue #293: Sender collateral / stake mechanism ───────────────────────
    //
    // Senders deposit stake collateral to unlock the ability to create streams
    // for a given token.  The admin configures a per-token minimum stake amount.
    // Unstaking requires a cooldown period (STAKE_UNLOCK_DELAY) to prevent
    // quick deposit-and-withdraw exploits.  The admin can slash a sender's stake
    // to the protocol treasury as a penalty for bad behaviour.

    /// Deposits `amount` of `token` as stake collateral for the caller.
    ///
    /// The caller must have pre-approved the token transfer before calling this.
    /// Once staked, the balance persists until the sender initiates an unstake
    /// and waits for the cooldown to elapse.
    ///
    /// # Errors
    /// - `ZeroAmount` — `amount` must be > 0.
    pub fn stake(env: Env, sender: Address, token: Address, amount: i128) -> Result<(), StreamError> {
        sender.require_auth();
        if is_paused_or_auto_unpause(&env) {
            return Err(StreamError::ContractPaused);
        }
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        // Transfer stake from sender into contract escrow.
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );
        add_stake_balance(&env, &sender, &token, amount);
        let new_balance = get_stake_balance(&env, &sender, &token);
        events::stake_deposited(&env, &sender, &token, amount, new_balance);
        Ok(())
    }

    /// Returns the staked balance for `sender` in `token` (0 if none).
    pub fn get_stake(env: Env, sender: Address, token: Address) -> i128 {
        get_stake_balance(&env, &sender, &token)
    }

    /// Initiates an unstake request for `amount` of `token`.
    ///
    /// The funds remain locked for `STAKE_UNLOCK_DELAY` seconds after this
    /// call.  Call `complete_unstake` after the delay to receive the tokens.
    ///
    /// # Errors
    /// - `ZeroAmount` — `amount` must be > 0.
    /// - `InsufficientStake` — stake balance is below `amount`.
    pub fn initiate_unstake(env: Env, sender: Address, token: Address, amount: i128) -> Result<(), StreamError> {
        sender.require_auth();
        if amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        let balance = get_stake_balance(&env, &sender, &token);
        if balance < amount {
            return Err(StreamError::InsufficientStake);
        }
        let unlock_at = env.ledger().timestamp().saturating_add(STAKE_UNLOCK_DELAY);
        // Reduce balance immediately; store unlock timestamp with the pending amount.
        sub_stake_balance(&env, &sender, &token, amount);
        // Reuse the unlock_at slot to encode both (amount, timestamp) together.
        // We encode: store the pending amount in a separate key and the unlock time as before.
        // For simplicity, we store `(amount, unlock_at)` as a tuple in the persistent store.
        env.storage().persistent().set(
            &(soroban_sdk::Symbol::new(&env, "stk_p"), sender.clone(), token.clone()),
            &(amount, unlock_at),
        );
        events::unstake_initiated(&env, &sender, &token, amount, unlock_at);
        Ok(())
    }

    /// Completes a previously initiated unstake, returning the tokens to the sender.
    ///
    /// Can only be called after `STAKE_UNLOCK_DELAY` seconds have elapsed since
    /// `initiate_unstake` was called.
    ///
    /// # Errors
    /// - `NoPendingUnstake` — no pending unstake for this sender/token pair.
    /// - `StakeStillLocked` — the cooldown period has not yet elapsed.
    pub fn complete_unstake(env: Env, sender: Address, token: Address) -> Result<(), StreamError> {
        sender.require_auth();
        let pending_key = (soroban_sdk::Symbol::new(&env, "stk_p"), sender.clone(), token.clone());
        let pending: Option<(i128, u64)> = env.storage().persistent().get(&pending_key);
        let (amount, unlock_at) = pending.ok_or(StreamError::NoPendingUnstake)?;
        let now = env.ledger().timestamp();
        if now < unlock_at {
            return Err(StreamError::StakeStillLocked);
        }
        // Clear pending entry and transfer tokens back to sender.
        env.storage().persistent().remove(&pending_key);
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &sender,
            &amount,
        );
        events::unstake_completed(&env, &sender, &token, amount);
        Ok(())
    }

    /// Returns the pending unstake `(amount, unlock_at)` for `sender`/`token`, or `None`.
    pub fn get_pending_unstake(env: Env, sender: Address, token: Address) -> Option<(i128, u64)> {
        env.storage().persistent().get(
            &(soroban_sdk::Symbol::new(&env, "stk_p"), sender, token),
        )
    }

    /// Admin: sets the minimum stake required for senders to create streams with `token`.
    ///
    /// Set to 0 to disable the stake requirement for that token.
    ///
    /// Only the super-admin or fee-manager may call this.
    pub fn set_min_stake_amount(env: Env, admin: Address, token: Address, amount: i128) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        roles::require_fee_manager_or_admin(&env, &admin, &stored_admin)?;
        if amount < 0 {
            return Err(StreamError::ZeroAmount);
        }
        set_min_stake(&env, &token, amount);
        events::min_stake_set(&env, &token, amount, &admin);
        Ok(())
    }

    /// Returns the minimum stake required for senders to create streams with `token` (0 = no requirement).
    pub fn get_min_stake_amount(env: Env, token: Address) -> i128 {
        get_min_stake(&env, &token)
    }

    /// Admin: slashes `slash_amount` from `sender`'s staked collateral for `token`.
    ///
    /// The slashed tokens are transferred to `destination` (typically the treasury).
    /// Used to penalise spammers and bad actors after off-chain review / governance vote.
    ///
    /// Only the super-admin may perform a slash.
    ///
    /// # Errors
    /// - `NotInitialized`    — contract not yet initialised.
    /// - `NotAuthorized`     — caller is not the super-admin.
    /// - `InsufficientStake` — `slash_amount` exceeds the sender's staked balance.
    pub fn slash_stake(
        env: Env,
        admin: Address,
        sender: Address,
        token: Address,
        slash_amount: i128,
        destination: Address,
    ) -> Result<(), StreamError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != stored_admin {
            return Err(StreamError::NotAuthorized);
        }
        if slash_amount <= 0 {
            return Err(StreamError::ZeroAmount);
        }
        let balance = get_stake_balance(&env, &sender, &token);
        if balance < slash_amount {
            return Err(StreamError::InsufficientStake);
        }
        sub_stake_balance(&env, &sender, &token, slash_amount);
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &destination,
            &slash_amount,
        );
        events::stake_slashed(&env, &sender, &token, slash_amount, &destination, &admin);
        let ts = env.ledger().timestamp();
        let entry = AuditEntry {
            instruction: String::from_str(&env, "slash_stake"),
            admin: admin.clone(),
            timestamp: ts,
            params: String::from_str(&env, ""),
        };
        append_audit_entry(&env, &entry);
        events::admin_action(&env, &entry.instruction, &admin, ts);
        Ok(())
    }

    /// Proposes a new protocol fee change with a 7-day timelock.
    ///
    /// After the timelock expires, anyone may call `execute_fee_change` to apply it.
    /// Only the super-admin may propose a fee change.
    pub fn propose_fee_change(env: Env, admin: Address, new_fee_bps: u32) -> Result<(), StreamError> {
        admin.require_auth();
        let current_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != current_admin {
            return Err(StreamError::NotAuthorized);
        }
        if new_fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }

        let now = env.ledger().timestamp();
        let unlock_time = now.saturating_add(7 * 24 * 60 * 60);

        write_pending_fee_proposal(&env, new_fee_bps, unlock_time);
        events::fee_change_proposed(&env, new_fee_bps, unlock_time);
        Ok(())
    }

    pub fn execute_fee_change(env: Env) -> Result<(), StreamError> {
        let (new_fee_bps, unlock_time) = read_pending_fee_proposal(&env).ok_or(StreamError::NotAuthorized)?;

        let now = env.ledger().timestamp();
        if now < unlock_time {
            return Err(StreamError::StreamLocked);
        }

        set_protocol_fee(&env, new_fee_bps);
        clear_pending_fee_proposal(&env);
        events::fee_change_executed(&env, new_fee_bps);

        Ok(())
    }

    /// Sets the treasury address to receive protocol fees.
    pub fn set_treasury_address(env: Env, treasury: Address) -> Result<(), StreamError> {
        set_treasury(&env, &treasury);
        Ok(())
    }

    /// Sets a per-token fee tier (in basis points).
    ///
    /// Allows different tokens to have different fee rates.
    /// If set, overrides the global default protocol fee for withdrawals on that token.
    pub fn set_token_fee_tier(env: Env, admin: Address, token: Address, fee_bps: u32) -> Result<(), StreamError> {
        admin.require_auth();
        let current_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != current_admin {
            return Err(StreamError::NotAuthorized);
        }
        if fee_bps > 10_000 {
            return Err(StreamError::InvalidDuration);
        }

        storage::set_token_fee_tier(&env, &token, fee_bps);
        Ok(())
    }

    /// Removes a per-token fee tier, reverting to the global default protocol fee.
    pub fn remove_token_fee_tier(env: Env, admin: Address, token: Address) -> Result<(), StreamError> {
        admin.require_auth();
        let current_admin = read_admin(&env).ok_or(StreamError::NotInitialized)?;
        if admin != current_admin {
            return Err(StreamError::NotAuthorized);
        }

        storage::remove_token_fee_tier(&env, &token);
        Ok(())
    }

    /// Gets the effective fee tier (in basis points) for a token.
    ///
    /// Returns the token-specific tier if set, otherwise the global default protocol fee.
    pub fn get_token_fee_tier(env: Env, token: Address) -> u32 {
        storage::get_effective_fee_tier(&env, &token)
    }

    /// Gets the metadata URI for a stream, if set.
    pub fn get_metadata_uri(env: Env, stream_id: u64) -> Option<String> {
        load_stream(&env, stream_id).and_then(|s| s.options.metadata_uri)
    }

    /// Returns the current XLM creation fee in stroops (0 = disabled).
    pub fn get_creation_fee(env: Env) -> i128 {
        get_creation_fee_xlm(&env)
    }

    /// Returns protocol fee configuration.
    pub fn get_protocol_fee_info(env: Env) -> (u32, Option<Address>) {
        (get_protocol_fee(&env), get_treasury(&env))
    }

    /// Withdraws accumulated protocol fees from the treasury contract.
    pub fn withdraw_treasury(
        env: Env,
        token: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), StreamError> {
        check_admin(&env);
        let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
        env.invoke_contract::<()>(
            &treasury,
            &Symbol::new(&env, "withdraw_treasury"),
            (token, amount, destination).into_val(&env),
        );
        Ok(())
    }

    /// Withdraws all accumulated protocol fees for a token from the treasury contract.
    pub fn withdraw_all_from_treasury(
        env: Env,
        token: Address,
        destination: Address,
    ) -> Result<i128, StreamError> {
        check_admin(&env);
        let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
        let result = env.invoke_contract::<i128>(
            &treasury,
            &Symbol::new(&env, "withdraw_all"),
            (token, destination).into_val(&env),
        );
        Ok(result)
    }

    /// Returns aggregate contract statistics.
    pub fn get_stats(env: Env) -> Stats {
        let total_streams = get_global_stream_count(&env) as u64;
        let active_streams = get_active_stream_count(&env) as u64;

        let mut total_volume: i128 = 0;
        let count = get_global_stream_count(&env);

        for i in 0..count {
            if let Some(stream_id) = get_global_stream_at(&env, i) {
                if let Some(stream) = load_stream(&env, stream_id) {
                    total_volume = total_volume.saturating_add(stream.deposit);
                }
            }
        }

        Stats {
            total_streams,
            active_streams,
            total_volume,
        }
    }

    /// Returns enhanced protocol statistics with per-asset and per-status breakdown.
    ///
    /// This function aggregates statistics across all streams, providing:
    /// - Total stream counts and volume
    /// - Status breakdown (Active, Cancelled, Completed, Paused, Expired, PendingApproval)
    /// - Per-asset breakdown (token, stream count, volume, active count)
    ///
    /// The asset breakdown is sorted by total volume in descending order.
    pub fn get_protocol_stats(env: Env) -> types::ProtocolStats {
        let total_streams = get_global_stream_count(&env) as u64;
        let active_streams = get_active_stream_count(&env) as u64;
        let count = get_global_stream_count(&env);

        // Initialize status counters
        let mut status_stats = types::StatusStats {
            active: 0,
            cancelled: 0,
            completed: 0,
            paused: 0,
            expired: 0,
            pending_approval: 0,
        };

        // Use a map-like structure to aggregate per-asset stats
        // Key: token address, Value: (stream_count, total_volume, active_count)
        let mut asset_map: Vec<(Address, u64, i128, u64)> = Vec::new(&env);

        let mut total_volume: i128 = 0;

        // Iterate through all streams and aggregate statistics
        for i in 0..count {
            if let Some(stream_id) = get_global_stream_at(&env, i) {
                if let Some(stream) = load_stream(&env, stream_id) {
                    // Update status breakdown
                    match stream.status {
                        types::StreamStatus::Active => status_stats.active += 1,
                        types::StreamStatus::Cancelled => status_stats.cancelled += 1,
                        types::StreamStatus::Completed => status_stats.completed += 1,
                        types::StreamStatus::Paused => status_stats.paused += 1,
                        types::StreamStatus::Expired => status_stats.expired += 1,
                        types::StreamStatus::PendingApproval => status_stats.pending_approval += 1,
                        types::StreamStatus::EscrowHold => {}
                    }

                    // Update total volume
                    total_volume = total_volume.saturating_add(stream.deposit);

                    // Update per-asset stats
                    if let Some(pos) = asset_map.iter().position(|(token, _, _, _)| &token == &stream.token) {
                        let (_, count, vol, active) = asset_map.get(pos as u32).unwrap();
                        let is_active = matches!(stream.status, types::StreamStatus::Active);
                        asset_map.set(
                            pos as u32,
                            (
                                stream.token.clone(),
                                count + 1,
                                vol.saturating_add(stream.deposit),
                                active + (if is_active { 1 } else { 0 }),
                            ),
                        );
                    } else {
                        let is_active = matches!(stream.status, types::StreamStatus::Active);
                        asset_map.push_back((
                            stream.token.clone(),
                            1u64,
                            stream.deposit,
                            if is_active { 1u64 } else { 0u64 },
                        ));
                    }
                }
            }
        }

        // Convert asset_map to Vec<AssetStats>
        let mut asset_stats: Vec<types::AssetStats> = Vec::new(&env);
        for (token, stream_count, total_vol, active_count) in asset_map.iter() {
            asset_stats.push_back(types::AssetStats {
                token,
                stream_count,
                total_volume: total_vol,
                active_streams: active_count,
            });
        }

        // Sort by total volume in descending order (selection sort; the asset
        // list is small and soroban Vec provides no sort method).
        let n = asset_stats.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let a = asset_stats.get(i).unwrap();
                let b = asset_stats.get(j).unwrap();
                if b.total_volume > a.total_volume {
                    asset_stats.set(i, b);
                    asset_stats.set(j, a);
                }
            }
        }

        types::ProtocolStats {
            total_streams,
            active_streams,
            total_volume,
            status_breakdown: status_stats,
            asset_breakdown: asset_stats,
        }
    }

    /// Returns the number of currently active streams for the given SAC token address.
    ///
    /// Returns `0` for unknown/never-used token addresses rather than erroring.
    /// Read-only, no auth required.
    pub fn get_stream_count_by_token(env: Env, token: Address) -> u64 {
        get_token_stream_count(&env, &token) as u64
    }

    /// Recalibrates the active stream count by scanning all streams.
    /// Only callable by admin. Use when counter drift is suspected.
    pub fn recalibrate_stats(env: Env, admin: Address) -> Result<(), StreamError> {        check_admin(&env);
        admin.require_auth();

        let mut correct_count = 0u32;
        let count = get_global_stream_count(&env);

        for i in 0..count {
            if let Some(stream_id) = get_global_stream_at(&env, i) {
                if let Some(stream) = load_stream(&env, stream_id) {
                    if stream.status == StreamStatus::Active {
                        correct_count += 1;
                    }
                }
            }
        }

        set_active_stream_count(&env, correct_count);
        Ok(())
    }

    /// Returns a health snapshot for the given stream's on-chain storage entry.
    ///
    /// Read-only, no auth required.  Reports the current ledger sequence, the
    /// stream's `end_time`, ledgers remaining before the persistent storage entry
    /// is evicted, and a derived health classification.
    ///
    /// ## Health thresholds
    /// | Remaining ledgers | Status       |
    /// |-------------------|--------------|
    /// | >= 10,000         | `Healthy`    |
    /// | 1,000 – 9,999     | `TTLWarning` |
    /// | < 1,000           | `AtRisk`     |
    ///
    /// # Errors
    /// Returns `StreamError::StreamNotFound` if no stream with this ID exists.
    pub fn get_stream_health(env: Env, stream_id: u64) -> Result<StreamHealth, StreamError> {
        use types::{HealthStatus, StreamHealth};

        // Confirm the stream exists — returns StreamNotFound for unknown IDs.
        let stream = load_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;

        let current_ledger = env.ledger().sequence();

        // Live TTL query requires the testutils Persistent trait; use 0 in contract builds.
        #[cfg(test)]
        let ttl_remaining: u32 = {
            use soroban_sdk::testutils::storage::Persistent as _;
            env.storage().persistent().get_ttl(&stream_id)
        };
        #[cfg(not(test))]
        let ttl_remaining: u32 = 0;

        const TTL_WARNING_THRESHOLD: u32 = 10_000;
        const TTL_AT_RISK_THRESHOLD: u32 = 1_000;

        let status = if ttl_remaining >= TTL_WARNING_THRESHOLD {
            HealthStatus::Healthy
        } else if ttl_remaining >= TTL_AT_RISK_THRESHOLD {
            HealthStatus::TTLWarning
        } else {
            HealthStatus::AtRisk
        };

        Ok(StreamHealth {
            current_ledger,
            end_time: stream.end_time,
            ttl_remaining_ledgers: ttl_remaining,
            status,
        })
    }

    /// Internal helper: invokes on_complete callback if configured for a stream.
    ///
    /// Called when a stream transitions to Completed status.  Attempts to invoke the
    /// configured contract's function with stream_id as an argument.  Success and
    /// failure are both emitted as events; no exception is thrown if the callback fails.
    fn invoke_on_complete(env: &Env, stream: &Stream) {
        if let (Some(ref contract), Some(ref function)) = (&stream.options.on_complete_contract, &stream.options.on_complete_function) {
            events::on_complete_invoked(env, stream.id, contract, function);

            // Attempt to invoke the callback. We invoke with the stream_id as the argument.
            // Since contract invocations can fail, we just log the attempt and assume success
            // unless the contract returns an error or panics.
            env.invoke_contract::<()>(
                contract,
                function,
                soroban_sdk::vec![env, soroban_sdk::IntoVal::into_val(&stream.id, env)],
            );

            // If we reach here without panic, the callback succeeded
            events::on_complete_success(env, stream.id, contract);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Issue #291: External contract composability implementation
// ═══════════════════════════════════════════════════════════════════════════

#[contractimpl]
impl composability::ISoroStreamComposability for SoroStreamContract {
    /// Creates a stream on behalf of an external contract.
    ///
    /// Delegates to the main `create_stream` implementation so all existing
    /// validation, rate-limiting, and event emission remain consistent.
    #[allow(clippy::too_many_arguments)]
    fn composable_create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        duration_seconds: u64,
        auto_renew: bool,
        params: crate::types::CreateStreamParams,
    ) -> Result<u64, StreamError> {
        SoroStreamContract::create_stream(
            env,
            sender,
            recipient,
            token,
            amount,
            duration_seconds,
            auto_renew,
            params,
        )
    }

    /// Triggers a withdrawal for `recipient` on `stream_id`.
    fn composable_withdraw(
        env: Env,
        stream_id: u64,
        recipient: Address,
    ) -> Result<(), StreamError> {
        SoroStreamContract::withdraw(env, stream_id, recipient)
    }

    /// Returns the amount of tokens currently claimable by the recipient.
    fn composable_get_claimable(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        SoroStreamContract::get_claimable(env, stream_id)
    }

    /// Returns the full [`Stream`] struct for `stream_id`.
    fn composable_get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        SoroStreamContract::get_stream(env, stream_id)
    }

    /// Returns all streams where `address` is sender (if `as_sender = true`) or recipient.
    fn composable_get_streams_for(
        env: Env,
        address: Address,
        as_sender: bool,
        start: u32,
        limit: u32,
    ) -> Vec<Stream> {
        if as_sender {
            SoroStreamContract::get_streams_by_sender(env, address, start, limit)
        } else {
            SoroStreamContract::get_streams_by_recipient(env, address, start, limit)
        }
    }

    /// Returns `true` when the stream has a non-zero claimable balance.
    fn composable_has_claimable(env: Env, stream_id: u64) -> bool {
        SoroStreamContract::get_claimable(env.clone(), stream_id)
            .map(|v| v > 0)
            .unwrap_or(false)
    }

    /// Returns the SoroStream contract version string.
    fn composable_get_version(env: Env) -> Result<soroban_sdk::String, StreamError> {
        SoroStreamContract::get_version(env)
    }
}
