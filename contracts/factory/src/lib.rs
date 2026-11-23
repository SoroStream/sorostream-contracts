#![no_std]

#[cfg(test)]
extern crate std;

// StreamFactory — Issue #237
//
// An on-chain registry and factory that deploys child `SoroStreamContract`
// instances at deterministic addresses using a caller-supplied salt.
//
// ## Design
//
// * A single privileged `admin` is stored during `initialize`.
// * `deploy_stream_contract(admin, wasm_hash, salt)` deploys a new stream
//   contract instance deterministically, records deployment metadata, and
//   returns the new contract's `Address`.
// * `get_deployed_contracts(start, limit)` returns a bounded page of child addresses.
// * `get_deployment_info` returns metadata for a single child contract.
//
// ## Storage layout
//
// | Key                        | Type           | Description                        |
// |----------------------------|----------------|------------------------------------|
// | `Symbol("admin")`          | `Address`      | Factory admin                      |
// | `Symbol("contract_count")` | `u32`          | Number of deployed addresses       |
// | `(Symbol("contract"), u32)` | `Address`     | Address at a deployment index       |
// | `(Symbol("info"), Address)` | `DeploymentInfo` | Per-contract metadata            |

#[cfg(test)]
mod test;

#[cfg(test)]
mod storage_bench;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, String, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata stored for each deployed stream contract.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DeploymentInfo {
    /// Address of the deployed child contract.
    pub contract_address: Address,
    /// Address of the account that triggered the deployment.
    pub deployer: Address,
    /// Ledger sequence number at time of deployment.
    pub deploy_ledger: u32,
    /// Unix timestamp at time of deployment.
    pub deploy_timestamp: u64,
    /// WASM hash used for this deployment (encodes the contract version).
    pub wasm_hash: BytesN<32>,
    /// Optional human-readable label supplied by the deployer (e.g.
    /// "production-mainnet-v2") for dashboard identification.
    pub label: Option<String>,
    /// Whether this deployment is still active.
    pub status: DeploymentStatus,
}

/// Lifecycle status recorded by the factory for each deployment.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeploymentStatus {
    Active,
    Deprecated,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const CONTRACTS_KEY: &str = "contracts";
const CONTRACTS_COUNT_KEY: &str = "contracts_count";
const INFO_PREFIX: &str = "info";
const MAX_DEPLOYMENT_PAGE_SIZE: u32 = 20;
const CONTRACT_PREFIX: &str = "contract";

fn admin_key(env: &Env) -> Symbol {
    Symbol::new(env, ADMIN_KEY)
}

fn contract_index_key(env: &Env, index: u32) -> (Symbol, u32) {
    (Symbol::new(env, CONTRACT_PREFIX), index)
}

fn contracts_count_key(env: &Env) -> Symbol {
    Symbol::new(env, CONTRACTS_COUNT_KEY)
}

fn contracts_key(env: &Env) -> Symbol {
    Symbol::new(env, CONTRACTS_KEY)
}

