//! Tests for issue #644: stream earnings estimates.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

#[test]
fn earnings_estimate_matches_expiry_with_paused_period() {
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
    StellarAssetClient::new(&env, &token).mint(&sender, &1_000);

    let client = SoroStreamContractClient::new(&env, &contract);
    client.set_min_duration(&sender, &0u64);
    let stream_id = client
        .create_stream(
            &sender,
            &recipient,
            &token,
            &1_000i128,
            &1_000u64,
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

    env.ledger().set_timestamp(400);
    client.pause_stream(&stream_id, &sender, &None);
    env.ledger().set_timestamp(600);
    client.resume_stream(&stream_id, &sender);

    let estimate = client.get_stream_earnings_estimate(&stream_id).unwrap();
    let stream = client.get_stream(&stream_id).unwrap();
    assert_eq!(stream.end_time, 1_200);
    assert_eq!(estimate, 1_000);

    env.ledger().set_timestamp(stream.end_time);
    let balance_before = TokenClient::new(&env, &token).balance(&recipient);
    client.withdraw(&stream_id, &recipient).unwrap();
    let actual_withdrawal = TokenClient::new(&env, &token).balance(&recipient) - balance_before;

    assert_eq!(estimate, actual_withdrawal);
}
