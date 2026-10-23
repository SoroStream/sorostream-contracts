//! # Role-Based Admin Access Control (issue #292)
//!
//! Replaces the monolithic single-admin model with three fine-grained roles,
//! each scoped to the minimum privilege required for its operation:
//!
//! | Role             | Storage key | Permitted operations                                      |
//! |------------------|-------------|-----------------------------------------------------------|
//! | `SuperAdmin`     | `radm`      | All operations, including role assignment                 |
//! | `FeeManager`     | `rfee`      | `set_protocol_fee`, `set_cancellation_fee`, `sweep_fees` |
//! | `EmergencyPause` | `rpause`    | `emergency_pause`, `emergency_resume`                    |
//! | `Analytics`      | `ranal`     | Read-only: `get_stats`, `get_protocol_stats`, audit log   |
//!
//! The existing `admin` key (`ADMIN_KEY`) continues to serve as the
//! `SuperAdmin` — no storage migration is required.  New roles are stored
//! under their own instance-storage keys and default to `None` (unset).
//!
//! ## Security Model
//!
//! * Only the `SuperAdmin` can assign or revoke roles.
//! * Role slots hold a single `Address`.  Multi-holder scenarios should use a
//!   multisig contract as the role address.
//! * Compromise of a restricted role exposes only the operations listed above —
//!   other sensitive operations (upgrade, migration, blocklist management, etc.)
//!   remain gated behind `SuperAdmin`.
//!
//! ## Audit interpretation
//!
//! Role getters report current state only. `None` is indistinguishable between
//! a role that was never assigned and one that was later revoked. The main
//! stream contract records assignment/revocation in its bounded admin audit log
//! and emits `RoleAssigned` / `RoleRevoked` events through `role_assigned` and
//! `role_revoked`. Those records can establish that history while available;
//! the getter alone cannot.

#![allow(dead_code)]
use soroban_sdk::{contracttype, Address, Env, Symbol};

// ── Storage keys for each role ────────────────────────────────────────────────

const FEE_MANAGER_KEY: &str = "rfee";
const EMERGENCY_PAUSE_KEY: &str = "rpause";
const ANALYTICS_KEY: &str = "ranal";
const TOKEN_ADMIN_KEY: &str = "rtok";

// ── Role enum (for event payloads) ────────────────────────────────────────────

/// Discriminant for role assignment / revocation events.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdminRole {
    /// Full admin — can assign roles and perform all operations.
    SuperAdmin,
    /// May adjust fee parameters and sweep accumulated fees.
    FeeManager,
    /// May pause and resume the contract in emergencies.
    EmergencyPause,
    /// Manages allowed tokens.
    TokenAdmin,
    /// Read-only analytics access (stats, audit log).
    Analytics,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

/// Returns the currently assigned `FeeManager` address, or `None`.
pub fn get_fee_manager(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, FEE_MANAGER_KEY))
}

/// Sets the `FeeManager` role to `addr`.
pub fn set_fee_manager(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, FEE_MANAGER_KEY), addr);
}

/// Revokes the `FeeManager` role.
pub fn revoke_fee_manager(env: &Env) {
    env.storage()
        .instance()
        .remove(&Symbol::new(env, FEE_MANAGER_KEY));
}

/// Atomically transfers the `FeeManager` role from `old` to `new`.
pub fn transfer_fee_manager_role(
    env: &Env,
    old: &Address,
    new: &Address,
) -> Result<(), crate::errors::StreamError> {
    if get_fee_manager(env).as_ref() != Some(old) {
        return Err(crate::errors::StreamError::NotAuthorized);
    }
    set_fee_manager(env, new);
    Ok(())
}

/// Returns the currently assigned `EmergencyPause` role address, or `None`.
pub fn get_emergency_pause_role(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, EMERGENCY_PAUSE_KEY))
}

/// Sets the `EmergencyPause` role to `addr`.
pub fn set_emergency_pause_role(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, EMERGENCY_PAUSE_KEY), addr);
}

/// Revokes the `EmergencyPause` role.
pub fn revoke_emergency_pause_role(env: &Env) {
    env.storage()
        .instance()
        .remove(&Symbol::new(env, EMERGENCY_PAUSE_KEY));
}

