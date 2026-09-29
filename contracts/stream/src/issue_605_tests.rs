//! Tests for issue #605: recipient index correctness after cancelling multiple
//! streams for the same recipient.
//!
//! After cancelling two streams for the same recipient, `get_streams_by_recipient`
//! must return an empty list (neither stream appears).

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

fn setup_605() -> (Env, Address, Address, Address, Address) {
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

    (env, contract_id, token_id, sender, recipient)
}

/// Cancel two streams for the same recipient and verify neither appears in the
/// recipient index afterwards.
#[test]
fn test_cancel_two_streams_recipient_index_empty() {
    let (env, contract_id, token_id, sender, recipient) = setup_605();
    let c = SoroStreamContractClient::new(&env, &contract_id);

    // Create stream A and stream B for the same recipient.
    let stream_a = c.create_stream(
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
    let stream_b = c.create_stream(
        &sender,
        &recipient,
        &token_id,
        &200_000,
        &2000,
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

    // Confirm both are indexed.
    let before = c.get_streams_by_recipient(&recipient, &0u32, &10u32);
    assert_eq!(before.len(), 2, "both streams should be indexed before cancel");

    env.ledger().set_timestamp(100);

    // Cancel stream A.
    c.cancel_stream(&stream_a, &sender);

    // Verify stream A is gone but B remains.
    let after_a = c.get_streams_by_recipient(&recipient, &0u32, &10u32);
    assert_eq!(after_a.len(), 1, "one stream should remain after cancelling A");
    assert_eq!(
        after_a.get(0).unwrap().id,
        stream_b,
        "remaining stream should be B"
    );

    // Cancel stream B.
    c.cancel_stream(&stream_b, &sender);

    // Verify recipient index is now empty.
    let after_b = c.get_streams_by_recipient(&recipient, &0u32, &10u32);
    assert_eq!(
        after_b.len(),
        0,
        "recipient index must be empty after both streams are cancelled"
    );
}
