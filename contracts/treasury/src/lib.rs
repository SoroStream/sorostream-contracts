#![no_std]

use soroban_sdk::{contract, contractimpl, token, Address, Env, Symbol};

const ADMIN_KEY: &str = "admin";

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
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
    pub fn initialize(env: Env, admin: Address) {
        if read_admin(&env).is_some() {
            panic!("treasury already initialized");
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &admin);
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        read_admin(&env)
    }

    pub fn set_admin(env: Env, new_admin: Address) {
        check_admin(&env);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
    }

    pub fn deposit(env: Env, token: Address, amount: i128) {
        let key = balance_key(&env, &token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&key, &(current + amount));
    }

    pub fn get_balance(env: Env, token: Address) -> i128 {
        let key = balance_key(&env, &token);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

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
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
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
