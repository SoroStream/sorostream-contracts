#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[test]
fn test_get_owners_returns_empty_before_initialize() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    assert_eq!(c.owner_count(), 0);
    assert_eq!(c.get_owners().len(), 0);
}

#[test]
fn test_get_owners_returns_all_owners() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let owner3 = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner1.clone(), owner2.clone(), owner3.clone()]);

    c.initialize(&owners, 2);

    assert_eq!(c.owner_count(), 3);
    assert_eq!(c.get_owners(), owners);
}

#[test]
fn test_get_owners_reflects_removal() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let owner3 = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner1.clone(), owner2.clone(), owner3.clone()]);

    c.initialize(&owners, 1);

    // Remove owner2 via the multi-sig itself (current contract auth).
    c.remove_owner(&owner2);

    let remaining = c.get_owners();
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining.get(0).unwrap(), owner1);
    assert_eq!(remaining.get(1).unwrap(), owner3);
}

#[test]
fn test_get_intents_by_proposer_and_target() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let other = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone(), other.clone()]);

    c.initialize(&owners, 1);

    let calldata = soroban_sdk::Bytes::new(&env);
    let id1 = c.propose(
        &proposer,
        &target_a,
        &soroban_sdk::symbol!("fn_a"),
        &calldata,
        1000,
    );
    let id2 = c.propose(
        &proposer,
        &target_b,
        &soroban_sdk::symbol!("fn_b"),
        &calldata,
        1000,
    );
    let id3 = c.propose(
        &other,
        &target_a,
        &soroban_sdk::symbol!("fn_a"),
        &calldata,
        1000,
    );

    // Proposer index.
    let by_proposer = c.get_intents_by_proposer(&proposer);
    assert_eq!(by_proposer.len(), 2);
    assert!(by_proposer.contains(&id1));
    assert!(by_proposer.contains(&id2));

    let by_other = c.get_intents_by_proposer(&other);
    assert_eq!(by_other.len(), 1);
    assert!(by_other.contains(&id3));

    // Target index.
    let by_target_a = c.get_intents_by_target(&target_a);
    assert_eq!(by_target_a.len(), 2);
    assert!(by_target_a.contains(&id1));
    assert!(by_target_a.contains(&id3));

    let by_target_b = c.get_intents_by_target(&target_b);
    assert_eq!(by_target_b.len(), 1);
    assert!(by_target_b.contains(&id2));

    // Unknown address yields an empty list.
    let unknown = Address::generate(&env);
    assert_eq!(c.get_intents_by_proposer(&unknown).len(), 0);
    assert_eq!(c.get_intents_by_target(&unknown).len(), 0);
}

#[test]
fn test_get_intent_returns_stored_fields() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, 1);

    let intent_id = c.propose(
        &proposer,
        &target,
        &soroban_sdk::symbol!("fn"),
        &soroban_sdk::Bytes::new(&env),
        1000,
    );

    let intent = c.get_intent(intent_id);
    assert_eq!(intent.id, intent_id);
    assert_eq!(intent.proposer, proposer);
    assert_eq!(intent.target, target);
    assert_eq!(intent.status, super::IntentStatus::Pending);
    assert_eq!(intent.approval_count, 0);
}
