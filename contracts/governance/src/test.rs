#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn default_token(env: &Env) -> Address {
    Address::generate(env)
}

#[test]
fn test_set_guardian_single_step_still_works() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, 0, 0, 0, 0);

    let new_guardian = Address::generate(&env);
    c.set_guardian(&new_guardian);

    let stored: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY));
    assert_eq!(stored, Some(new_guardian));
}

#[test]
fn test_guardian_two_step_transfer() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, 0, 0, 0, 0);

    let new_guardian = Address::generate(&env);

    // No pending guardian before proposal.
    let pending: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY));
    assert!(pending.is_none());

    // Admin proposes the new guardian.
    c.propose_guardian(&new_guardian);

    let pending: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY));
    assert_eq!(pending, Some(new_guardian.clone()));

    // Guardian is not yet switched.
    let stored: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY));
    assert_eq!(stored, Some(guardian.clone()));

    // The pending guardian accepts, completing the two-step transfer.
    c.accept_guardian(&new_guardian);

    let stored: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY));
    assert_eq!(stored, Some(new_guardian));

    // Pending is cleared after acceptance.
    let pending: Option<Address> = env
        .storage()
        .instance()
        .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY));
    assert!(pending.is_none());
}

#[test]
fn test_accept_guardian_without_proposal_fails() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, 0, 0, 0, 0);

    let new_guardian = Address::generate(&env);
    let result = c.try_accept_guardian(&new_guardian);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::NotInitialized),
        _ => panic!("expected NotInitialized"),
    }
}

#[test]
fn test_accept_guardian_wrong_caller_fails() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, 0, 0, 0, 0);

    let new_guardian = Address::generate(&env);
    let impostor = Address::generate(&env);

    c.propose_guardian(&new_guardian);

    let result = c.try_accept_guardian(&impostor);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::NotAdmin),
        _ => panic!("expected NotAdmin"),
    }
}

#[test]
fn test_queue_quorum_boundary_evenly_split() {
    // Issue #824: Test boundary where total_votes exactly equals quorum
    // but votes_for equals votes_against (tie at exactly quorum threshold).
    // The queue() function requires: total_votes >= quorum AND votes_for > votes_against
    // This test ensures the proposal is Defeated, not accidentally passed.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);
    let quorum = 4u32;  // Set quorum to 4 votes

    c.initialize(&admin, &guardian, &token, quorum, 100, 0, 0);

    // Create a proposal: total_votes = 4 (exactly at quorum), split 2 for / 2 against
    let prop_id = c.propose(&admin, b"test proposal".to_vec());

    // Vote 2 for, 2 against (exactly equal, exactly at quorum)
    let voter1 = Address::generate(&env);
    let voter2 = Address::generate(&env);
    let voter3 = Address::generate(&env);
    let voter4 = Address::generate(&env);

    c.vote(&voter1, &prop_id, true);  // vote_for
    c.vote(&voter2, &prop_id, true);  // vote_for
    c.vote(&voter3, &prop_id, false); // vote_against
    c.vote(&voter4, &prop_id, false); // vote_against

    // Try to queue: should fail because votes_for (2) == votes_against (2), tie not >
    let result = c.try_queue(&admin, &prop_id);
    assert!(result.is_err(), "Tied proposal at quorum boundary should not queue");
}
