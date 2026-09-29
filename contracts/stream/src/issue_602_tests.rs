//! Tests for issue #602: `cancel_stream` must remove the stream's metadata blob
//! from temporary storage so it is no longer accessible after cancellation.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Bytes, Env,
};

/// After cancelling a stream, `get_metadata` must return `None`.
#[test]
fn test_metadata_cleared_on_cancel() {
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

    // Attach metadata to the stream.
    let metadata = Bytes::from_slice(&env, b"invoice-2026-09");
    c.update_metadata(&sender, &stream_id, &metadata);

    // Confirm metadata is present before cancel.
    assert_eq!(
        c.get_metadata(&stream_id),
        Some(metadata),
        "metadata should be present before cancellation"
    );

    env.ledger().set_timestamp(100);

    // Cancel the stream.
    c.cancel_stream(&stream_id, &sender);

    // Metadata must be gone.
    assert_eq!(
        c.get_metadata(&stream_id),
        None,
        "metadata must be cleared after cancel_stream"
    );
}
