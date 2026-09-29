//! Tests for issue #604: cancel_stream double-cancel idempotency.
//!
//! A second call to `cancel_stream` on an already-cancelled stream must return
//! `StreamError::StreamAlreadyCancelled` rather than `StreamNotFound`.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

fn setup_604() -> (Env, Address, Address, Address, Address, u64) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);

    let stream_id = c.create_stream(
        &sender,
        &recipient,
        &token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    (env, contract_id, token_id, sender, recipient, stream_id)
}

/// A second `cancel_stream` call must return `StreamAlreadyCancelled`.
#[test]
fn test_double_cancel_returns_already_cancelled() {
    let (env, contract_id, _token_id, sender, _recipient, stream_id) = setup_604();
    let c = SoroStreamContractClient::new(&env, &contract_id);

    env.ledger().set_timestamp(100);

    // First cancel — succeeds.
    c.cancel_stream(&stream_id, &sender);

    // Second cancel — must fail with StreamAlreadyCancelled.
    let result = c.try_cancel_stream(&stream_id, &sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamAlreadyCancelled)),
        "second cancel_stream should return StreamAlreadyCancelled"
    );
}

/// A cancel on a stream that never existed must still return `StreamNotFound`.
#[test]
fn test_cancel_nonexistent_stream_returns_not_found() {
    let (env, contract_id, _token_id, sender, _recipient, _stream_id) = setup_604();
    let c = SoroStreamContractClient::new(&env, &contract_id);

    let result = c.try_cancel_stream(&999_999u64, &sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotFound)),
        "cancel of never-created stream should return StreamNotFound"
    );
}
