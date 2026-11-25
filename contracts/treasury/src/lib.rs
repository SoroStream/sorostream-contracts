#![no_std]

//! # SoroStream Treasury Contract
//!
//! Holds accumulated protocol fees and provides configurable distribution
//! between a treasury wallet and an LP reward pool.

#[cfg(test)]
extern crate std;

use soroban_sdk::{contract, contractimpl, contracterror, token, Address, Env, Symbol};

const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const LP_POOL_KEY: &str = "lp_pool";
const TREASURY_SPLIT_BPS_KEY: &str = "t_split";

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TreasuryError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidTreasuryAddress = 3,
    InsufficientBalance = 4,
    LpPoolNotConfigured = 5,
    InvalidTreasurySplit = 6,
    NoPendingAdmin = 7,
    NotPendingAdmin = 8,
    InvalidAmount = 9,
    Overflow = 10,
    InvalidLpPoolAddress = 11,
    LpPoolIsTreasuryContract = 12,
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

fn read_lp_pool(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, LP_POOL_KEY))
}

/// Returns the treasury split in basis points (100 bps = 1%).
/// The remainder (10_000 - treasury_bps) goes to the LP reward pool.
fn read_treasury_split_bps(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, TREASURY_SPLIT_BPS_KEY))
        .unwrap_or(10_000u32) // default: 100% to treasury, 0% to LP
}

fn check_admin(env: &Env) -> Result<(), TreasuryError> {
    let admin = read_admin(env).ok_or(TreasuryError::NotInitialized)?;
    admin.require_auth();
    Ok(())
}

fn balance_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "balance"), token.clone())
}

fn is_zero_address(env: &Env, address: &Address) -> bool {
    let zero_account = Address::from_string(&soroban_sdk::String::from_str(env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));
    if address == &zero_account {
        return true;
    }
    let zero_contract = Address::from_string(&soroban_sdk::String::from_str(env, "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4"));
    if address == &zero_contract {
        return true;
    }
    false
}

