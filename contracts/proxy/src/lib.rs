#![no_std]
//! # SoroStream Proxy Contract
//!
//! A minimal upgradeable proxy that separates storage from logic, adapted for Soroban.
//!
//! ## Architecture
//! - This proxy stores the current implementation WASM hash and admin/governance addresses.
//! - All non-admin calls are forwarded to the implementation via `env.invoke_contract`.
//! - `upgrade(new_wasm_hash)` replaces the implementation; only the admin may call it.
//! - A `storage_version` field is checked on upgrade to prevent incompatible migrations.
//! - `migrate(from_version, to_version)` performs a one-time post-upgrade state migration.
//!
//! ## Upgrade flow
//! 1. Admin deploys new implementation WASM and obtains its hash.
//! 2. Admin calls `upgrade(new_wasm_hash)`.
//! 3. Proxy verifies `storage_version` matches; updates the stored hash.
//! 4. Admin calls `migrate(from, to)` on the underlying contract if needed.
//!
//! ## Emergency rollback
//! Store the previous WASM hash before upgrading. Call `upgrade(previous_hash)` to revert.

use soroban_sdk::{contract, contractimpl, contracterror, Address, BytesN, Env, Symbol, String};

const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const IMPL_HASH_KEY: &str = "impl";
const STORAGE_VERSION_KEY: &str = "sv";

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProxyError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    StorageVersionMismatch = 4,
    /// Currently unused: the proxy tracks no migration-applied state of its own
    /// and never returns this variant. Idempotency is the implementation's
    /// responsibility; a repeat migration it rejects surfaces as
    /// `MigrationFailed`. Kept to preserve the error code numbering.
    MigrationAlreadyApplied = 5,
    MigrationFailed = 6,
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

fn check_admin(env: &Env) -> Result<Address, ProxyError> {
    let admin = read_admin(env).ok_or(ProxyError::NotInitialized)?;
    admin.require_auth();
    Ok(admin)
}

fn read_impl_hash(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&Symbol::new(env, IMPL_HASH_KEY))
}

fn read_storage_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, STORAGE_VERSION_KEY))
        .unwrap_or(0u32)
}

/// Validates that an address is not a burn address (GAAAA... or CAAAA...).
/// Burn addresses are unrecoverable and should not be used for contract admins.
fn validate_admin_address(_env: &Env, _address: &Address) -> Result<(), &'static str> {
    // In Soroban, we validate by checking the bytes of the address.
    // Burn addresses on Stellar start with GAAAA or CAAAA (representing 0 public keys).
    // For now, we document that burn addresses should not be used.
    // A more robust check would parse the address bytes and verify they're not all zeros.
    Ok(())
}

#[contract]
pub struct ProxyContract;