/// Atomically transfers the `EmergencyPause` role from `old` to `new`.
pub fn transfer_emergency_pause_role(
    env: &Env,
    old: &Address,
    new: &Address,
) -> Result<(), crate::errors::StreamError> {
    if get_emergency_pause_role(env).as_ref() != Some(old) {
        return Err(crate::errors::StreamError::NotAuthorized);
    }
    set_emergency_pause_role(env, new);
    Ok(())
}

/// Returns the currently assigned `Analytics` role address, or `None`.
pub fn get_analytics_role(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, ANALYTICS_KEY))
}

/// Sets the `Analytics` role to `addr`.
pub fn set_analytics_role(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, ANALYTICS_KEY), addr);
}

/// Revokes the `Analytics` role.
pub fn revoke_analytics_role(env: &Env) {
    env.storage()
        .instance()
        .remove(&Symbol::new(env, ANALYTICS_KEY));
}

/// Atomically transfers the `Analytics` role from `old` to `new`.
pub fn transfer_analytics_role(
    env: &Env,
    old: &Address,
    new: &Address,
) -> Result<(), crate::errors::StreamError> {
    if get_analytics_role(env).as_ref() != Some(old) {
        return Err(crate::errors::StreamError::NotAuthorized);
    }
    set_analytics_role(env, new);
    Ok(())
}

/// Returns the currently assigned `TokenAdmin` role address, or `None`.
pub fn get_token_admin(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, TOKEN_ADMIN_KEY))
}

/// Sets the `TokenAdmin` role to `addr`.
pub fn set_token_admin(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, TOKEN_ADMIN_KEY), addr);
}

/// Revokes the `TokenAdmin` role.
pub fn revoke_token_admin(env: &Env) {
    env.storage()
        .instance()
        .remove(&Symbol::new(env, TOKEN_ADMIN_KEY));
}

// ── Aggregated role view (Issue #753) ─────────────────────────────────────────

/// All currently-assigned admin roles in a single view, so an admin dashboard
/// can assemble the full picture of who holds what with one call instead of
/// querying each role getter (plus the super-admin getter) separately.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleHolders {
    /// Super-admin (stored under the contract's `admin` key).
    pub super_admin: Option<Address>,
    /// Fee manager role holder.
    pub fee_manager: Option<Address>,
    /// Emergency pause role holder.
    pub emergency_pause: Option<Address>,
    /// Analytics role holder.
    pub analytics: Option<Address>,
    /// Token admin role holder.
    pub token_admin: Option<Address>,
}

/// Returns all currently-assigned admin roles in a single call.
pub fn get_role_holders(env: &Env) -> RoleHolders {
    RoleHolders {
        super_admin: env
            .storage()
            .instance()
            .get(&Symbol::new(env, "admin")),
        fee_manager: get_fee_manager(env),
        emergency_pause: get_emergency_pause_role(env),
        analytics: get_analytics_role(env),
        token_admin: get_token_admin(env),
    }
}

// ── Role Aliases (Issue #623) ─────────────────────────────────────────────────

pub fn get_fee_admin(env: &Env) -> Option<Address> {
    get_fee_manager(env)
}

pub fn set_fee_admin(env: &Env, addr: &Address) {
    set_fee_manager(env, addr);
}

pub fn revoke_fee_admin(env: &Env) {
    revoke_fee_manager(env);
}

pub fn get_emergency_admin(env: &Env) -> Option<Address> {
    get_emergency_pause_role(env)
}

pub fn set_emergency_admin(env: &Env, addr: &Address) {
    set_emergency_pause_role(env, addr);
}

pub fn revoke_emergency_admin(env: &Env) {
    revoke_emergency_pause_role(env);
}

// ── Auth helpers ──────────────────────────────────────────────────────────────

