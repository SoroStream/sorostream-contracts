//! Regression tests for Issue #517.
//!
//! `cloneStream` must not reuse the source stream's original deposit.  A clone
//! created mid-stream must be funded with — and be limited to — the value that
//! still has to be streamed over the source's remaining duration, otherwise it
//! can pay out funds the source already vested.

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
    env.mock_all_auths_allowing_non_root_auth();

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

/// A pure clone created at t=400 of a 100_000/1000 stream (rate = 100/s) must
/// carry only the 600s that remain — 60_000 — not the original 100_000.
#[test]
fn test_517_pure_clone_uses_remaining_unstreamed_balance() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);

    t.env.ledger().set_timestamp(400);
    let clone_id = c.clone_stream(
        &source_id,
        &t.sender,
        &None::<Address>,
        &None::<Address>,
        &None::<i128>,
        &None::<u64>,
    );

    assert_ne!(clone_id, source_id);
    let clone = c.get_stream(&clone_id);
    assert_eq!(clone.deposit, 60_000, "clone must use the remaining balance");
    assert_eq!(clone.flow_rate, 100, "clone keeps the source flow rate");
    assert_eq!(clone.start_time, 400);
    assert_eq!(clone.end_time, 1000, "clone lasts only the remaining duration");
    assert_eq!(clone.options.total_withdrawn, 0);
    assert_eq!(clone.sender, t.sender);
    assert_eq!(clone.recipient, t.recipient);

    // The source stream is untouched by the clone.
    let source = c.get_stream(&source_id);
    assert_eq!(source.deposit, 100_000);
    assert_eq!(source.end_time, 1000);
}

/// End-to-end check that the clone can never pay out more than the remaining
/// unstreamed value (the pre-fix behaviour let it drain the full deposit).
#[test]
fn test_517_clone_cannot_pay_out_more_than_remaining() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);

    t.env.ledger().set_timestamp(400);
    let clone_id = c.clone_stream(
        &source_id,
        &t.sender,
        &None::<Address>,
        &None::<Address>,
        &None::<i128>,
        &None::<u64>,
    );

    // Run the clone to completion and drain it.
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&clone_id, &t.recipient);

    let paid = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(paid, 60_000, "clone paid out more than its remaining value");
}

#[test]
fn test_517_clone_with_recipient_override() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);

    let new_recipient = Address::generate(&t.env);
    t.env.ledger().set_timestamp(400);

    let clone_id = c.clone_stream(
        &source_id,
        &t.sender,
        &Some(new_recipient.clone()),
        &None::<Address>,
        &None::<i128>,
        &None::<u64>,
    );

    let clone = c.get_stream(&clone_id);
    assert_eq!(clone.recipient, new_recipient);
    assert_eq!(clone.deposit, 60_000);
}

#[test]
fn test_517_clone_with_rate_override() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);
    t.env.ledger().set_timestamp(400);

    // 200/s over the 600s that remain => 120_000 deposit.
    let clone_id = c.clone_stream(
        &source_id,
        &t.sender,
        &None::<Address>,
        &None::<Address>,
        &Some(200i128),
        &None::<u64>,
    );

    let clone = c.get_stream(&clone_id);
    assert_eq!(clone.flow_rate, 200);
    assert_eq!(clone.deposit, 120_000);
}

#[test]
fn test_517_clone_with_duration_override() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);
    t.env.ledger().set_timestamp(400);

    // 100/s over an explicit 300s => 30_000 deposit ending at 700.
    let clone_id = c.clone_stream(
        &source_id,
        &t.sender,
        &None::<Address>,
        &None::<Address>,
        &None::<i128>,
        &Some(300u64),
    );

    let clone = c.get_stream(&clone_id);
    assert_eq!(clone.deposit, 30_000);
    assert_eq!(clone.end_time, 700);
    assert_eq!(clone.flow_rate, 100);
}

#[test]
fn test_517_non_sender_cannot_clone() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);
    let stranger = Address::generate(&t.env);

    t.env.ledger().set_timestamp(400);
    let result = c.try_clone_stream(
        &source_id,
        &stranger,
        &None::<Address>,
        &None::<Address>,
        &None::<i128>,
        &None::<u64>,
    );
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn test_517_clone_blocked_while_paused() {
    let t = setup();
    let c = client(&t);
    let source_id = create_stream(&t, 100_000, 1000);
    t.env.ledger().set_timestamp(400);
    c.emergency_pause();

    let result = c.try_clone_stream(
        &source_id,
        &t.sender,
        &None::<Address>,
        &None::<Address>,
        &None::<i128>,
        &None::<u64>,
    );
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
}
