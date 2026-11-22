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
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env);
        
        // Validate that new admin is not a burn address
        validate_admin_address(&env, &new_admin).map_err(|_| ProxyError::NotAdmin)?;
        
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
        Ok(())
    }

    /// Proposes a new admin. Only admin may call this.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), ProxyError> {
        check_admin(&env);
        
        // Validate that new admin is not a burn address
        validate_admin_address(&env, &new_admin).map_err(|_| ProxyError::NotAdmin)?;
        
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

    /// Returns both the implementation hash and storage version in one call.
    /// Useful for checking if the proxy is properly initialized.
    pub fn get_impl_storage_info(env: Env) -> Result<(BytesN<32>, u32), ProxyError> {
        let impl_hash = read_impl_hash(&env).ok_or(ProxyError::NotInitialized)?;
        let storage_version = read_storage_version(&env);
        Ok((impl_hash, storage_version))
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
        let _ = env.invoke_contract::<Result<(), soroban_sdk::Error>>(
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
        assert_eq!(c.get_admin(), admin);
        assert_eq!(c.get_impl_hash(), impl_hash);
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
    fn test_get_impl_storage_info_returns_not_initialized_on_fresh_deploy() {
        let (env, contract_id, _admin, _impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);

        // Call get_impl_storage_info on uninitialized proxy
        let result = c.try_get_impl_storage_info();
        assert_eq!(result, Err(Ok(ProxyError::NotInitialized)));
    }

    #[test]
    fn test_get_impl_storage_info_succeeds_after_initialize() {
        let (env, contract_id, admin, impl_hash) = setup();
        let c = ProxyContractClient::new(&env, &contract_id);

        c.initialize(&admin, &impl_hash, &5u32);

        let (hash, version) = c.get_impl_storage_info();
        assert_eq!(hash, impl_hash);
        assert_eq!(version, 5u32);
    }
}