fn info_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, INFO_PREFIX), addr.clone())
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&admin_key(env))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FactoryError {
    /// Factory has already been initialised.
    AlreadyInitialized = 1,
    /// Caller is not the factory admin.
    Unauthorized = 2,
    /// Factory has not been initialised yet.
    NotInitialized = 3,
    /// The proposed admin is the same as the current admin.
    NewAdminSameAsCurrent = 4,
    /// Address was not deployed through this factory.
    UnknownDeployment = 5,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct StreamFactory;

#[contractimpl]
impl StreamFactory {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initializes the factory with an `admin` address.
    ///
    /// Must be called exactly once after deployment. Subsequent calls revert
    /// with `AlreadyInitialized`.
    ///
    /// # Arguments
    /// * `admin` - Address that will be permitted to deploy child contracts.
    pub fn initialize(env: Env, admin: Address) -> Result<(), FactoryError> {
        if env.storage().instance().has(&admin_key(&env)) {
            return Err(FactoryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&admin_key(&env), &admin);
        env.storage()
            .persistent()
            .set(&contracts_count_key(&env), &0u32);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Deployment
    // -----------------------------------------------------------------------

    /// Deploys a child `SoroStreamContract` at a deterministic address derived
    /// from `salt` and records its metadata in the on-chain registry.
    ///
    /// Only the factory admin may call this function.
    ///
    /// # Arguments
    /// * `admin` - Must match the stored admin address (auth required).
    /// * `wasm_hash` - SHA-256 hash of the child contract WASM (previously
    ///   uploaded via `env.deployer().upload_contract_wasm()`).
    /// * `salt` - 32-byte salt used to derive the deterministic address.
    /// * `label` - Optional human-readable label attached to the deployment
    ///   (e.g. "production-mainnet-v2") for dashboard identification.
    ///
    /// # Returns
    /// The `Address` of the newly deployed child contract.
    pub fn deploy_stream_contract(
        env: Env,
        admin: Address,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
        label: Option<String>,
    ) -> Result<Address, FactoryError> {
        // Auth + admin check.
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key(&env))
            .ok_or(FactoryError::NotInitialized)?;
        if admin != stored_admin {
            return Err(FactoryError::Unauthorized);
        }

        // Deploy child contract at deterministic address.
        let child_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_v2(wasm_hash.clone(), ());

        // Build and persist deployment metadata.
        let info = DeploymentInfo {
            contract_address: child_address.clone(),
            deployer: admin.clone(),
            deploy_ledger: env.ledger().sequence(),
            deploy_timestamp: env.ledger().timestamp(),
            wasm_hash,
            label,
            status: DeploymentStatus::Active,
        };
        env.storage()
            .persistent()
            .set(&info_key(&env, &child_address), &info);

        // Store each address separately so append and paginated reads stay bounded.
        let count = Self::get_deployed_contract_count(env.clone());
        env.storage()
            .persistent()
            .set(&contract_index_key(&env, count), &child_address);
        env.storage()
            .persistent()
            .set(&contracts_count_key(&env), &(count + 1));
        env.events().publish(
            (Symbol::new(&env, "StreamContractDeployed"),),
            (child_address.clone(), admin, info.deploy_ledger, info.wasm_hash),
        );

        Ok(child_address)
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    /// Returns a page of deployed child contract addresses in deployment order.
    ///
    /// `start` is the zero-based deployment index and `limit` is capped at 20.
    pub fn get_deployed_contracts(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let count: u32 = env.storage()
            .persistent()
            .get(&contracts_count_key(&env))
            .unwrap_or(0);
        let end = start
            .saturating_add(limit.min(MAX_DEPLOYMENT_PAGE_SIZE))
            .min(count);
        let mut page = Vec::new(&env);

        for index in start..end {
            if let Some(address) = env
                .storage()
                .persistent()
                .get(&contract_index_key(&env, index))
            {
                page.push_back(address);
            }
        }

        page
    }

    /// Returns the number of deployed child contracts without loading the registry.
    ///
    /// For factories created before this count was persisted, falls back to the
    /// registry length until the next deployment records the count.
    pub fn get_deployed_contract_count(env: Env) -> u32 {
        if let Some(count) = env
            .storage()
            .persistent()
            .get(&contracts_count_key(&env))
        {
            return count;
        }

        env.storage()
            .persistent()
            .get::<_, Vec<Address>>(&contracts_key(&env))
            .unwrap_or(Vec::new(&env))
            .len()
    }

    /// Returns deployment metadata for a single child contract address.
    ///
    /// Returns `None` if the address was not deployed through this factory.
    pub fn get_deployment_info(env: Env, contract_address: Address) -> Option<DeploymentInfo> {
        env.storage()
            .persistent()
            .get(&info_key(&env, &contract_address))
    }

    /// Marks a deployment deprecated. Only the current factory admin may do this.
    pub fn deprecate_deployment(
        env: Env,
        admin: Address,
        contract_address: Address,
    ) -> Result<(), FactoryError> {
        admin.require_auth();
        let stored_admin = read_admin(&env).ok_or(FactoryError::NotInitialized)?;
        if admin != stored_admin {
            return Err(FactoryError::Unauthorized);
        }
        let mut info: DeploymentInfo = env
            .storage()
            .persistent()
            .get(&info_key(&env, &contract_address))
            .ok_or(FactoryError::UnknownDeployment)?;
        info.status = DeploymentStatus::Deprecated;
        env.storage()
            .persistent()
            .set(&info_key(&env, &contract_address), &info);
        env.events().publish(
            (Symbol::new(&env, "DeploymentDeprecated"),),
            contract_address,
        );
        Ok(())
    }

    /// Returns a page containing only active deployments, in deployment order.
    pub fn get_active_deployed_contracts(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let mut active = Vec::new(&env);
        let count = Self::get_deployed_contract_count(env.clone());
        let end = start.saturating_add(limit.min(MAX_DEPLOYMENT_PAGE_SIZE)).min(count);
        for index in start..end {
            let Some(address) = env
                .storage()
                .persistent()
                .get::<_, Address>(&contract_index_key(&env, index))
            else {
                continue;
            };
            let is_active = env
                .storage()
                .persistent()
                .get::<_, DeploymentInfo>(&info_key(&env, &address))
                .map(|info| info.status == DeploymentStatus::Active)
                .unwrap_or(true);
            if is_active {
                active.push_back(address);
            }
        }
        active
    }

    /// Returns the current factory admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&admin_key(&env))
    }

    /// Proposes a new admin. Only the current admin may call this.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&admin_key(&env))
            .ok_or(FactoryError::NotInitialized)?;
        admin.require_auth();
        if new_admin == admin {
            return Err(FactoryError::NewAdminSameAsCurrent);
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
    pub fn accept_admin(env: Env, accepted_by: Address) -> Result<(), FactoryError> {
        accepted_by.require_auth();
        let pending = read_pending_admin(&env)
            .ok_or(FactoryError::NotInitialized)?;
        if accepted_by != pending {
            return Err(FactoryError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&admin_key(&env), &accepted_by);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        env.events().publish(
            (Symbol::new(&env, "AdminTransferAccepted"),),
            accepted_by,
        );
        Ok(())
    }
}
