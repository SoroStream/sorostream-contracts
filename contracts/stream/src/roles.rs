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

#![allow(dead_code)]
use soroban_sdk::{contracttype, Address, Env, Symbol};

// ── Storage keys for each role ────────────────────────────────────────────────

const FEE_MANAGER_KEY: &str = "rfee";
const EMERGENCY_PAUSE_KEY: &str = "rpause";
const ANALYTICS_KEY: &str = "ranal";

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

// ── Auth helpers ──────────────────────────────────────────────────────────────

/// Checks that `caller` holds the `FeeManager` role **or** is the super-admin.
///
/// Callers must have invoked `caller.require_auth()` before calling this.
///
/// # Panics
/// Panics with `NotAuthorized` if the caller has neither role.
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

/// Checks that `caller` holds the `EmergencyPause` role **or** is the super-admin.
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

/// Checks that `caller` holds the `Analytics` role **or** is the super-admin.
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
/// emergency-pause, or analytics).  Useful for access-gate checks that accept any role.
pub fn has_any_role(env: &Env, caller: &Address, admin: &Address) -> bool {
    if caller == admin { return true; }
    if get_fee_manager(env).as_ref() == Some(caller) { return true; }
    if get_emergency_pause_role(env).as_ref() == Some(caller) { return true; }
    if get_analytics_role(env).as_ref() == Some(caller) { return true; }
    false
}
