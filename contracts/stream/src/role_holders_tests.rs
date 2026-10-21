extern crate std;

use crate::{RoleHolders, SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);

    let client = SoroStreamContractClient::new(&env, &contract);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    client.set_min_duration(&admin, &0u64);

    (env, contract, admin)
}

fn client<'a>(env: &'a Env, contract: &'a Address) -> SoroStreamContractClient<'a> {
    SoroStreamContractClient::new(env, contract)
}

#[test]
fn test_get_role_holders_returns_all_unassigned_except_super_admin() {
    let (env, contract, admin) = setup();
    let c = client(&env, &contract);

    let holders = c.get_role_holders();

    assert_eq!(holders.super_admin, Some(admin));
    assert_eq!(holders.fee_manager, None);
    assert_eq!(holders.emergency_pause, None);
    assert_eq!(holders.analytics, None);
    assert_eq!(holders.token_admin, None);
}

#[test]
fn test_get_role_holders_aggregates_all_assigned_roles() {
    let (env, contract, admin) = setup();
    let c = client(&env, &contract);

    let fee_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let analytics = Address::generate(&env);
    let token_admin = Address::generate(&env);

    c.assign_fee_manager(&admin, &fee_manager);
    c.assign_emergency_pause_role(&admin, &emergency);
    c.assign_analytics_role(&admin, &analytics);
    c.assign_token_admin(&admin, &token_admin);

    let holders = c.get_role_holders();

    assert_eq!(holders.super_admin, Some(admin));
    assert_eq!(holders.fee_manager, Some(fee_manager));
    assert_eq!(holders.emergency_pause, Some(emergency));
    assert_eq!(holders.analytics, Some(analytics));
    assert_eq!(holders.token_admin, Some(token_admin));
}
