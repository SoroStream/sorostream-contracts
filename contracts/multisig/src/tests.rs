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
fn single_owner_threshold_cannot_exceed_one() {
    let env = Env::default();
    let contract_id = env.register(MultiSigContract, ());
    let client = MultiSigContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    assert_eq!(
        client.try_initialize(&vec![&env, owner], &2),
        Err(Ok(MultiSigError::InvalidThreshold))
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


// ── #939: swap-and-pop keeps the owner slots consistent across removals ─────

/// True when `owners` holds exactly `expected`, each once, in any order.
fn same_owner_set(owners: &Vec<Address>, expected: &[&Address]) -> bool {
    if owners.len() as usize != expected.len() {
        return false;
    }
    expected.iter().all(|addr| owners.iter().filter(|o| o == *addr).count() == 1)
}

#[test]
fn sequential_removals_keep_owner_slots_consistent() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultiSigContract, ());
    let client = MultiSigContractClient::new(&env, &contract_id);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);
    let e = Address::generate(&env);
    client.initialize(&vec![&env, a.clone(), b.clone(), c.clone(), d.clone(), e.clone()], &2);

    // Remove a non-last owner (slot 1: `e` is swapped into it), then another
    // non-last owner (slot 0: the new last owner, `d`, is swapped into it).
    client.remove_owner(&b);
    assert_eq!(client.get_owners(), vec![&env, a.clone(), e.clone(), c.clone(), d.clone()]);
    client.remove_owner(&a);

    let owners = client.get_owners();
    assert_eq!(owners, vec![&env, d.clone(), e.clone(), c.clone()]);
    assert_eq!(client.owner_count(), 3);

    // Exactly the remaining set: no stale entries, no duplicates.
    assert!(same_owner_set(&owners, &[&c, &d, &e]));
    assert!(!client.is_owner(&a));
    assert!(!client.is_owner(&b));

    // A third removal (of the last slot) and a re-add still keep slots dense.
    client.remove_owner(&c);
    client.add_owner(&a);
    assert_eq!(client.get_owners(), vec![&env, d.clone(), e.clone(), a.clone()]);
    assert_eq!(client.owner_count(), 3);
}

// ── #940: a self-targeting `add_owner` intent through propose() + approve() ─

#[test]
fn self_targeting_add_owner_intent_runs_through_propose_and_approve() {
    use soroban_sdk::xdr::ToXdr;
    use soroban_sdk::IntoVal;

    let (env, contract_id, owner1, owner2) = setup();
    let client = MultiSigContractClient::new(&env, &contract_id);
    let new_owner = Address::generate(&env);

    // add_owner(new_owner) encoded as the XDR of its argument list.
    let args: Vec<Val> = vec![&env, new_owner.into_val(&env)];
    let calldata = args.to_xdr(&env);

    let id = client.propose(
        &owner1,
        &contract_id,
        &Symbol::new(&env, "add_owner"),
        &calldata,
        &(env.ledger().timestamp() + 1_000),
    );

    assert!(!client.approve(&owner1, &id));
    assert!(!client.is_owner(&new_owner));
    assert!(client.approve(&owner2, &id), "the second approval reaches the 2-of-2 threshold");

    assert!(client.is_owner(&new_owner), "the self-targeting intent must add the owner");
    assert_eq!(client.owner_count(), 3);
    assert_eq!(client.get_intent(&id).status, IntentStatus::Executed);
}
