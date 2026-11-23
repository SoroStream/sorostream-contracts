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

fn setup_three() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultiSigContract, ());
    let owners = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let client = MultiSigContractClient::new(&env, &contract_id);
    client.initialize(
        &vec![
            &env,
            owners[0].clone(),
            owners[1].clone(),
            owners[2].clone(),
        ],
        &2,
    );
    (
        env,
        contract_id,
        owners[0].clone(),
        owners[1].clone(),
        owners[2].clone(),
    )
}

#[test]
fn removing_an_approving_owner_decrements_pending_approval_count() {
    let (env, contract_id, owner, second_owner, third_owner) = setup_three();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    assert_eq!(client.get_intent(&id).approval_count, 1);

    // Removing the approving owner must drop their stale approval from the
    // still-pending intent.
    client.remove_owner(&owner);
    assert_eq!(client.owner_count(), 2);
    assert_eq!(client.get_intent(&id).approval_count, 0);

    // The removed owner's approval can no longer count toward the threshold,
    // so a single remaining approval is not enough to execute.
    assert!(!client.approve(&second_owner, &id));
    assert_eq!(client.get_intent(&id).approval_count, 1);
    assert!(client.get_intent(&id).executed == false);

    // A second legitimate approval still reaches the threshold.
    assert!(client.approve(&third_owner, &id));
    assert_eq!(client.get_intent(&id).approval_count, 2);
}

#[test]
fn removing_an_owner_without_approvals_leaves_counts_unchanged() {
    let (env, contract_id, owner, second_owner, third_owner) = setup_three();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let id = client.propose(
        &owner,
        &contract_id,
        &Symbol::new(&env, "remove_owner"),
        &Bytes::new(&env),
        &u64::MAX,
    );

    assert!(!client.approve(&owner, &id));
    assert_eq!(client.get_intent(&id).approval_count, 1);

    // `third_owner` never approved the intent, so removing them must not
    // change the stored approval count.
    client.remove_owner(&third_owner);
    assert_eq!(client.owner_count(), 2);
    assert_eq!(client.get_intent(&id).approval_count, 1);

    // The remaining approval still counts toward the threshold.
    assert!(client.approve(&second_owner, &id));
    assert_eq!(client.get_intent(&id).approval_count, 2);
}
