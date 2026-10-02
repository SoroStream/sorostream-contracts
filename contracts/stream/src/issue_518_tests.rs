//! Regression tests for Issue #518.
//!
//! `emergency_pause` must prevent *any* pending withdrawal from being
//! processed once the flag is raised: the `withdraw` and `batch_withdraw`
//! entry points must return `StreamError::ContractPaused` and leave both the
//! stream state and the escrowed token balance completely untouched.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    c.set_min_duration(&admin, &0u64);
    // `create_stream` requires an admin-approved token.
    c.add_token_to_whitelist(&admin, &token_id);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn params(nonce: u64) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds: 0,
        nonce,
        renew_count: None,
        recurrence: None,
        lock_until: 0,
        allow_recipient_termination: false,
        non_transferable: false,
        holdback_amount: 0,
        withdrawal_steps: None,
        min_withdrawal_amount: None,
        sponsor: None,
        requires_recipient_approval: false,

        priority: None,
        tags: None,
    }
}

fn create_stream(t: &TestEnv, amount: i128, duration: u64) -> u64 {
    let c = client(t);
    t.env.ledger().set_timestamp(0);
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &amount,
        &duration,
        &false,
        &params(0),
    )
}

fn balance(t: &TestEnv, addr: &Address) -> i128 {
    TokenClient::new(&t.env, &t.token_id).balance(addr)
}

/// A paused contract must reject `withdraw` and move no funds, even though a
/// claimable balance has already accrued.
#[test]
fn test_518_withdraw_blocked_while_paused_moves_no_funds() {
    let t = setup();
    let c = client(&t);
    let stream_id = create_stream(&t, 100_000, 1000);

    // Enough time has elapsed that a withdrawal would otherwise succeed.
    t.env.ledger().set_timestamp(500);

    let escrow_before = balance(&t, &t.contract_id);
    c.emergency_pause();

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));

    // No token moved, no withdrawal recorded.
    assert_eq!(balance(&t, &t.recipient), 0);
    assert_eq!(balance(&t, &t.contract_id), escrow_before);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.total_withdrawn, 0);
}

/// `batch_withdraw` must be blocked by the emergency pause too.
#[test]
fn test_518_batch_withdraw_blocked_while_paused() {
    let t = setup();
    let c = client(&t);
    let stream_id = create_stream(&t, 100_000, 1000);
    t.env.ledger().set_timestamp(500);

    c.emergency_pause();

    let result = c.try_batch_withdraw(&soroban_sdk::vec![&t.env, stream_id], &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
    assert_eq!(balance(&t, &t.recipient), 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.total_withdrawn, 0);
}

/// A stream that is mid-withdrawal (partially drained before the pause) must
/// not release any further funds while the contract is paused.
#[test]
fn test_518_pending_withdrawal_blocked_after_partial_withdraw() {
    let t = setup();
    let c = client(&t);
    let stream_id = create_stream(&t, 100_000, 1000);

    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);

    let after_first = c.get_stream(&stream_id);
    let withdrawn_before = after_first.options.total_withdrawn;
    let recipient_before = balance(&t, &t.recipient);
    assert!(withdrawn_before > 0);

    // A second, still-pending withdrawal accrues but must be frozen by the pause.
    t.env.ledger().set_timestamp(500);
    c.emergency_pause();

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));

    assert_eq!(balance(&t, &t.recipient), recipient_before);
    let after_second = c.get_stream(&stream_id);
    assert_eq!(after_second.options.total_withdrawn, withdrawn_before);
}

/// Once the pause is lifted the same withdrawal succeeds, proving the guard
/// only freezes settlement temporarily.
#[test]
fn test_518_withdraw_resumes_after_emergency_resume() {
    let t = setup();
    let c = client(&t);
    let stream_id = create_stream(&t, 100_000, 1000);

    t.env.ledger().set_timestamp(500);
    c.emergency_pause();
    assert_eq!(
        c.try_withdraw(&stream_id, &t.recipient),
        Err(Ok(StreamError::ContractPaused))
    );

    c.emergency_resume();
    c.withdraw(&stream_id, &t.recipient);
    assert!(balance(&t, &t.recipient) > 0);
}
