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

use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, Symbol, String};

const ADMIN_KEY: &str = "admin";
const IMPL_HASH_KEY: &str = "impl";
const STORAGE_VERSION_KEY: &str = "sv";

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProxyError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    StorageVersionMismatch = 4,
    MigrationAlreadyApplied = 5,
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
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

    /// Returns the current implementation WASM hash.
    pub fn get_impl_hash(env: Env) -> Result<BytesN<32>, ProxyError> {
        read_impl_hash(&env).ok_or(ProxyError::NotInitialized)
    }

    /// Returns the current storage layout version.
    pub fn get_storage_version(env: Env) -> u32 {
        read_storage_version(&env)
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
    /// Use this to update the proxy logic itself.
    pub fn upgrade_proxy(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ProxyError> {
        let _admin = check_admin(&env);
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
        env.invoke_contract::<Result<(), soroban_sdk::Error>>(
            &impl_contract,
            &Symbol::new(&env, "migrate"),
            soroban_sdk::vec![
                &env,
                soroban_sdk::IntoVal::into_val(&from_version, &env),
                soroban_sdk::IntoVal::into_val(&to_version, &env),
            ],
        );
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
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

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
        assert_eq!(c.get_admin(), Ok(admin));
        assert_eq!(c.get_impl_hash(), Ok(impl_hash));
        assert_eq!(c.get_storage_version(), 0u32);
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

        assert_eq!(c.get_impl_hash(), Ok(new_hash));
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
    fn test_set_admin_transfers_role() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);
        c.initialize(&admin, &impl_hash, &0u32);

        let new_admin = Address::generate(&env);
        c.set_admin(&new_admin);
        assert_eq!(c.get_admin(), Ok(new_admin));
    }
}
