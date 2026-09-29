use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    admin: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);

    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &String::from_str(&env, "1.0.0"));

    TestEnv {
        env,
        contract_id,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

/// The mandatory protocol-fee update timelock, in seconds (48 hours).
const TIMELOCK: u64 = 48 * 60 * 60;

// ─────────────────────────────────────────────────────────────────────────
// Issue #523: access-controlled fee update with a 48-hour timelock
// ─────────────────────────────────────────────────────────────────────────
// A proposed fee change must sit in a pending state for at least 48 hours of
// ledger time before a separate commit entry point applies it, and the pending
// change must be observable by users through `get_pending_fee_update`.

#[test]
fn test_issue_523_no_update_is_pending_initially() {
    let t = setup();
    let c = client(&t);

    assert_eq!(c.get_pending_fee_update(), None);
    assert_eq!(c.get_protocol_fee_info().0, 0);
}

#[test]
fn test_issue_523_proposal_is_pending_and_not_applied_immediately() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.set_protocol_fee(&500u32);

    // The live fee is untouched: a proposal alone must never change it.
    assert_eq!(c.get_protocol_fee_info().0, 0);

    // …but the pending update is visible, with the mandatory 48-hour unlock.
    assert_eq!(c.get_pending_fee_update(), Some((500u32, TIMELOCK)));
}

#[test]
fn test_issue_523_execute_before_timelock_is_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.set_protocol_fee(&500u32);

    // One second before the timelock elapses the commit must still fail.
    t.env.ledger().set_timestamp(TIMELOCK - 1);
    assert_eq!(
        c.try_execute_fee_change(),
        Err(Ok(StreamError::StreamLocked))
    );
    assert_eq!(c.get_protocol_fee_info().0, 0);
    assert_eq!(c.get_pending_fee_update(), Some((500u32, TIMELOCK)));
}

#[test]
fn test_issue_523_execute_at_timelock_boundary_applies_fee() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.set_protocol_fee(&500u32);

    // Exactly at the boundary the commit succeeds.
    t.env.ledger().set_timestamp(TIMELOCK);
    c.execute_fee_change();

    assert_eq!(c.get_protocol_fee_info().0, 500);
    // The proposal is consumed, so nothing is left pending.
    assert_eq!(c.get_pending_fee_update(), None);
}

#[test]
fn test_issue_523_execute_without_proposal_fails() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(TIMELOCK * 4);

    assert_eq!(
        c.try_execute_fee_change(),
        Err(Ok(StreamError::NotAuthorized))
    );
    assert_eq!(c.get_protocol_fee_info().0, 0);
}

#[test]
fn test_issue_523_propose_via_admin_entry_point_uses_same_timelock() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.propose_fee_change(&t.admin, &250u32);
    assert_eq!(c.get_pending_fee_update(), Some((250u32, TIMELOCK)));

    t.env.ledger().set_timestamp(TIMELOCK);
    c.execute_fee_change();
    assert_eq!(c.get_protocol_fee_info().0, 250);
    assert_eq!(c.get_pending_fee_update(), None);
}

#[test]
fn test_issue_523_propose_fee_change_requires_admin() {
    let t = setup();
    let c = client(&t);
    let stranger = Address::generate(&t.env);

    let result = c.try_propose_fee_change(&stranger, &100u32);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
    assert_eq!(c.get_pending_fee_update(), None);
}

#[test]
fn test_issue_523_reproposal_restarts_the_timelock_window() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.set_protocol_fee(&100u32);
    assert_eq!(c.get_pending_fee_update(), Some((100u32, TIMELOCK)));

    // A second proposal replaces the first and restarts the window.
    t.env.ledger().set_timestamp(1_000);
    c.set_protocol_fee(&700u32);
    assert_eq!(c.get_pending_fee_update(), Some((700u32, 1_000 + TIMELOCK)));

    // The original unlock time is no longer sufficient to commit the change.
    t.env.ledger().set_timestamp(TIMELOCK);
    assert_eq!(
        c.try_execute_fee_change(),
        Err(Ok(StreamError::StreamLocked))
    );

    t.env.ledger().set_timestamp(1_000 + TIMELOCK);
    c.execute_fee_change();
    assert_eq!(c.get_protocol_fee_info().0, 700);
}

#[test]
fn test_issue_523_rejects_fee_above_maximum() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 10 001 bps (> 100 %) is invalid and must never enter the pending slot.
    assert_eq!(
        c.try_set_protocol_fee(&10_001u32),
        Err(Ok(StreamError::InvalidDuration))
    );
    assert_eq!(
        c.try_propose_fee_change(&t.admin, &10_001u32),
        Err(Ok(StreamError::InvalidDuration))
    );
    assert_eq!(c.get_pending_fee_update(), None);

    // The maximum itself (100 %) is accepted and only applied after the timelock.
    c.set_protocol_fee(&10_000u32);
    t.env.ledger().set_timestamp(TIMELOCK);
    c.execute_fee_change();
    assert_eq!(c.get_protocol_fee_info().0, 10_000);
}
