#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec,
};

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

    c.initialize(&owners, &2u32);

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

    c.initialize(&owners, &1u32);

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

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    let id1 = c.propose(&proposer, &target_a, &soroban_sdk::symbol_short!("fn_a"), &calldata, &1000u64);
    let id2 = c.propose(&proposer, &target_b, &soroban_sdk::symbol_short!("fn_b"), &calldata, &1000u64);
    let id3 = c.propose(&other, &target_a, &soroban_sdk::symbol_short!("fn_a"), &calldata, &1000u64);

    // Proposer index.
    let by_proposer = c.get_intents_by_proposer(&proposer, &0u32, &10u32);
    assert_eq!(by_proposer.len(), 2);
    assert!(by_proposer.contains(&id1));
    assert!(by_proposer.contains(&id2));

    let by_other = c.get_intents_by_proposer(&other, &0u32, &10u32);
    assert_eq!(by_other.len(), 1);
    assert!(by_other.contains(&id3));

    // Target index.
    let by_target_a = c.get_intents_by_target(&target_a, &0u32, &10u32);
    assert_eq!(by_target_a.len(), 2);
    assert!(by_target_a.contains(&id1));
    assert!(by_target_a.contains(&id3));

    let by_target_b = c.get_intents_by_target(&target_b, &0u32, &10u32);
    assert_eq!(by_target_b.len(), 1);
    assert!(by_target_b.contains(&id2));

    // Unknown address yields an empty list.
    let unknown = Address::generate(&env);
    assert_eq!(c.get_intents_by_proposer(&unknown, &0u32, &10u32).len(), 0);
    assert_eq!(c.get_intents_by_target(&unknown, &0u32, &10u32).len(), 0);
}

#[test]
fn test_get_intents_by_proposer_pagination() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    let mut ids = Vec::new(&env);
    for _ in 0..5 {
        ids.push_back(c.propose(
            &proposer,
            &target,
            &soroban_sdk::symbol_short!("fn"),
            &calldata,
            &1000u64,
        ));
    }

    // Full page.
    let all = c.get_intents_by_proposer(&proposer, &0u32, &10u32);
    assert_eq!(all.len(), 5);
    for i in 0..5u32 {
        assert_eq!(all.get(i).unwrap(), ids.get(i).unwrap());
    }

    // Offset slice.
    let page = c.get_intents_by_proposer(&proposer, &1u32, &2u32);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), ids.get(1).unwrap());
    assert_eq!(page.get(1).unwrap(), ids.get(2).unwrap());

    // Tail slice.
    let tail = c.get_intents_by_proposer(&proposer, &3u32, &10u32);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail.get(0).unwrap(), ids.get(3).unwrap());
    assert_eq!(tail.get(1).unwrap(), ids.get(4).unwrap());
}

#[test]
fn test_get_intents_by_target_pagination() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    let mut ids = Vec::new(&env);
    for _ in 0..4 {
        ids.push_back(c.propose(
            &proposer,
            &target,
            &soroban_sdk::symbol_short!("fn"),
            &calldata,
            &1000u64,
        ));
    }

    let page = c.get_intents_by_target(&target, &1u32, &2u32);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), ids.get(1).unwrap());
    assert_eq!(page.get(1).unwrap(), ids.get(2).unwrap());
}

#[test]
fn test_get_intents_by_proposer_limit_clamped() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    let total = MAX_INTENT_PAGE_SIZE + 5;
    for _ in 0..total {
        c.propose(
            &proposer,
            &target,
            &soroban_sdk::symbol_short!("fn"),
            &calldata,
            &1000u64,
        );
    }

    // A limit larger than the cap is clamped to MAX_INTENT_PAGE_SIZE.
    let page = c.get_intents_by_proposer(&proposer, &0u32, &u32::MAX);
    assert_eq!(page.len(), MAX_INTENT_PAGE_SIZE);

    // The next page continues where the previous one left off.
    let next = c.get_intents_by_proposer(&proposer, &MAX_INTENT_PAGE_SIZE, &u32::MAX);
    assert_eq!(next.len(), 5);
}

#[test]
fn test_get_intents_by_target_limit_clamped() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    let total = MAX_INTENT_PAGE_SIZE + 3;
    for _ in 0..total {
        c.propose(
            &proposer,
            &target,
            &soroban_sdk::symbol_short!("fn"),
            &calldata,
            &1000u64,
        );
    }

    let page = c.get_intents_by_target(&target, &0u32, &u32::MAX);
    assert_eq!(page.len(), MAX_INTENT_PAGE_SIZE);

    let next = c.get_intents_by_target(&target, &MAX_INTENT_PAGE_SIZE, &u32::MAX);
    assert_eq!(next.len(), 3);
}

#[test]
fn test_get_intents_pagination_out_of_range_offset() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let calldata = soroban_sdk::Bytes::new(&env);
    c.propose(
        &proposer,
        &target,
        &soroban_sdk::symbol_short!("fn"),
        &calldata,
        &1000u64,
    );

    // Offset past the end returns an empty Vec.
    assert_eq!(c.get_intents_by_proposer(&proposer, &5u32, &10u32).len(), 0);
    assert_eq!(c.get_intents_by_target(&target, &5u32, &10u32).len(), 0);

    // Zero limit returns an empty Vec.
    assert_eq!(c.get_intents_by_proposer(&proposer, &0u32, &0u32).len(), 0);
    assert_eq!(c.get_intents_by_target(&target, &0u32, &0u32).len(), 0);
}

#[test]
fn test_get_intents_pagination_unknown_address() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let unknown = Address::generate(&env);
    assert_eq!(c.get_intents_by_proposer(&unknown, &0u32, &10u32).len(), 0);
    assert_eq!(c.get_intents_by_target(&unknown, &0u32, &10u32).len(), 0);
}

#[test]
fn test_get_intent_returns_stored_fields() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let proposer = Address::generate(&env);
    let target = Address::generate(&env);
    let owners = Vec::from_array(&env, [proposer.clone()]);

    c.initialize(&owners, &1u32);

    let intent_id = c.propose(
        &proposer,
        &target,
        &soroban_sdk::symbol_short!("fn"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.id, intent_id);
    assert_eq!(intent.proposer, proposer);
    assert_eq!(intent.target, target);
    assert_eq!(intent.status, super::IntentStatus::Pending);
    assert_eq!(intent.approval_count, 0);
}
