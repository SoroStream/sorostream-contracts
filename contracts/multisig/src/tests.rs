use super::*;
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultiSigContract, ());
    let owners = [Address::generate(&env), Address::generate(&env)];
    let client = MultiSigContractClient::new(&env, &contract_id);
    client.initialize(&vec![&env, owners[0].clone(), owners[1].clone()], &2);
    (env, contract_id, owners[0].clone(), owners[1].clone())
}

#[test]
fn owner_management_methods_preserve_their_invariants() {
    let (env, contract_id, owner, _) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);

    assert_eq!(
        client.try_remove_owner(&owner),
        Err(Ok(MultiSigError::ThresholdExceedsOwners)),
        "removing an owner must not make the threshold unreachable"
    );
    assert_eq!(
        client.try_change_threshold(&0),
        Err(Ok(MultiSigError::InvalidThreshold)),
        "the threshold must remain between one and owner_count"
    );
    assert_eq!(
        client.try_add_owner(&owner),
        Err(Ok(MultiSigError::OwnerAlreadyExists)),
        "an existing owner cannot be added twice"
    );
}

#[test]
fn self_targeting_intent_records_the_method_whose_invariants_apply() {
    let (env, contract_id, owner, _) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    let intent = client.get_intent(&id);
    assert_eq!(intent.target, contract_id);
    assert_eq!(intent.function, Symbol::new(&env, "remove_owner"));
}

#[test]
fn approve_marks_intent_failed_when_target_invocation_fails() {
    let (env, contract_id, owner, second_owner) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    // The second approval reaches the threshold, but the target invocation
    // fails, so the intent must be recorded as Failed rather than Executed.
    assert!(client.try_approve(&second_owner, &id).is_err());
    assert_eq!(client.get_intent(&id).status, IntentStatus::Failed);
    assert_eq!(client.owner_count(), 2);
}

#[test]
fn retry_keeps_intent_failed_while_target_invocation_keeps_failing() {
    let (env, contract_id, owner, second_owner) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    assert!(client.try_approve(&second_owner, &id).is_err());
    assert_eq!(client.get_intent(&id).status, IntentStatus::Failed);

    // Retrying a still-failing invocation leaves the intent Failed.
    assert!(client.try_retry(&owner, &id).is_err());
    assert_eq!(client.get_intent(&id).status, IntentStatus::Failed);
    assert_eq!(client.owner_count(), 2);
}

#[test]
fn retry_transitions_failed_intent_to_executed_on_success() {
    let (env, contract_id, owner, second_owner) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    assert!(client.try_approve(&second_owner, &id).is_err());
    assert_eq!(client.get_intent(&id).status, IntentStatus::Failed);

    // A retry that succeeds must transition the intent to Executed.
    client.retry(&owner, &id);
    assert_eq!(client.get_intent(&id).status, IntentStatus::Executed);
}

#[test]
fn successful_approval_marks_intent_executed() {
    let (env, contract_id, owner, second_owner) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "add_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    assert!(client.approve(&second_owner, &id));
    assert_eq!(client.get_intent(&id).status, IntentStatus::Executed);
}

#[test]
fn typed_self_targeting_call_cannot_bypass_checks_while_bytes_dispatch_is_broken() {
    let (env, contract_id, owner, second_owner) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    // `approve` supplies the calldata Bytes as one argument. `remove_owner`
    // expects an Address, so dispatch fails before its invariant code runs.
    assert!(client.try_approve(&second_owner, &id).is_err());
    assert_eq!(client.owner_count(), 2);
    assert_eq!(client.get_intent(&id).approval_count, 1);
}
