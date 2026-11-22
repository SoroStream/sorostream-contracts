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
    MigrationAlreadyApplied = 5,
    MigrationFailed = 6,
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

fn check_admin(env: &Env) -> Address {
    let admin = read_admin(env).expect("proxy not initialized");
    admin.require_auth();
    admin
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
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env);
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
        Ok(())
    }

    /// Proposes a new admin. Only admin may call this.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env);
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
    pub fn get_impl_hash(env: Env) -> Result<BytesN<32>, ProxyError> {
        read_impl_hash(&env).ok_or(ProxyError::NotInitialized)
    }

    /// Returns the current storage layout version.
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
        let admin = check_admin(&env);
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
    pub fn upgrade_proxy(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_storage_version: u32,
    ) -> Result<(), ProxyError> {
        let _admin = check_admin(&env);
        if read_storage_version(&env) != expected_storage_version {
            return Err(ProxyError::StorageVersionMismatch);
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Runs a one-time migration step on the implementation contract.
    ///
    /// Forwards a `migrate(from_version, to_version)` call to the implementation.
    /// The implementation is responsible for idempotency guards.
    pub fn migrate(
        env: Env,
        impl_contract: Address,
        from_version: String,
        to_version: String,
    ) -> Result<(), ProxyError> {
        let _admin = check_admin(&env);
        let migration_result = env.invoke_contract::<Result<(), soroban_sdk::Error>>(
            &impl_contract,
            &Symbol::new(&env, "migrate"),
            soroban_sdk::vec![
                &env,
                soroban_sdk::IntoVal::into_val(&from_version, &env),
                soroban_sdk::IntoVal::into_val(&to_version, &env),
            ],
        );
        if migration_result.is_err() {
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
    use soroban_sdk::{contracterror, contractimpl, contract, testutils::Address as _, Address, BytesN, Env, IntoVal};

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

    #[test]
    fn test_accept_admin_second_call_by_current_admin_with_stale_reference_rejected() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        c.propose_admin(&new_admin);
        c.accept_admin(&new_admin);
        assert_eq!(c.get_admin(), new_admin);

        // Second call by the already-accepted admin with a stale acceptance reference fails
        // and leaves the current admin unchanged.
        let stale_retry = c.try_accept_admin(&new_admin);
        assert_eq!(stale_retry, Err(Ok(ProxyError::NotInitialized)));
        assert_eq!(c.get_admin(), new_admin);

        // Even if a subsequent transfer is proposed to a third party, the current admin
        // re-invoking accept_admin with their stale reference must be rejected as Unauthorized.
        let third_admin = Address::generate(&env);
        c.propose_admin(&third_admin);
        let stale_during_new_pending = c.try_accept_admin(&new_admin);
        assert_eq!(stale_during_new_pending, Err(Ok(ProxyError::Unauthorized)));
        assert_eq!(c.get_admin(), new_admin);
    }
}
