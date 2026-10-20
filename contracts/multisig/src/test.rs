#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec, Symbol, Bytes,
};

fn setup_test_env() -> (Env, Address, MultiSigContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MultiSigContract, ());
    let client = MultiSigContractClient::new(&env, &contract_id);

    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let owner3 = Address::generate(&env);

    let mut owners = Vec::new(&env);
    owners.push_back(owner1.clone());
    owners.push_back(owner2.clone());
    owners.push_back(owner3.clone());

    client.initialize(&owners, &2);

    (env, contract_id, client, owner1, owner2, owner3)
}

#[test]
fn test_multisig_initialization_invariants() {
    let (_env, _contract_id, client, owner1, owner2, owner3) = setup_test_env();

    assert_eq!(client.owner_count(), 3);
    assert_eq!(client.get_threshold(), 2);
    assert!(client.is_owner(&owner1));
    assert!(client.is_owner(&owner2));
    assert!(client.is_owner(&owner3));
}

#[test]
fn test_direct_owner_management_enforces_invariants() {
    let (env, _contract_id, client, owner1, _owner2, _owner3) = setup_test_env();

    // 1. Duplicate owner invariant
    let res = client.try_add_owner(&owner1);
    assert_eq!(res, Err(Ok(MultiSigError::OwnerAlreadyExists)));

    // 2. Invalid threshold invariant: threshold == 0
    let res = client.try_change_threshold(&0);
    assert_eq!(res, Err(Ok(MultiSigError::InvalidThreshold)));

    // 3. Invalid threshold invariant: threshold > owner_count
    let res = client.try_change_threshold(&4);
    assert_eq!(res, Err(Ok(MultiSigError::InvalidThreshold)));

    // 4. Threshold exceeds owners on removal:
    // With 3 owners and threshold 2, removing 2 owners would leave 1 owner, which is < threshold (2).
    let owner4 = Address::generate(&env);
    let res = client.try_remove_owner(&owner4);
    assert_eq!(res, Err(Ok(MultiSigError::OwnerNotFound)));
}

#[test]
fn test_self_targeting_propose_and_single_bytes_invocation_boundary() {
    let (env, contract_id, client, owner1, owner2, _owner3) = setup_test_env();

    // Verification of Issue #838:
    // propose() does NOT validate whether target == env.current_contract_address()
    let self_target = contract_id.clone();
    let function = Symbol::new(&env, "change_threshold");
    let calldata = Bytes::new(&env);
    let expiry = 100_000u64;

    let intent_id = client.propose(&owner1, &self_target, &function, &calldata, &expiry);
    assert_eq!(intent_id, 0);

    let intent = client.get_intent(&intent_id);
    assert_eq!(intent.target, self_target);
    assert_eq!(intent.function, function);
    assert_eq!(intent.status, IntentStatus::Pending);

    // First owner approves (approval count becomes 1 < threshold 2)
    let approved = client.approve(&owner1, &intent_id);
    assert_eq!(approved, false);

    // Second approval reaches threshold (2 >= 2).
    // Execution is triggered:
    // env.invoke_contract passes Vec::from_array(&env, [intent.calldata])
    // Because change_threshold expects `u32` and receives a single `Bytes`,
    // or because contract execution dispatches single Bytes calldata,
    // this demonstrates the single-Bytes-argument execution boundary.
    let result = client.try_approve(&owner2, &intent_id);
    // The execution fails or panics at the cross-contract invocation layer due to parameter mismatch
    assert!(result.is_err(), "Expected parameter type mismatch when invoking change_threshold with raw Bytes");
}

#[test]
fn test_removal_boundary_preserves_threshold_safety() {
    let (_env, _contract_id, client, _owner1, owner2, owner3) = setup_test_env();

    // Verify remove_owner protects against ThresholdExceedsOwners
    // Current: 3 owners, threshold = 2
    // Removing owner3 leaves 2 owners >= threshold (valid)
    assert!(client.try_remove_owner(&owner3).is_ok());
    assert_eq!(client.owner_count(), 2);
    assert_eq!(client.get_threshold(), 2);

    // Now removing owner2 leaves 1 owner < threshold (2), must fail with ThresholdExceedsOwners
    let res = client.try_remove_owner(&owner2);
    assert_eq!(res, Err(Ok(MultiSigError::ThresholdExceedsOwners)));
}
