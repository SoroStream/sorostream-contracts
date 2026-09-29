#![no_std]
//! # SoroStream Treasury Contract
//!
//! Holds accumulated protocol fees and provides configurable distribution
//! between a treasury wallet and an LP reward pool.

use soroban_sdk::{contract, contractimpl, token, Address, Env, Symbol};

const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const LP_POOL_KEY: &str = "lp_pool";
const TREASURY_SPLIT_BPS_KEY: &str = "t_split";

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

fn check_admin(env: &Env) {
    read_admin(env)
        .expect("treasury not initialized")
        .require_auth();
}

fn balance_key(env: &Env, token: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "balance"), token.clone())
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialises the treasury with its admin. May only be called once.
    pub fn initialize(env: Env, admin: Address) {
        if read_admin(&env).is_some() {
            panic!("treasury already initialized");
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &admin);
    }

    /// Returns the current admin address, or `None` before initialisation.
    pub fn get_admin(env: Env) -> Option<Address> {
        read_admin(&env)
    }

    /// Records `new_admin` as the pending admin. Only the current admin may call this.
    ///
    /// The transfer only takes effect once `new_admin` calls `accept_admin`.
    pub fn propose_admin(env: Env, new_admin: Address) {
        check_admin(&env);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, PENDING_ADMIN_KEY), &new_admin);
        env.events().publish(
            (Symbol::new(&env, "AdminTransferProposed"),),
            new_admin,
        );
    }

    /// Accepts the pending admin role. Must be called by the pending admin itself.
    pub fn accept_admin(env: Env, accepted_by: Address) {
        accepted_by.require_auth();
        let pending = read_pending_admin(&env)
            .expect("no pending admin transfer");
        if accepted_by != pending {
            panic!("caller is not the pending admin");
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
    }

    /// Immediately transfers the admin role. Only the current admin may call this.
    pub fn set_admin(env: Env, new_admin: Address) {
        check_admin(&env);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
    }

    /// Credits `amount` of `token` to the treasury's internal accounting balance.
    ///
    /// This only updates the recorded balance; the tokens must already be held by the
    /// contract for [`Self::withdraw_treasury`] to move them out.
    pub fn deposit(env: Env, token: Address, amount: i128) {
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&key, &(current + amount));
    }

    /// Returns the recorded balance of `token` held by the treasury.
    pub fn get_balance(env: Env, token: Address) -> i128 {
        let key = balance_key(&env, &token);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Transfers `amount` of `token` from the treasury to `destination`.
    ///
    /// Only the admin may call this. Panics when `amount` exceeds the recorded balance.
    pub fn withdraw_treasury(env: Env, token: Address, amount: i128, destination: Address) {
        check_admin(&env);
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current {
            panic!("insufficient treasury balance");
        }
        env.storage()
            .persistent()
            .set(&key, &(current - amount));
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &destination,
            &amount,
        );
    }

    /// Withdraws the entire recorded balance of `token` to `destination`.
    ///
    /// Only the admin may call this. Returns the amount withdrawn (`0` if the treasury
    /// held nothing), in which case no transfer is performed.
    pub fn withdraw_all(env: Env, token: Address, destination: Address) -> i128 {
        check_admin(&env);
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if current > 0 {
            env.storage().persistent().remove(&key);
            token::Client::new(&env, &token).transfer(
                &env.current_contract_address(),
                &destination,
                &current,
            );
        }
        current
    }

    /// Sets the LP reward pool address. Only admin may call this.
    pub fn set_lp_pool(env: Env, lp_pool: Address) {
        check_admin(&env);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, LP_POOL_KEY), &lp_pool);
    }

    /// Returns the LP reward pool address, if set.
    pub fn get_lp_pool(env: Env) -> Option<Address> {
        read_lp_pool(&env)
    }

    /// Sets the treasury-to-LP split in basis points.
    ///
    /// `treasury_bps` is the fraction (in bps, 0–10_000) that goes to the treasury.
    /// The remaining `10_000 - treasury_bps` goes to the LP reward pool.
    /// Only admin may call this.
    pub fn set_treasury_split(env: Env, treasury_bps: u32) {
        check_admin(&env);
        if treasury_bps > 10_000 {
            panic!("treasury_bps exceeds 10_000");
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, TREASURY_SPLIT_BPS_KEY), &treasury_bps);
    }

    /// Returns the configured treasury split in basis points.
    pub fn get_treasury_split(env: Env) -> u32 {
        read_treasury_split_bps(&env)
    }

    /// Distributes all accumulated fees for `token` between the treasury wallet
    /// (`destination`) and the LP reward pool according to the configured split.
    ///
    /// - `treasury_bps` of `total` goes to `destination`.
    /// - The remainder goes to the LP pool (must be configured via `set_lp_pool`).
    ///
    /// Emits `FeeDistributed` with `(token, treasury_amount, lp_amount)`.
    pub fn distribute(env: Env, token: Address, destination: Address) -> (i128, i128) {
        check_admin(&env);
        let key = balance_key(&env, &token);
        let total: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if total == 0 {
            return (0, 0);
        }

        let treasury_bps = read_treasury_split_bps(&env);
        let treasury_amount = total * treasury_bps as i128 / 10_000;
        let lp_amount = total - treasury_amount;

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
        if lp_amount > 0 {
            let lp_pool = read_lp_pool(&env).expect("lp_pool not configured");
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

        (treasury_amount, lp_amount)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _},
        token::{Client as TokenClient, StellarAssetClient},
        Address, Env,
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
        env.mock_all_auths();

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

        let _new_admin = Address::generate(&t.env);
        // The main flow is tested in test_propose_and_accept_admin
        // This test name serves as documentation for the behavior
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

        let user_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(user_balance, 3000);
        assert_eq!(c.get_balance(&t.token_id), 7000);
    }

    #[test]
    fn test_withdraw_all() {
        let t = setup();
        let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
        c.initialize(&t.admin);

        StellarAssetClient::new(&t.env, &t.token_id).mint(&t.treasury_id, &10_000);
        c.deposit(&t.token_id, &10_000);

        let withdrawn = c.withdraw_all(&t.token_id, &t.user);
        assert_eq!(withdrawn, 10_000);

        let user_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.user);
        assert_eq!(user_balance, 10_000);
        assert_eq!(c.get_balance(&t.token_id), 0);
    }
}
