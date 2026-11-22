#![cfg(test)]

use super::*;
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Events},
    Address, Bytes, Env, TryFromVal, Vec,
};

/// Minimal execution target: a zero-argument entry point that records that it
/// was invoked, so tests can observe that `approve()` actually executed.
#[contract]
pub struct MockTarget;

#[contractimpl]
impl MockTarget {
    pub fn ping(env: Env) {
        let count: u32 = env.storage().instance().get(&symbol_short!("pings")).unwrap_or(0);
        env.storage().instance().set(&symbol_short!("pings"), &(count + 1));
    }

    pub fn pings(env: Env) -> u32 {
        env.storage().instance().get(&symbol_short!("pings")).unwrap_or(0)
    }
}

/// Counts events emitted by `contract` whose first topic is `name`.
fn count_events(env: &Env, contract: &Address, name: &str) -> u32 {
    let wanted = Symbol::new(env, name);
    let mut n = 0;
    for (addr, topics, _) in env.events().all().iter() {
        if &addr != contract {
            continue;
        }
        if let Some(first) = topics.get(0) {
            if let Ok(sym) = Symbol::try_from_val(env, &first) {
                if sym == wanted {
                    n += 1;
                }
            }
        }
    }
    n
}

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
fn test_get_intent_reports_exact_approval_count_between_approvals() {
    // Issue #982: 3-of-5 multisig, read get_intent after every single approval.
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let o: [Address; 5] = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let owners = Vec::from_array(&env, o.clone());
    c.initialize(&owners, &3u32);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);

    let intent_id = c.propose(&o[0], &target_id, &symbol_short!("ping"), &Bytes::new(&env), &u64::MAX);
    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.approval_count, 0);
    assert_eq!(intent.status, IntentStatus::Pending);

    // First approval: count is exactly 1, still pending, not executed.
    assert!(!c.approve(&o[0], &intent_id));
    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.approval_count, 1);
    assert_eq!(intent.status, IntentStatus::Pending);
    assert_eq!(target.pings(), 0);

    // Second approval: count is exactly 2, still pending, not executed.
    assert!(!c.approve(&o[1], &intent_id));
    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.approval_count, 2);
    assert_eq!(intent.status, IntentStatus::Pending);
    assert_eq!(target.pings(), 0);

    // Third approval reaches the threshold and executes.
    assert!(c.approve(&o[2], &intent_id));
    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.approval_count, 3);
    assert_eq!(intent.status, IntentStatus::Executed);
    assert_eq!(target.pings(), 1);
}

#[test]
fn test_approve_with_threshold_one_executes_on_first_approval() {
    // Issue #831: threshold == 1 is valid; the very first approval must
    // immediately trigger execution.
    //
    // Execution path note (see issue #717 and the `approve` doc comment):
    // `approve()` decodes `intent.calldata` as an XDR `Vec<Val>` and forwards
    // the decoded values as the target's arguments. Empty calldata means no
    // arguments, so this test targets a zero-argument entry point (`ping`),
    // which exercises the real `invoke_contract` path end to end. Targets
    // with typed parameters (e.g. the multisig's own `remove_owner`) are
    // subject to the limitation tracked in #717 and covered separately by
    // `typed_self_targeting_call_cannot_bypass_checks_while_bytes_dispatch_is_broken`.
    let env = env_with_auth();
    let id = env.register(MultiSigContract, ());
    let c = MultiSigContractClient::new(&env, &id);

    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone(), other.clone()]);
    c.initialize(&owners, &1u32);
    assert_eq!(c.get_threshold(), 1);

    let target_id = env.register(MockTarget, ());
    let target = MockTargetClient::new(&env, &target_id);

    let intent_id = c.propose(&owner, &target_id, &symbol_short!("ping"), &Bytes::new(&env), &u64::MAX);
    assert_eq!(target.pings(), 0);

    // A single approval reaches the threshold and executes immediately.
    assert!(c.approve(&owner, &intent_id));

    // Both threshold and execution events fire from that one approval.
    assert_eq!(count_events(&env, &id, "SignatureSubmitted"), 1);
    assert_eq!(count_events(&env, &id, "ThresholdReached"), 1);
    assert_eq!(count_events(&env, &id, "ExecutionCompleted"), 1);

    let intent = c.get_intent(&intent_id);
    assert_eq!(intent.approval_count, 1);
    assert_eq!(intent.status, IntentStatus::Executed);
    assert_eq!(target.pings(), 1);

    // The intent is no longer approvable by the remaining owner.
    assert_eq!(
        c.try_approve(&other, &intent_id),
        Err(Ok(MultiSigError::IntentNotPending))
    );
    assert_eq!(target.pings(), 1);
}
