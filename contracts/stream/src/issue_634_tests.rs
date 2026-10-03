//! Tests for issue #634: persisted stream pause reasons.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

fn setup_634() -> (Env, Address, Address, Address, Address, u64) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.ledger().set_timestamp(0);

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &100_000);

    let client = SoroStreamContractClient::new(&env, &contract);
    client.set_min_duration(&sender, &0u64);
    let stream_id = client
        .create_stream(
            &sender,
            &recipient,
            &token,
            &1_000i128,
            &100u64,
            &false,
            &0u64,
            &CreateStreamParams {
                cliff_seconds: 0,
                nonce: 1,
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
                tags: None,
            },
        )
        .unwrap();

    (env, contract, sender, recipient, token, stream_id)
}

#[test]
fn test_634_pause_reason_is_stored_and_survives_resume() {
    let (env, contract, sender, _, _, stream_id) = setup_634();
    let client = SoroStreamContractClient::new(&env, &contract);
    let reason = String::from_str(&env, "scheduled maintenance");

    client.pause_stream(&stream_id, &sender, &Some(reason.clone()));
    assert_eq!(
        client.get_stream(&stream_id).unwrap().options.pause_reason,
        Some(reason.clone())
    );

    client.resume_stream(&stream_id, &sender);
    assert_eq!(
        client.get_stream(&stream_id).unwrap().options.pause_reason,
        Some(reason)
    );
}

#[test]
fn test_634_pause_reason_is_limited_to_256_bytes() {
    let (env, contract, sender, _, _, stream_id) = setup_634();
    let client = SoroStreamContractClient::new(&env, &contract);
    let reason = String::from_str(
        &env,
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );

    assert_eq!(reason.len(), 257);
    assert_eq!(
        client.try_pause_stream(&stream_id, &sender, &Some(reason)),
        Err(Ok(StreamError::InvalidParameter))
    );
}

#[test]
fn test_634_accepts_256_byte_pause_reason() {
    let (env, contract, sender, _, _, stream_id) = setup_634();
    let client = SoroStreamContractClient::new(&env, &contract);
    let reason = String::from_str(&env, &"x".repeat(256));

    assert_eq!(reason.len(), 256);
    client.pause_stream(&stream_id, &sender, &Some(reason.clone()));
    assert_eq!(
        client.get_stream(&stream_id).unwrap().options.pause_reason,
        Some(reason)
    );
}
