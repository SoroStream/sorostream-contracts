use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

struct TestEnv {
    env: Env,
    super_admin: Address,
    fee_admin: Address,
    emergency_admin: Address,
    token_admin: Address,
    contract_id: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let super_admin = Address::generate(&env);
    let fee_admin = Address::generate(&env);
    let emergency_admin = Address::generate(&env);
    let token_admin = Address::generate(&env);

    SoroStreamContractClient::new(&env, &contract_id).initialize_roles(
        &super_admin,
        &soroban_sdk::String::from_str(&env, "1.0.0"),
        &Some(fee_admin.clone()),
        &Some(emergency_admin.clone()),
        &Some(token_admin.clone()),
    );

    TestEnv {
        env,
        super_admin,
        fee_admin,
        emergency_admin,
        token_admin,
        contract_id,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #623: Admin Role Separation Tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_roles_set_independently_at_deployment() {
    let t = setup();
    let c = client(&t);

    assert_eq!(c.get_admin(), t.super_admin);
    assert_eq!(c.get_fee_admin(), Some(t.fee_admin));
    assert_eq!(c.get_emergency_admin(), Some(t.emergency_admin));
    assert_eq!(c.get_token_admin(), Some(t.token_admin));
}

#[test]
fn test_fee_admin_cannot_trigger_emergency_pause() {
    let t = setup();
    let c = client(&t);

    // Fee admin attempts to trigger role emergency pause -> must fail with NotAuthorized
    let result = c.try_role_emergency_pause(&t.fee_admin);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));

    // Emergency admin can trigger role emergency pause -> must succeed
    c.role_emergency_pause(&t.emergency_admin);
    assert!(c.is_paused());
}

#[test]
fn test_roles_can_be_assigned_and_revoked_by_super_admin() {
    let t = setup();
    let c = client(&t);

    let new_fee_admin = Address::generate(&t.env);
    c.set_fee_admin(&t.super_admin, &new_fee_admin);
    assert_eq!(c.get_fee_admin(), Some(new_fee_admin));

    let new_emergency_admin = Address::generate(&t.env);
    c.set_emergency_admin(&t.super_admin, &new_emergency_admin);
    assert_eq!(c.get_emergency_admin(), Some(new_emergency_admin));

    let new_token_admin = Address::generate(&t.env);
    c.set_token_admin(&t.super_admin, &new_token_admin);
    assert_eq!(c.get_token_admin(), Some(new_token_admin));
}

#[test]
fn test_roles_transfer_atomically_and_require_expected_current_holder() {
    let t = setup();
    let c = client(&t);
    let new_fee_manager = Address::generate(&t.env);
    let new_emergency_admin = Address::generate(&t.env);
    let new_token_admin = Address::generate(&t.env);
    let analytics_holder = Address::generate(&t.env);
    let new_analytics_admin = Address::generate(&t.env);
    c.assign_analytics_role(&t.super_admin, &analytics_holder);

    let wrong_old_holder = Address::generate(&t.env);
    assert_eq!(
        c.try_transfer_fee_manager_role(&t.super_admin, &wrong_old_holder, &new_fee_manager),
        Err(Ok(StreamError::NotAuthorized))
    );
    assert_eq!(c.get_fee_admin(), Some(t.fee_admin.clone()));

    c.transfer_fee_manager_role(&t.super_admin, &t.fee_admin, &new_fee_manager);
    let events = t.env.events().all();
    let transfer_events: std::vec::Vec<_> = events
        .iter()
        .filter(|(_, topics, _)| {
            !topics.is_empty()
                && soroban_sdk::Symbol::new(&t.env, "RoleTransferred")
                    == topics.get(0).unwrap().into_val(&t.env)
        })
        .collect();
    assert_eq!(transfer_events.len(), 1);

    c.transfer_emergency_pause_role(&t.super_admin, &t.emergency_admin, &new_emergency_admin);
    c.transfer_token_admin_role(&t.super_admin, &t.token_admin, &new_token_admin);
    c.transfer_analytics_role(&t.super_admin, &analytics_holder, &new_analytics_admin);

    assert_eq!(c.get_fee_admin(), Some(new_fee_manager));
    assert_eq!(c.get_emergency_admin(), Some(new_emergency_admin));
    assert_eq!(c.get_token_admin(), Some(new_token_admin));
    assert_eq!(c.get_analytics_role(), Some(new_analytics_admin));
}