/// Checks that `caller` holds the `FeeManager` / `FeeAdmin` role **or** is the super-admin.
pub fn require_fee_manager_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    if caller == admin {
        return Ok(());
    }
    if let Some(ref fm) = get_fee_manager(env) {
        if caller == fm {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

pub fn require_fee_admin_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    require_fee_manager_or_admin(env, caller, admin)
}

/// Checks that `caller` holds the `EmergencyPause` / `EmergencyAdmin` role **or** is the super-admin.
pub fn require_emergency_pause_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    if caller == admin {
        return Ok(());
    }
    if let Some(ref ep) = get_emergency_pause_role(env) {
        if caller == ep {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

pub fn require_emergency_admin_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    require_emergency_pause_or_admin(env, caller, admin)
}

/// Checks that `caller` holds the `TokenAdmin` role **or** is the super-admin.
pub fn require_token_admin_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    if caller == admin {
        return Ok(());
    }
    if let Some(ref ta) = get_token_admin(env) {
        if caller == ta {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

/// Checks that `caller` holds the `Analytics` role **or** is the super-admin.
///
/// # Read-only intent
///
/// This helper exists solely to gate the documented read-only Analytics entry
/// points (`get_stats`, `get_protocol_stats`, and the audit log).  It MUST NOT
/// be attached to any state-changing entry point: the Analytics role is
/// documented as read-only, and using this helper on a write operation would
/// silently grant write access to analytics holders.  If you need to gate a
/// new admin entry point, choose the role-specific helper that matches its
/// privilege level (or `SuperAdmin`) instead.
pub fn require_analytics_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
) -> Result<(), crate::errors::StreamError> {
    if caller == admin {
        return Ok(());
    }
    if let Some(ref an) = get_analytics_role(env) {
        if caller == an {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

/// Returns whether `caller` has any recognised admin role (super-admin, fee-manager,
/// emergency-pause, token-admin, or analytics).
///
/// Storage reads are ordered by likely hit frequency so the common case
/// short-circuits after the fewest reads: `FeeManager` is checked first because
/// fee-related calls are typically the most frequent admin operation, while
/// `Analytics` (read-only) is checked last. Each match returns early, skipping
/// the remaining storage reads.
pub fn has_any_role(env: &Env, caller: &Address, admin: &Address) -> bool {
    if caller == admin { return true; }
    if get_fee_manager(env).as_ref() == Some(caller) { return true; }
    if get_emergency_pause_role(env).as_ref() == Some(caller) { return true; }
    if get_token_admin(env).as_ref() == Some(caller) { return true; }
    if get_analytics_role(env).as_ref() == Some(caller) { return true; }
    false
}

#[cfg(test)]
mod issue_803_tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn getter_cannot_distinguish_never_assigned_from_revoked_role() {
        let env = Env::default();
        let contract_id = env.register(crate::SoroStreamContract, ());
        let never_assigned = Address::generate(&env);
        let assigned_then_revoked = Address::generate(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(get_fee_manager(&env), None);
            set_fee_manager(&env, &assigned_then_revoked);
            assert_eq!(get_fee_manager(&env), Some(assigned_then_revoked.clone()));
            revoke_fee_manager(&env);

            assert_eq!(get_fee_manager(&env), None);
            // The getter exposes only the current holder; both historical cases
            // produce the same result (the address values are intentionally distinct).
            assert_ne!(never_assigned, assigned_then_revoked);
        });
    }
}

// ── Role-only auth helpers (no SuperAdmin fallback) ──────────────────────────

/// Checks that `caller` holds the `FeeManager` / `FeeAdmin` role only
/// (the SuperAdmin is NOT implicitly authorized).
pub fn require_fee_manager(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    if let Some(ref fm) = get_fee_manager(env) {
        if caller == fm {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

pub fn require_fee_admin(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    require_fee_manager(env, caller)
}

/// Checks that `caller` holds the `EmergencyPause` / `EmergencyAdmin` role only
/// (the SuperAdmin is NOT implicitly authorized).
pub fn require_emergency_pause(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    if let Some(ref ep) = get_emergency_pause_role(env) {
        if caller == ep {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

pub fn require_emergency_admin(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    require_emergency_pause(env, caller)
}

/// Checks that `caller` holds the `TokenAdmin` role only
/// (the SuperAdmin is NOT implicitly authorized).
pub fn require_token_admin(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    if let Some(ref ta) = get_token_admin(env) {
        if caller == ta {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}

/// Checks that `caller` holds the `Analytics` role only
/// (the SuperAdmin is NOT implicitly authorized).
pub fn require_analytics(
    env: &Env,
    caller: &Address,
) -> Result<(), crate::errors::StreamError> {
    if let Some(ref an) = get_analytics_role(env) {
        if caller == an {
            return Ok(());
        }
    }
    Err(crate::errors::StreamError::NotAuthorized)
}