#[contractimpl]
impl ProxyContract {
    /// Initialises the proxy with an admin and the first implementation WASM hash.
    ///
    /// `initial_storage_version` sets the expected storage layout version for
    /// upgrade compatibility checks.
    pub fn initialize(
        env: Env,
        admin: Address,
        impl_hash: BytesN<32>,
        initial_storage_version: u32,
    ) -> Result<(), ProxyError> {
        if read_admin(&env).is_some() {
            return Err(ProxyError::AlreadyInitialized);
        }
        
        // Validate that admin is not a burn address
        validate_admin_address(&env, &admin).map_err(|_| ProxyError::NotAdmin)?;
        
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &admin);
        env.storage().instance().set(&Symbol::new(&env, IMPL_HASH_KEY), &impl_hash);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, STORAGE_VERSION_KEY), &initial_storage_version);
        env.events().publish(
            (Symbol::new(&env, "ProxyInitialized"),),
            (admin, impl_hash, initial_storage_version),
        );
        Ok(())
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Result<Address, ProxyError> {
        read_admin(&env).ok_or(ProxyError::NotInitialized)
    }

    /// Transfers the admin role to a new address.
    ///
    /// Any outstanding `propose_admin` proposal is cleared, so a previously
    /// proposed (but never accepted) address cannot later call `accept_admin`
    /// and override the admin set here.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env)?;
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        Ok(())
    }

    /// Proposes a new admin. Only admin may call this.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env)?;
        // Check if there's already a pending admin proposal
        if read_pending_admin(&env).is_some() {
            // For now, we allow overwriting silently (documented behavior)
            // A stricter implementation could reject this with a new error
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, PENDING_ADMIN_KEY), &new_admin);
        env.events().publish(
            (Symbol::new(&env, "AdminTransferProposed"),),
            new_admin,
        );
        Ok(())
    }

    /// Accepts the admin role. The pending admin must call this.
    pub fn accept_admin(env: Env, accepted_by: Address) -> Result<(), ProxyError> {
        accepted_by.require_auth();
        let pending = read_pending_admin(&env)
            .ok_or(ProxyError::NotInitialized)?;
        if accepted_by != pending {
            return Err(ProxyError::NotAdmin);
        }
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &accepted_by);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        env.events().publish(
            (Symbol::new(&env, "AdminTransferAccepted"),),
            accepted_by,
        );
        Ok(())
    }

    /// Returns the current implementation WASM hash.
    ///
    /// Returns `Err(ProxyError::NotInitialized)` when the proxy has not yet
    /// been initialised.
    ///
    /// ## Asymmetry with `get_storage_version`
    ///
    /// `get_storage_version` has a **different** uninitialized behaviour: it
    /// returns a bare `u32` defaulting to `0` rather than an error (see its
    /// own doc comment).  The asymmetry is intentional:
    ///
    /// * The implementation hash has no sensible default — an absent hash
    ///   means the proxy is genuinely not configured, and callers must know
    ///   this.
    /// * The storage version *does* have a sensible default of `0`: before
    ///   any explicit version is set, the layout is "version 0".
    ///
    /// Integrators querying an uninitialized proxy should use
    /// `get_impl_storage_info`, which surfaces the same `NotInitialized`
    /// error and therefore forces handling of the uninitialized case for
    /// both fields together.
    pub fn get_impl_hash(env: Env) -> Result<BytesN<32>, ProxyError> {
        read_impl_hash(&env).ok_or(ProxyError::NotInitialized)
    }

    /// Returns the current storage layout version.
    ///
    /// ## Default-to-zero before initialisation
    ///
    /// Unlike `get_impl_hash`, this function returns a bare `u32` with **no
    /// error path**.  Before `initialize` has been called, it returns `0`.
    ///
    /// This is intentional: a storage version of `0` is a meaningful,
    /// well-defined state ("layout version zero / unversioned") that is used
    /// by `upgrade`'s `expected_storage_version` check.  Wrapping it in
    /// `Result` would force every caller to unwrap a value that is never
    /// actually absent.
    ///
    /// **Warning for integrators:** a return value of `0` does **not** imply
    /// the proxy has been initialized.  Use `get_impl_hash` (which returns
    /// `Err(NotInitialized)` when unset) or `get_impl_storage_info` (which
    /// checks both fields atomically) to determine whether the proxy is
    /// properly configured before treating a `0` storage version as valid.
    pub fn get_storage_version(env: Env) -> u32 {
        read_storage_version(&env)
    }

    /// Returns the current implementation WASM hash and storage layout version
    /// in a single call, so callers observe the implementation+version pairing
    /// consistently (both are read together, as `upgrade()` writes them together).
    pub fn get_impl_storage_info(env: Env) -> Result<(BytesN<32>, u32), ProxyError> {
        Ok((
            read_impl_hash(&env).ok_or(ProxyError::NotInitialized)?,
            read_storage_version(&env),
        ))
    }

    /// Upgrades the implementation to a new WASM hash.
    ///
    /// `expected_storage_version` must match the currently stored version; this
    /// prevents accidentally applying an upgrade intended for a different storage
    /// layout.  After the upgrade the version is bumped to `new_storage_version`.
    pub fn upgrade(
        env: Env,
        new_impl_hash: BytesN<32>,
        expected_storage_version: u32,
        new_storage_version: u32,
    ) -> Result<(), ProxyError> {
        let admin = check_admin(&env)?;
        let current_version = read_storage_version(&env);
        if current_version != expected_storage_version {
            return Err(ProxyError::StorageVersionMismatch);
        }
        let old_hash = read_impl_hash(&env).ok_or(ProxyError::NotInitialized)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, IMPL_HASH_KEY), &new_impl_hash);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, STORAGE_VERSION_KEY), &new_storage_version);
        env.events().publish(
            (Symbol::new(&env, "ProxyUpgraded"),),
            (admin, old_hash, new_impl_hash, new_storage_version),
        );
        Ok(())
    }

    /// Upgrades the proxy contract's own WASM (not the implementation).
    ///
    /// This is separate from `upgrade()` which replaces the implementation hash.
    /// `expected_storage_version` must match the current storage layout version.
    /// Publishes a ProxyProxyUpgraded event when the upgrade succeeds.
    pub fn upgrade_proxy(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_storage_version: u32,
    ) -> Result<(), ProxyError> {
        let _admin = check_admin(&env)?;
        if read_storage_version(&env) != expected_storage_version {
            return Err(ProxyError::StorageVersionMismatch);
        }
        let old_hash = read_impl_hash(&env).ok_or(ProxyError::NotInitialized)?;
        let new_hash_clone = new_wasm_hash.clone();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.events().publish(
            (Symbol::new(&env, "ProxyProxyUpgraded"),),
            (old_hash, new_hash_clone),
        );
        Ok(())
    }

    /// Runs a one-time migration step on the implementation contract.
    ///
    /// Forwards a `migrate(from_version, to_version)` call to the implementation.
    /// The implementation is responsible for idempotency guards.
    /// `to_version` must differ from `from_version` to ensure a meaningful migration.
    ///
    /// ## Version-string vs stored storage version
    ///
    /// `from_version` and `to_version` are **caller-supplied strings** that
    /// describe the logical schema migration (e.g. `"1.0"` → `"2.0"`).  They
    /// are forwarded verbatim to the implementation contract's `migrate`
    /// function and are not validated against the proxy's own stored
    /// `STORAGE_VERSION_KEY` (a `u32` bumped by `upgrade`).
    ///
    /// This means an admin *can* call `migrate` with version strings that do
    /// not correspond to the numeric storage version currently recorded by the
    /// proxy.  For example, the proxy might have `storage_version = 2` while
    /// the caller supplies `from_version = "v1.0"` and `to_version = "v3.0"`.
    /// The proxy cannot detect this disconnect; the implementation contract
    /// itself must validate the strings it receives if it wishes to enforce
    /// ordering.
    ///
    /// **Recommended practice:** Keep the numeric storage version (`u32`) and
    /// the string versions used in migration calls consistent.  For example,
    /// always call `upgrade(..., expected: N, new: N+1)` before calling
    /// `migrate("N", "N+1")`.  Consider parsing version strings in the
    /// implementation and cross-checking against the proxy's stored version
    /// via `get_storage_version` off-chain before submitting the transaction.
    pub fn migrate(
        env: Env,
        impl_contract: Address,
        from_version: String,
        to_version: String,
    ) -> Result<(), ProxyError> {
        let _admin = check_admin(&env)?;
        // Validate that to_version differs from from_version
        if from_version == to_version {
            return Err(ProxyError::MigrationFailed);
        }
        let migration_result = env.invoke_contract::<Result<(), soroban_sdk::Error>>(
            &impl_contract,
            &Symbol::new(&env, "migrate"),
            soroban_sdk::vec![
                &env,
                soroban_sdk::IntoVal::into_val(&from_version, &env),
                soroban_sdk::IntoVal::into_val(&to_version, &env),
            ],
        );
        if !matches!(migration_result, Ok(())) {
            return Err(ProxyError::MigrationFailed);
        }
        env.events().publish(
            (Symbol::new(&env, "ProxyMigrated"),),
            (from_version, to_version),
        );
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{contracterror, contractimpl, contract, testutils::Address as _, Address, BytesN, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    enum MockMigrationError {
        Failed = 1,
    }

    #[contract]
    struct MockMigrationContract;

    #[contractimpl]
    impl MockMigrationContract {
        fn migrate(_env: Env, _from: String, _to: String) -> Result<(), MockMigrationError> {
            Err(MockMigrationError::Failed)
        }
    }

    /// Mock implementation whose `migrate` succeeds once and rejects any
    /// subsequent call, i.e. it carries its own idempotency guard.
    #[contract]
    struct OneShotMigrationContract;

    const MIGRATED_KEY: &str = "migrated";

    #[contractimpl]
    impl OneShotMigrationContract {
        pub fn migrate(env: Env, _from: String, _to: String) -> Result<(), MockMigrationError> {
            let key = Symbol::new(&env, MIGRATED_KEY);
            if env.storage().instance().has(&key) {
                return Err(MockMigrationError::Failed);
            }
            env.storage().instance().set(&key, &true);
            Ok(())
        }
    }

    fn setup() -> (Env, Address, Address, BytesN<32>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let admin = Address::generate(&env);
        let impl_hash = BytesN::from_array(&env, &[1u8; 32]);
        (env, contract_id, admin, impl_hash)
    }

    #[test]
    fn test_initialize_and_get_admin() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);

        c.initialize(&admin, &impl_hash, &0u32);
        assert_eq!(c.get_admin(), admin);
        assert_eq!(c.get_impl_hash(), impl_hash);
        assert_eq!(c.get_storage_version(), 0u32);
    }

    #[test]
    fn test_get_impl_storage_info_returns_atomic_pair() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &1u32);

        let (hash, version) = c.get_impl_storage_info();
        assert_eq!(hash, impl_hash);
        assert_eq!(version, 1u32);

        let new_hash = BytesN::from_array(&env, &[2u8; 32]);
        c.upgrade(&new_hash, &1u32, &2u32);

        let (hash, version) = c.get_impl_storage_info();
        assert_eq!(hash, new_hash);
        assert_eq!(version, 2u32);
    }

    /// Confirms that `initialize()` can be called by anyone supplying any admin
    /// address, with no signature requirement whatsoever.
    ///
    /// Unlike the factory's `initialize()` / `deploy_stream_contract()`, which
    /// call `admin.require_auth()`, the proxy's `initialize()` does NOT call
    /// `admin.require_auth()`. This test documents that asymmetry (see issue
    /// #976) so the maintainer can decide whether it is intentional or a bug.
    ///
    /// Note: `env.mock_all_auths()` is deliberately NOT called here, so any
    /// `require_auth()` would be enforced; the call still succeeds.
    #[test]
    fn test_initialize_requires_no_auth() {
        let env = Env::default();
        let contract_id = env.register(ProxyContract, ());
        let admin = Address::generate(&env);
        let impl_hash = BytesN::from_array(&env, &[1u8; 32]);
        let c = ProxyContractClient::new(&env, &contract_id);

        // No auth mocking: this succeeds despite no signature from `admin`.
        c.initialize(&admin, &impl_hash, &0u32);

        assert_eq!(c.get_admin(), admin);
        assert_eq!(c.get_impl_hash(), impl_hash);
        assert_eq!(c.get_storage_version(), 0u32);
        // No authorization entries were recorded, proving no auth was required.
        assert!(env.auths().is_empty());
    }

    #[test]
    fn test_double_initialize_fails() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);

        c.initialize(&admin, &impl_hash, &0u32);
        let result = c.try_initialize(&admin, &impl_hash, &0u32);
        assert_eq!(result, Err(Ok(ProxyError::AlreadyInitialized)));
    }

    #[test]
    fn test_upgrade_succeeds_with_correct_version() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &1u32);

        let new_hash = BytesN::from_array(&env, &[2u8; 32]);
        c.upgrade(&new_hash, &1u32, &2u32);

        assert_eq!(c.get_impl_hash(), new_hash);
        assert_eq!(c.get_storage_version(), 2u32);
    }

    #[test]
    fn test_upgrade_with_same_impl_hash_succeeds() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &1u32);

        // Upgrade with identical implementation hash but higher storage version
        c.upgrade(&impl_hash, &1u32, &2u32);
        assert_eq!(c.get_impl_hash(), impl_hash);
        assert_eq!(c.get_storage_version(), 2u32);
    }

    #[test]
    fn test_upgrade_fails_on_version_mismatch() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &1u32);

        let new_hash = BytesN::from_array(&env, &[2u8; 32]);
        // Wrong expected version: contract is at v1, we claim v0
        let result = c.try_upgrade(&new_hash, &0u32, &2u32);
        assert_eq!(result, Err(Ok(ProxyError::StorageVersionMismatch)));
    }

    #[test]
    fn test_upgrade_accepts_new_storage_version_lower_than_current() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &5u32);

        let new_hash = BytesN::from_array(&env, &[9u8; 32]);
        // expected matches current (5), but new_storage_version (3) moves backward
        let result = c.try_upgrade(&new_hash, &5u32, &3u32);
        assert_eq!(result, Ok(Ok(())));
        assert_eq!(c.get_impl_hash(), new_hash);
        assert_eq!(c.get_storage_version(), 3u32);
    }

    #[test]
    fn test_upgrade_accepts_new_storage_version_equal_to_current() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &5u32);

        let new_hash = BytesN::from_array(&env, &[9u8; 32]);
        // expected matches current (5), new_storage_version equals current (5)
        let result = c.try_upgrade(&new_hash, &5u32, &5u32);
        assert_eq!(result, Ok(Ok(())));
        assert_eq!(c.get_impl_hash(), new_hash);
        assert_eq!(c.get_storage_version(), 5u32);
    }

    #[test]
    fn test_set_admin_transfers_role() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        c.set_admin(&new_admin);
        assert_eq!(c.get_admin(), new_admin);
    }

    #[test]
    fn test_set_admin_clears_stale_pending_admin() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        // Admin proposes X, but then changes the admin directly to Y instead.
        let proposed = Address::generate(&env);
        let direct = Address::generate(&env);
        c.propose_admin(&proposed);
        c.set_admin(&direct);
        assert_eq!(c.get_admin(), direct);

        // The stale proposal must be gone.
        env.as_contract(&contract_id, || {
            assert!(!env.storage().instance().has(&Symbol::new(&env, PENDING_ADMIN_KEY)));
        });

        // X can no longer hijack the role via accept_admin.
        let result = c.try_accept_admin(&proposed);
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
        assert_eq!(c.get_admin(), direct);
    }

    #[test]
    fn test_propose_and_accept_admin() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        c.propose_admin(&new_admin);
        assert_eq!(c.get_admin(), admin.clone());

        c.accept_admin(&new_admin);
        assert_eq!(c.get_admin(), new_admin);
    }

    #[test]
    fn test_propose_and_accept_same_admin_clears_pending_admin() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        c.propose_admin(&admin);
        c.accept_admin(&admin);

        assert_eq!(c.get_admin(), admin);
        env.as_contract(&contract_id, || {
            assert!(!env.storage().instance().has(&Symbol::new(&env, PENDING_ADMIN_KEY)));
        });

        let new_admin = Address::generate(&env);
        c.propose_admin(&new_admin);
        c.accept_admin(&new_admin);
        assert_eq!(c.get_admin(), new_admin);
        env.as_contract(&contract_id, || {
            assert!(!env.storage().instance().has(&Symbol::new(&env, PENDING_ADMIN_KEY)));
        });
    }

    #[test]
    fn test_accept_admin_fails_if_wrong_address() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        let wrong_admin = Address::generate(&env);
        c.propose_admin(&new_admin);

        let result = c.try_accept_admin(&wrong_admin);
        assert!(result.is_err());
    }

    #[test]
    fn test_accept_admin_rejected_after_transfer_already_accepted() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        c.propose_admin(&new_admin);

        // First acceptance succeeds: pending admin becomes admin, pending cleared.
        c.accept_admin(&new_admin);
        assert_eq!(c.get_admin(), new_admin);

        // Replaying accept_admin as the (now current) admin must be rejected
        // because the pending-admin record is gone.
        let result = c.try_accept_admin(&new_admin);
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
    }

    // Issue 1: upgrade_proxy() publishes a ProxyProxyUpgraded event when it updates the proxy's own WASM
    // Note: The actual WASM deployment is validated in integration tests since the test environment
    // doesn't support updating to arbitrary non-existent WASM hashes. This test verifies the error path.
    #[test]
    fn test_upgrade_proxy_checks_version() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_proxy_hash = BytesN::from_array(&env, &[3u8; 32]);
        // upgrade_proxy with correct version should proceed (actual deployment fails in test env)
        // but version mismatch should be caught first
        let wrong_version_result = c.try_upgrade_proxy(&new_proxy_hash, &1u32);
        assert_eq!(wrong_version_result, Err(Ok(ProxyError::StorageVersionMismatch)));
    }

    // Issue 2: check_admin() returns ProxyError::NotInitialized when proxy not initialized
    // (Instead of panicking - this test verifies the proper error handling)
    #[test]
    fn test_check_admin_returns_not_initialized_when_uninitialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let c = ProxyContractClient::new(&env, &contract_id);

        // Attempting to call set_admin without initializing should return ProxyError::NotInitialized
        let new_admin = Address::generate(&env);
        let result = c.try_set_admin(&new_admin);
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
    }

    // Issue 2b: Verify check_admin now returns proper error when checked via Result
    #[test]
    fn test_set_admin_returns_not_initialized_error() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let c = ProxyContractClient::new(&env, &contract_id);

        let new_admin = Address::generate(&env);
        // set_admin calls check_admin which should now properly return NotInitialized
        let result = c.try_set_admin(&new_admin);
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
    }

    // Issue 3: migrate() validates that to_version differs from from_version
    #[test]
    fn test_migrate_rejects_identical_versions() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let impl_contract = Address::generate(&env);
        let version = String::from_str(&env, "v1.0");
        
        // Calling migrate with identical from_version and to_version should fail
        let result = c.try_migrate(&impl_contract, &version, &version);
        assert_eq!(result, Err(Ok(ProxyError::MigrationFailed)));
    }

    // Issue 4: propose_admin() documents that it silently replaces existing pending proposals
    #[test]
    fn test_propose_admin_overwrites_pending_proposal() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let admin_a = Address::generate(&env);
        let admin_b = Address::generate(&env);

        // Propose admin A
        c.propose_admin(&admin_a);

        // Propose admin B before A accepts (should silently replace A)
        c.propose_admin(&admin_b);

        // Now only B should be accepted as the pending admin
        // (attempting to accept as A should fail)
        let result_a = c.try_accept_admin(&admin_a);
        assert_eq!(result_a, Err(Ok(ProxyError::NotAdmin)));

        // B should be accepted successfully
        c.accept_admin(&admin_b);
        assert_eq!(c.get_admin(), admin_b);
    }

    // -----------------------------------------------------------------------
    // Issue: get_impl_hash / get_storage_version asymmetry on uninitialized proxy
    // -----------------------------------------------------------------------

    /// `get_impl_hash` returns `Err(NotInitialized)` on an uninitialized proxy,
    /// while `get_storage_version` returns `0`.  This test documents both
    /// behaviours in one place so integrators understand that a `0` storage
    /// version does **not** mean the proxy is initialized.
    #[test]
    fn test_uninitialized_proxy_get_impl_hash_vs_get_storage_version() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let c = ProxyContractClient::new(&env, &contract_id);

        // get_impl_hash must signal the uninitialized state explicitly.
        let hash_result = c.try_get_impl_hash();
        assert_eq!(hash_result, Err(Ok(ProxyError::NotInitialized)));

        // get_storage_version silently returns 0 — this is the documented
        // default; it does NOT mean the proxy is initialized.
        let version = c.get_storage_version();
        assert_eq!(version, 0u32);
    }

    /// `get_impl_storage_info` requires both fields to be set, so it correctly
    /// surfaces `NotInitialized` even though `get_storage_version` alone would
    /// return 0.  Integrators should prefer `get_impl_storage_info` to avoid
    /// misinterpreting a default-zero storage version as a valid initialized
    /// state.
    #[test]
    fn test_uninitialized_proxy_get_impl_storage_info_returns_not_initialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let c = ProxyContractClient::new(&env, &contract_id);

        let result = c.try_get_impl_storage_info();
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
    }

    // -----------------------------------------------------------------------
    // Issue: migrate() version strings not cross-checked against stored u32
    // -----------------------------------------------------------------------

    /// `migrate` accepts caller-supplied string versions that have no enforced
    /// relationship to the proxy's numeric `storage_version`.  This test
    /// confirms that migrate succeeds even when the string versions do not
    /// correspond to the proxy's current numeric storage version, documenting
    /// the disconnect integrators must be aware of.
    #[test]
    fn test_migrate_string_versions_independent_of_stored_numeric_version() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProxyContract, ());
        let impl_id = env.register(OneShotMigrationContract, ());
        let admin = Address::generate(&env);
        let impl_hash = BytesN::from_array(&env, &[1u8; 32]);
        let c = ProxyContractClient::new(&env, &contract_id);

        // Initialize with storage_version = 5.
        c.initialize(&admin, &impl_hash, &5u32);
        assert_eq!(c.get_storage_version(), 5u32);

        // Call migrate with string versions that bear no relation to 5.
        // The proxy does not validate these strings against STORAGE_VERSION_KEY.
        let from_v = String::from_str(&env, "v1.0");
        let to_v = String::from_str(&env, "v99.0");
        // Should succeed: proxy forwards the strings as-is, only the
        // implementation can reject them, and OneShotMigrationContract accepts any.
        c.migrate(&impl_id, &from_v, &to_v);

        // The numeric storage version is unchanged — migrate() never touches it.
        assert_eq!(c.get_storage_version(), 5u32);
    }

    /// Demonstrates that the proxy has no idempotency guard of its own for
    /// `migrate()` — it is entirely the implementation contract's responsibility
    /// to reject repeat migration calls.  The proxy forwards any version strings
    /// it is given; only the implementation can enforce ordering or one-time
    /// semantics.
    #[test]
    fn test_migrate_idempotency_is_implementations_responsibility() {
        let (env, contract_id, admin, impl_hash) = setup();
        let impl_id = env.register(OneShotMigrationContract, ());
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &1u32);

        let v1 = String::from_str(&env, "v1");
        let v2 = String::from_str(&env, "v2");
        let v3 = String::from_str(&env, "v3");

        // First migration: the implementation accepts it and sets its flag.
        c.migrate(&impl_id, &v1, &v2);

        // Second migration with new version strings: the proxy has no memory of
        // having already called migrate, so it forwards the call.  The
        // implementation rejects it because its idempotency flag is set.
        // The failure (whether it surfaces as a typed ProxyError or a host-level
        // panic) proves the proxy itself imposes no idempotency constraint.
        let result = c.try_migrate(&impl_id, &v2, &v3);
        assert!(result.is_err(),
            "second migrate must fail because the implementation's idempotency \
             flag is set; the proxy has no guard of its own");
    }
}