fn validate_treasury_address(env: &Env, address: &Address) -> Result<(), TreasuryError> {
    if is_zero_address(env, address) {
        return Err(TreasuryError::InvalidTreasuryAddress);
    }
    if address == &env.current_contract_address() {
        return Err(TreasuryError::InvalidTreasuryAddress);
    }
    Ok(())
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialises the treasury with its admin. May only be called once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), TreasuryError> {
        validate_treasury_address(&env, &admin)?;
        if read_admin(&env).is_some() {
            return Err(TreasuryError::AlreadyInitialized);
        }
        
        // Validate that admin is not a burn address
        validate_treasury_address(&env, &admin).expect("invalid admin address");
        
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &admin);
        Ok(())
    }

    /// Returns the current admin address, or `None` before initialisation.
    pub fn get_admin(env: Env) -> Result<Option<Address>, TreasuryError> {
        Ok(read_admin(&env))
    }

    /// Records `new_admin` as the pending admin. Only the current admin may call this.
    ///
    /// The transfer only takes effect once `new_admin` calls `accept_admin`.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        validate_treasury_address(&env, &new_admin)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, PENDING_ADMIN_KEY), &new_admin);
        env.events().publish(
            (Symbol::new(&env, "AdminTransferProposed"),),
            new_admin,
        );
        Ok(())
    }

    /// Accepts the pending admin role. Must be called by the pending admin itself.
    pub fn accept_admin(env: Env, accepted_by: Address) -> Result<(), TreasuryError> {
        accepted_by.require_auth();
        validate_treasury_address(&env, &accepted_by)?;
        let pending = read_pending_admin(&env).ok_or(TreasuryError::NoPendingAdmin)?;
        if accepted_by != pending {
            return Err(TreasuryError::NotPendingAdmin);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &accepted_by);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        env.events().publish(
            (Symbol::new(&env, "AdminTransferAccepted"),),
            accepted_by,
        );
        Ok(())
    }

    /// Immediately transfers the admin role. Only the current admin may call this.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        validate_treasury_address(&env, &new_admin)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
        // Clear any pending admin to prevent old proposals from accidentally succeeding.
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        Ok(())
    }

    /// Sets or updates the treasury address. Only admin may call this.
    ///
    /// Rejects the zero/burn address and the treasury contract's own address
    /// (to prevent self-referential transfers).
    pub fn set_treasury(env: Env, treasury: Address) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        if is_zero_address(&env, &treasury) {
            return Err(TreasuryError::InvalidTreasuryAddress);
        }
        if treasury == env.current_contract_address() {
            return Err(TreasuryError::InvalidTreasuryAddress);
        }
        validate_treasury_address(&env, &treasury)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "treasury"), &treasury);
        Ok(())
    }

    /// Credits `amount` of `token` to the treasury's internal accounting balance.
    ///
    /// This only updates the recorded balance; the tokens must already be held by the
    /// contract for [`Self::withdraw_treasury`] to move them out.
    pub fn deposit(env: Env, token: Address, amount: i128) -> Result<(), TreasuryError> {
        if amount < 0 {
            return Err(TreasuryError::InvalidAmount);
        }
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let updated = current.checked_add(amount).ok_or(TreasuryError::Overflow)?;
        env.storage()
            .persistent()
            .set(&key, &updated);
        Ok(())
    }

    /// Returns the recorded balance of `token` held by the treasury.
    pub fn get_balance(env: Env, token: Address) -> Result<i128, TreasuryError> {
        let key = balance_key(&env, &token);
        Ok(env.storage().persistent().get(&key).unwrap_or(0))
    }

    /// Transfers `amount` of `token` from the treasury to `destination`.
    ///
    /// Only the admin may call this. Returns an error when `amount` exceeds the recorded balance.
    pub fn withdraw_treasury(env: Env, token: Address, amount: i128, destination: Address) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        if amount < 0 {
            return Err(TreasuryError::InvalidAmount);
        }
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current {
            return Err(TreasuryError::InsufficientBalance);
        }
        env.storage()
            .persistent()
            .set(&key, &(current - amount));
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &destination,
            &amount,
        );
        env.events().publish(
            (Symbol::new(&env, "TreasuryWithdrawn"),),
            (token, amount, destination),
        );
        Ok(())
    }

    /// Withdraws the entire recorded balance of `token` to `destination`.
    ///
    /// Only the admin may call this. Returns the amount withdrawn (`0` if the treasury
    /// held nothing), in which case no transfer is performed.
    pub fn withdraw_all(env: Env, token: Address, destination: Address) -> Result<i128, TreasuryError> {
        check_admin(&env)?;
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if current > 0 {
            env.storage().persistent().remove(&key);
            token::Client::new(&env, &token).transfer(
                &env.current_contract_address(),
                &destination,
                &current,
            );
            env.events().publish(
                (Symbol::new(&env, "TreasuryWithdrawn"),),
                (token, current, destination),
            );
        }
        Ok(current)
    }

    /// Sets the LP reward pool address. Only admin may call this.
    ///
    /// Rejects the zero/burn address and the treasury contract's own address
    /// (a self-referential split would route LP rewards back into the treasury).
    pub fn set_lp_pool(env: Env, lp_pool: Address) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        if is_zero_address(&env, &lp_pool) {
            return Err(TreasuryError::InvalidLpPoolAddress);
        }
        if lp_pool == env.current_contract_address() {
            return Err(TreasuryError::LpPoolIsTreasuryContract);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, LP_POOL_KEY), &lp_pool);
        Ok(())
    }

    /// Clears the configured LP reward pool address, reverting to a treasury-only
    /// configuration. Only admin may call this.
    ///
    /// Succeeds even when no LP pool is currently configured.
    pub fn clear_lp_pool(env: Env) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, LP_POOL_KEY));
        env.events().publish(
            (Symbol::new(&env, "LpPoolCleared"),),
            (),
        );
        Ok(())
    }

    /// Returns the LP reward pool address, if set.
    pub fn get_lp_pool(env: Env) -> Result<Option<Address>, TreasuryError> {
        Ok(read_lp_pool(&env))
    }

    /// Sets the treasury-to-LP split in basis points.
    ///
    /// `treasury_bps` is the fraction (in bps, 0–10_000) that goes to the treasury.
    /// The remaining `10_000 - treasury_bps` goes to the LP reward pool.
    /// Only admin may call this.
    pub fn set_treasury_split(env: Env, treasury_bps: u32) -> Result<(), TreasuryError> {
        check_admin(&env)?;
        if treasury_bps > 10_000 {
            return Err(TreasuryError::InvalidTreasurySplit);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, TREASURY_SPLIT_BPS_KEY), &treasury_bps);
        env.events().publish(
            (Symbol::new(&env, "TreasurySplitSet"),),
            treasury_bps,
        );
        Ok(())
    }

    /// Returns the configured treasury split in basis points.
    pub fn get_treasury_split(env: Env) -> Result<u32, TreasuryError> {
        Ok(read_treasury_split_bps(&env))
    }

    /// Distributes all accumulated fees for `token` between the treasury wallet
    /// (`destination`) and the LP reward pool according to the configured split.
    ///
    /// - `treasury_bps` of `total` goes to `destination`.
    /// - The remainder goes to the LP pool (must be configured via `set_lp_pool`).
    ///
    /// Emits `FeeDistributed` with `(token, treasury_amount, lp_amount)`.
    pub fn distribute(env: Env, token: Address, destination: Address) -> Result<(i128, i128), TreasuryError> {
        check_admin(&env)?;
        let key = balance_key(&env, &token);
        let total: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if total == 0 {
            return Ok((0, 0));
        }
        if total < 0 {
            return Err(TreasuryError::InvalidAmount);
        }

        let treasury_bps = read_treasury_split_bps(&env);
        let treasury_amount = total
            .checked_mul(treasury_bps as i128)
            .and_then(|amount| amount.checked_div(10_000))
            .expect("treasury distribution overflow");
        let lp_amount = total - treasury_amount;

        let lp_pool = if lp_amount > 0 {
            Some(read_lp_pool(&env).ok_or(TreasuryError::LpPoolNotConfigured)?)
        } else {
            None
        };

        // Clear the balance before transfers (checks-effects-interactions)
        env.storage().persistent().remove(&key);

        let token_client = token::Client::new(&env, &token);

        if treasury_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &destination,
                &treasury_amount,
            );
        }
        if let Some(lp_pool) = lp_pool {
            token_client.transfer(
                &env.current_contract_address(),
                &lp_pool,
                &lp_amount,
            );
        }

        env.events().publish(
            (Symbol::new(&env, "FeeDistributed"),),
            (token, treasury_amount, lp_amount),
        );

        Ok((treasury_amount, lp_amount))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events},
        token::{Client as TokenClient, StellarAssetClient},
        Address, Env, IntoVal,
    };

    struct TreasuryTest {
        env: Env,
        treasury_id: Address,
        token_id: Address,
        admin: Address,
        user: Address,
    }

    fn setup() -> TreasuryTest {
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();

        let treasury_id = env.register(TreasuryContract, ());
        let token_admin = Address::generate(&env);
        let token_id = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        TreasuryTest {
            env,
            treasury_id,
            token_id,
            admin,
            user,
        }
    }

    fn withdrawal_events(env: &Env) -> std::vec::Vec<(Symbol, soroban_sdk::Val)> {
        env.events().all().iter().filter_map(|(_, topics, data)| {
            let name: Symbol = topics.get(0)?.into_val(env);
            (name == Symbol::new(env, "TreasuryWithdrawn")).then(|| (name, data.clone()))
        }).collect()
    }

    #[test]
    fn test_initialize_and_get_admin() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);

        assert!(c.get_admin().is_none());
        c.initialize(&t.admin);
        assert_eq!(c.get_admin(), Some(t.admin.clone()));
    }

    #[test]
    fn test_propose_and_accept_admin() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let new_admin = Address::generate(&t.env);
        c.propose_admin(&new_admin);
        assert_eq!(c.get_admin(), Some(t.admin.clone()));

        c.accept_admin(&new_admin);
        assert_eq!(c.get_admin(), Some(new_admin));
    }

    #[test]
    fn test_accept_admin_fails_if_not_pending() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let other = Address::generate(&t.env);
        let result = c.try_accept_admin(&other);
        assert!(result.is_err());
    }

    #[test]
    fn test_accept_admin_fails_if_wrong_address() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let new_admin = Address::generate(&t.env);
        let wrong_admin = Address::generate(&t.env);
        c.propose_admin(&new_admin);

        let result = c.try_accept_admin(&wrong_admin);
        assert!(result.is_err());
    }

    #[test]
    fn test_propose_admin_only_by_current_admin() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // Only authorize the non-admin caller, so the admin's `require_auth`
        // inside `check_admin` cannot be satisfied.
        let non_admin = Address::generate(&t.env);
        let new_admin = Address::generate(&t.env);
        t.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &non_admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &t.treasury_id,
                fn_name: "propose_admin",
                args: (new_admin.clone(),).into_val(&t.env),
                sub_invokes: &[],
            },
        }]);

        let result = c.try_propose_admin(&new_admin);
        assert!(result.is_err());
    }

    #[test]
    fn test_deposit_and_get_balance() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        assert_eq!(c.get_balance(&t.token_id), 0);

        c.deposit(&t.token_id, &1000);
        assert_eq!(c.get_balance(&t.token_id), 1000);

        c.deposit(&t.token_id, &500);
        assert_eq!(c.get_balance(&t.token_id), 1500);
    }

    #[test]
    fn test_distribute_rejects_overflowing_treasury_calculation() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        c.deposit(&t.token_id, &i128::MAX);

        assert!(c.try_distribute(&t.token_id, &t.user).is_err());
    }

    #[test]
    fn test_withdraw_treasury() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // Mint tokens to treasury
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        let initial_user = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(initial_user, 0);

        c.withdraw_treasury(&t.token_id, &3000, &t.user);

        let events = withdrawal_events(&t.env);
        assert_eq!(events.len(), 1);
        let payload: (Address, i128, Address) = events[0].1.clone().into_val(&t.env);
        assert_eq!(payload, (t.token_id.clone(), 3000, t.user.clone()));

        let user_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(user_balance, 3000);
        assert_eq!(c.get_balance(&t.token_id), 7000);
    }

    #[test]
    fn test_withdraw_treasury_zero_amount() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        let treasury_balance_before = c.get_balance(&t.token_id);
        let user_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(treasury_balance_before, 10_000);
        assert_eq!(user_balance_before, 0);

        // A zero-amount withdrawal is a no-op: it must not fail or alter balances.
        c.withdraw_treasury(&t.token_id, &0, &t.user);

        let events = withdrawal_events(&t.env);
        assert_eq!(events.len(), 1);
        let payload: (Address, i128, Address) = events[0].1.clone().into_val(&t.env);
        assert_eq!(payload, (t.token_id.clone(), 0, t.user.clone()));

        assert_eq!(c.get_balance(&t.token_id), treasury_balance_before);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.user),
            user_balance_before
        );
    }

    #[test]
    fn test_withdraw_treasury_exact_balance() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        // Full withdrawal via the non-withdraw_all path: amount == balance.
        c.withdraw_treasury(&t.token_id, &10_000, &t.user);

        let user_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(user_balance, 10_000);
        assert_eq!(c.get_balance(&t.token_id), 0);
    }

    #[test]
    fn test_withdraw_all() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // Mint tokens to treasury first
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        let withdrawn = c.withdraw_all(&t.token_id, &t.user);
        assert_eq!(withdrawn, 10_000);

        let events = withdrawal_events(&t.env);
        assert_eq!(events.len(), 1);
        let payload: (Address, i128, Address) = events[0].1.clone().into_val(&t.env);
        assert_eq!(payload, (t.token_id.clone(), 10_000, t.user.clone()));

        let user_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(user_balance, 10_000);
        assert_eq!(c.get_balance(&t.token_id), 0);
    }

    #[test]
    fn test_withdraw_all_zero_balance_emits_no_withdrawal_event() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        assert_eq!(c.withdraw_all(&t.token_id, &t.user), 0);
        assert!(withdrawal_events(&t.env).is_empty());
    }

    #[test]
    fn test_initialize_zero_address_rejected() {
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();
        let treasury_id = env.register(TreasuryContract, ());
        let c = TreasuryContractClient::new(&env, &treasury_id);
        let zero_addr = Address::from_string(&soroban_sdk::String::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));
        let result = c.try_initialize(&zero_addr);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_set_treasury_zero_address_rejected() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        let zero_addr = Address::from_string(&soroban_sdk::String::from_str(&t.env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));
        let result = c.try_set_treasury(&zero_addr);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_initialize_self_address_rejected() {
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();
        let treasury_id = env.register(TreasuryContract, ());
        let c = TreasuryContractClient::new(&env, &treasury_id);
        let result = c.try_initialize(&treasury_id);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_propose_admin_self_address_rejected() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        let result = c.try_propose_admin(&t.treasury_id);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_accept_admin_self_address_rejected() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        let result = c.try_accept_admin(&t.treasury_id);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_set_treasury_self_address_rejected() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        let result = c.try_set_treasury(&t.treasury_id);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasuryAddress)));
    }

    #[test]
    fn test_set_treasury_succeeds_with_valid_address() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let treasury_wallet = Address::generate(&t.env);
        let result = c.try_set_treasury(&treasury_wallet);
        assert_eq!(result, Ok(Ok(())));
    }

    #[test]
    fn test_withdraw_treasury_returns_typed_insufficient_balance_error() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let result = c.try_withdraw_treasury(&t.token_id, &1, &t.user);
        assert_eq!(result, Err(Ok(TreasuryError::InsufficientBalance)));
    }

    #[test]
    fn test_distribute_returns_typed_missing_lp_pool_error() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &100);
        c.deposit(&t.token_id, &100);
        c.set_treasury_split(&5000);

        let result = c.try_distribute(&t.token_id, &t.user);
        assert_eq!(result, Err(Ok(TreasuryError::LpPoolNotConfigured)));
    }

    #[test]
    fn test_set_treasury_split_returns_typed_error() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let result = c.try_set_treasury_split(&10_001);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidTreasurySplit)));
    }

    #[test]
    fn test_set_lp_pool_zero_address_returns_typed_error() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        let zero_addr = Address::from_string(&soroban_sdk::String::from_str(&t.env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));

        let result = c.try_set_lp_pool(&zero_addr);
        assert_eq!(result, Err(Ok(TreasuryError::InvalidLpPoolAddress)));
    }

    #[test]
    fn test_set_lp_pool_self_reference_returns_typed_error() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let result = c.try_set_lp_pool(&t.treasury_id);
        assert_eq!(result, Err(Ok(TreasuryError::LpPoolIsTreasuryContract)));
    }

    #[test]
    fn test_set_lp_pool_succeeds_with_valid_address() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let lp_pool = Address::generate(&t.env);
        c.set_lp_pool(&lp_pool);
        assert_eq!(c.get_lp_pool(), Some(lp_pool));
    }

    #[test]
    fn test_distribute_succeeds_without_lp_pool_when_treasury_bps_is_10000() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);
        c.set_treasury_split(&10_000u32);

        assert_eq!(c.get_lp_pool(), None);

        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        // lp_amount is 0, so read_lp_pool is never reached and no lp_pool is required.
        let (treasury_amount, lp_amount) = c.distribute(&t.token_id, &t.admin);
        assert_eq!(treasury_amount, 10_000);
        assert_eq!(lp_amount, 0);
        assert_eq!(c.get_balance(&t.token_id), 0);
        assert!(c.get_lp_pool().is_none());
    }

    #[test]
    fn test_set_admin_clears_pending_admin() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        let pending_admin = Address::generate(&t.env);
        let new_admin = Address::generate(&t.env);

        // Propose a pending admin
        c.propose_admin(&pending_admin);
        assert_eq!(c.get_admin(), Some(t.admin.clone()));

        // Use set_admin to immediately transfer to a different admin
        c.set_admin(&new_admin);

        // Verify the new admin is set and pending admin is cleared
        assert_eq!(c.get_admin(), Some(new_admin.clone()));

        // Verify that the old pending admin can no longer accept
        let result = c.try_accept_admin(&pending_admin);
        assert_eq!(result, Err(Ok(TreasuryError::NoPendingAdmin)));
    }

    #[test]
    fn test_withdraw_all_fails_when_recorded_balance_exceeds_actual_token_balance() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // Actual on-chain balance transferred/minted to the contract is only 50
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &50);

        // But internal accounting records a deposit of 100, causing get_balance to diverge
        c.deposit(&t.token_id, &100);
        assert_eq!(c.get_balance(&t.token_id), 100);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            50
        );

        // Attempting to withdraw_all attempts to transfer the full recorded balance (100) via token::Client.
        // Because the contract holds only 50 tokens, the token::Client.transfer call fails
        // and rolls back rather than silently succeeding with insufficient real funds.
        let result = c.try_withdraw_all(&t.token_id, &t.user);
        assert!(result.is_err());

        // Internal accounting balance is rolled back and remains intact
        assert_eq!(c.get_balance(&t.token_id), 100);
        // On-chain funds were not moved
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            50
        );
        assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.user), 0);
        // No withdrawal events were published
        assert!(withdrawal_events(&t.env).is_empty());
    }

    #[test]
    #[should_panic]
    fn test_withdraw_all_panics_when_recorded_balance_exceeds_actual_token_balance() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &50);
        c.deposit(&t.token_id, &100);

        // Direct call panics when the underlying token transfer fails
        c.withdraw_all(&t.token_id, &t.user);
    }

    #[test]
    fn test_withdraw_all_fails_when_contract_holds_zero_real_tokens() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // No tokens were minted or transferred to the treasury contract (real balance = 0).
        // Internal accounting records a deposit of 100.
        c.deposit(&t.token_id, &100);
        assert_eq!(c.get_balance(&t.token_id), 100);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            0
        );

        let result = c.try_withdraw_all(&t.token_id, &t.user);
        assert!(result.is_err());

        assert_eq!(c.get_balance(&t.token_id), 100);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            0
        );
        assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.user), 0);
        assert!(withdrawal_events(&t.env).is_empty());
    }

    #[test]
    fn test_withdraw_treasury_fails_when_recorded_balance_exceeds_actual_token_balance() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        // Actual on-chain balance held is 50
        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &50);

        // Internal accounting ledger recorded is 100
        c.deposit(&t.token_id, &100);
        assert_eq!(c.get_balance(&t.token_id), 100);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            50
        );

        // Amount requested is 75, which is <= recorded balance (100) so treasury internal
        // check passes, but > real on-chain balance (50), so token::Client.transfer fails.
        let result = c.try_withdraw_treasury(&t.token_id, &75, &t.user);
        assert!(result.is_err());

        // State is rolled back
        assert_eq!(c.get_balance(&t.token_id), 100);
        assert_eq!(
            TokenClient::new(&t.env, &t.token_id).balance(&t.treasury_id),
            50
        );
        assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.user), 0);
        assert!(withdrawal_events(&t.env).is_empty());
    }

    #[test]
    fn test_set_lp_pool_reconfigured_after_distribution_routes_to_new_pool() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        let token = TokenClient::new(&t.env, &t.token_id);
        let minter = StellarAssetClient::new(&t.env, &t.token_id);
        c.initialize(&t.admin);
        c.set_treasury_split(&5000u32);

        // First distribution goes to the original LP pool.
        let old_pool = Address::generate(&t.env);
        c.set_lp_pool(&old_pool);
        assert_eq!(c.get_lp_pool(), Some(old_pool.clone()));

        minter.mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);
        let (treasury_amount, lp_amount) = c.distribute(&t.token_id, &t.user);
        assert_eq!((treasury_amount, lp_amount), (5_000, 5_000));
        assert_eq!(token.balance(&old_pool), 5_000);
        assert_eq!(token.balance(&t.user), 5_000);
        assert_eq!(c.get_balance(&t.token_id), 0);

        // Reconfigure the LP pool after fees were already distributed.
        let new_pool = Address::generate(&t.env);
        c.set_lp_pool(&new_pool);
        assert_eq!(c.get_lp_pool(), Some(new_pool.clone()));

        // Second distribution must route the LP share to the new pool only.
        minter.mint(&t.treasury_id, &4_000);
        c.deposit(&t.token_id, &4_000);
        let (treasury_amount, lp_amount) = c.distribute(&t.token_id, &t.user);
        assert_eq!((treasury_amount, lp_amount), (2_000, 2_000));
        assert_eq!(token.balance(&new_pool), 2_000);
        assert_eq!(token.balance(&t.user), 7_000);

        // The old pool received nothing further; no residual routing to it remains.
        assert_eq!(token.balance(&old_pool), 5_000);
        assert_ne!(c.get_lp_pool(), Some(old_pool));
        assert_eq!(c.get_balance(&t.token_id), 0);
        assert_eq!(token.balance(&t.treasury_id), 0);
    }
}
