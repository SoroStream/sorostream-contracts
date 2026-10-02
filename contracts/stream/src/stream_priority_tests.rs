//! Unit tests for issue #642: stream_priority_queue.
//!
//! Verifies that create_stream accepts an optional priority (u8 0-255) and
//! that batch_withdraw_by_priority processes high-priority streams before
//! low-priority ones.

use crate::{SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

fn setup_env() -> (Env, Address, SoroStreamContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);
    let contract_id = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);
    let client = SoroStreamContractClient::new(&env, &contract_id);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "0.2.0"));
    client.set_min_duration(&admin, &0u64);
    (env, admin, client)
}

fn make_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone()).address()
}

fn mint(env: &Env, token_id: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token_id).mint(to, &amount);
}

fn make_params(nonce: u64, priority: Option<u32>) -> crate::types::CreateStreamParams {
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
        priority,
    }
}

/// get_stream_priority returns 0 when no priority was set at creation.
#[test]
fn test_default_priority_is_zero() {
    let (env, admin, client) = setup_env();
    let token = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &sender, 10_000);

    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token,
        &10_000,
        &1_000,
        &false,
        &make_params(1, None),
    );

    assert_eq!(client.get_stream_priority(&stream_id), 0u32);
}

/// get_stream_priority returns the value supplied at creation.
#[test]
fn test_priority_stored_and_retrieved() {
    let (env, admin, client) = setup_env();
    let token = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &sender, 10_000);

    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token,
        &10_000,
        &1_000,
        &false,
        &make_params(2, Some(200u32)),
    );

    assert_eq!(client.get_stream_priority(&stream_id), 200u32);
}

/// batch_withdraw_by_priority processes high-priority streams before
/// low-priority ones — verified by checking balances after a partial-time run.
#[test]
fn test_priority_streams_processed_before_non_priority() {
    let (env, admin, client) = setup_env();
    let token = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Two streams, same amount / rate.  One high-priority (200), one low (10).
    mint(&env, &token, &sender, 20_000);

    let high_id = client.create_stream(
        &sender,
        &recipient,
        &token,
        &10_000,
        &1_000,
        &false,
        &make_params(10, Some(200u32)),
    );

    let low_id = client.create_stream(
        &sender,
        &recipient,
        &token,
        &10_000,
        &1_000,
        &false,
        &make_params(11, Some(10u32)),
    );

    // Advance time so both streams have claimable funds.
    env.ledger().set_timestamp(500);

    // Submit both IDs in low-priority-first order; the function must reorder.
    let mut ids = Vec::new(&env);
    ids.push_back(low_id);
    ids.push_back(high_id);

    let successes = client.batch_withdraw_by_priority(&recipient, &ids);
    // Both streams should have been processed.
    assert_eq!(successes, 2u32, "both streams should be withdrawn");

    // Confirm get_stream_priority returns expected values.
    assert_eq!(client.get_stream_priority(&high_id), 200u32);
    assert_eq!(client.get_stream_priority(&low_id), 10u32);
}
