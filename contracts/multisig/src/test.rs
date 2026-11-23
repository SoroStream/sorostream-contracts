#![cfg(test)]

use super::*;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Target contract used to exercise the success/failure paths of `approve()`
/// and `retry()`. The `should_fail` flag is stored in instance storage so a
/// test can flip it between invocations.
#[contract]
pub struct MockTarget;

#[contractimpl]
impl MockTarget {
    pub fn set_should_fail(env: Env, should_fail: bool) {
        env.storage().instance().set(&"fail", &should_fail);
    }

    pub fn run(env: Env) -> u32 {
        let should_fail: bool = env
            .storage()
            .instance()
            .get(&"fail")
            .unwrap_or(false);
        if should_fail {
            panic!("mock target failure");
        }
        42u32
    }
}

fn has_event(env: &Env, contract: &Address, topic: Symbol) -> bool {
    env.events().all().iter().any(|(addr, topics, _)| {
        if &addr != contract {
            return false;
        }
        let expected: Vec<Val> = Vec::from_array(env, [topic.into_val(env)]);
        topics == expected
    })
}

fn count_event(env: &Env, contract: &Address, topic: Symbol) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|(addr, topics, _)| {
            if &addr != contract {
                return false;
            }
            let expected: Vec<Val> = Vec::from_array(env, [topic.into_val(env)]);
            topics == expected
        })
        .count()
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

#[test]
fn test_approve_success_marks_executed_and_emits_completed() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);
    target.set_should_fail(&false);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    c.initialize(&owners, &1u32);

    let intent_id = c.propose(
        &owner,
        &target_id,
        &Symbol::new(&env, "run"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );

    c.approve(&owner, &intent_id);

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.status, super::IntentStatus::Executed);
    assert!(has_event(&env, &id, Symbol::new(&env, "ExecutionCompleted")));
    assert!(!has_event(&env, &id, Symbol::new(&env, "ExecutionFailed")));
}

#[test]
fn test_approve_failure_marks_failed_and_emits_failed() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);
    target.set_should_fail(&true);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    c.initialize(&owners, &1u32);

    let intent_id = c.propose(
        &owner,
        &target_id,
        &Symbol::new(&env, "run"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );

    c.approve(&owner, &intent_id);

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.status, super::IntentStatus::Failed);
    assert!(has_event(&env, &id, Symbol::new(&env, "ExecutionFailed")));
    assert!(!has_event(&env, &id, Symbol::new(&env, "ExecutionCompleted")));
}

#[test]
fn test_retry_failed_intent_succeeds_and_marks_executed() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);
    target.set_should_fail(&true);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    c.initialize(&owners, &1u32);

    let intent_id = c.propose(
        &owner,
        &target_id,
        &Symbol::new(&env, "run"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );

    c.approve(&owner, &intent_id);
    assert_eq!(c.get_intent(&intent_id).status, super::IntentStatus::Failed);

    // Target now succeeds; retry should re-invoke and mark Executed.
    target.set_should_fail(&false);
    c.retry(&owner, &intent_id);

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.status, super::IntentStatus::Executed);
    assert!(has_event(&env, &id, Symbol::new(&env, "ExecutionCompleted")));
    assert_eq!(
        count_event(&env, &id, Symbol::new(&env, "ExecutionCompleted")),
        1
    );
}

#[test]
fn test_retry_failed_intent_still_failing_stays_failed() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);
    target.set_should_fail(&true);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    c.initialize(&owners, &1u32);

    let intent_id = c.propose(
        &owner,
        &target_id,
        &Symbol::new(&env, "run"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );

    c.approve(&owner, &intent_id);
    c.retry(&owner, &intent_id);

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.status, super::IntentStatus::Failed);
    assert!(!has_event(&env, &id, Symbol::new(&env, "ExecutionCompleted")));
}

#[test]
fn test_retry_non_failed_intent_returns_error() {
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);
    target.set_should_fail(&false);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    c.initialize(&owners, &1u32);

    // Pending intent cannot be retried.
    let pending_id = c.propose(
        &owner,
        &target_id,
        &Symbol::new(&env, "run"),
        &soroban_sdk::Bytes::new(&env),
        &1000u64,
    );
    let res = c.try_retry(&owner, &pending_id);
    assert_eq!(res, Err(Ok(super::Error::IntentNotFailed)));

    // Executed intent cannot be retried either.
    c.approve(&owner, &pending_id);
    assert_eq!(
        c.get_intent(&pending_id).status,
        super::IntentStatus::Executed
    );
    let res = c.try_retry(&owner, &pending_id);
    assert_eq!(res, Err(Ok(super::Error::IntentNotFailed)));
}
