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
fn propose_accepts_arbitrary_target_function_and_calldata_verbatim() {
    let (env, contract_id, owner, _) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    // `propose` performs no validation of the intent's target, function or
    // calldata: reviewers must decode the calldata themselves before
    // approving, since nothing here constrains what is being proposed.
    let arbitrary_target = Address::generate(&env);
    let arbitrary_function = Symbol::new(&env, "some_unrelated_function");
    let arbitrary_calldata = Bytes::from_array(&env, &[0xde, 0xad, 0xbe, 0xef]);

    let id = client.propose(
        &owner,
        &arbitrary_target,
        &arbitrary_function,
        &arbitrary_calldata,
        &u64::MAX,
    );

    let intent = client.get_intent(&id);
    assert_eq!(intent.target, arbitrary_target);
    assert_eq!(intent.function, arbitrary_function);
    assert_eq!(intent.calldata, arbitrary_calldata);
    assert_eq!(intent.approval_count, 0);
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

#[test]
fn propose_rejects_calldata_that_decodes_as_non_vec_val() {
    let (env, contract_id, owner, _) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let target = Address::generate(&env);
    let function = Symbol::new(&env, "transfer");

    // Valid XDR for a bare `Val` (not a `Vec<Val>`): decoding as `Vec<Val>`
    // must fail rather than silently reinterpret the bytes.
    let bare_val = Val::U32(1).to_xdr(&env);
    assert_eq!(
        client.try_propose(&owner, &target, &function, &bare_val, &u64::MAX),
        Err(Ok(MultiSigError::InvalidCalldata)),
        "calldata decoding to a single Val must be rejected"
    );

    // Valid XDR for a different container type (`Vec<u32>`): also not a
    // `Vec<Val>` and must be rejected.
    let vec_u32 = vec![&env, 1u32, 2u32].to_xdr(&env);
    assert_eq!(
        client.try_propose(&owner, &target, &function, &vec_u32, &u64::MAX),
        Err(Ok(MultiSigError::InvalidCalldata)),
        "calldata decoding to a Vec<u32> must be rejected"
    );

    // Valid XDR for a `Map<Val, Val>`: a different container type that must
    // likewise be rejected.
    let mut map = Map::<Val, Val>::new(&env);
    map.set(Val::U32(1), Val::U32(2));
    let map_xdr = map.to_xdr(&env);
    assert_eq!(
        client.try_propose(&owner, &target, &function, &map_xdr, &u64::MAX),
        Err(Ok(MultiSigError::InvalidCalldata)),
        "calldata decoding to a Map<Val, Val> must be rejected"
    );

    // The empty-calldata shortcut must remain unaffected.
    assert!(
        client
            .try_propose(&owner, &target, &function, &Bytes::new(&env), &u64::MAX)
            .is_ok(),
        "empty calldata must still be accepted"
    );
}
