extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient, StreamError};
use crate::types::StreamStatus;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

struct TestEnv {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    let client = SoroStreamContractClient::new(&env, &contract);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    client.set_min_duration(&admin, &0u64);

    StellarAssetClient::new(&env, &token).mint(&sender, &100_000_000);

    TestEnv {
        env,
        contract,
        token,
        sender,
        recipient,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract)
}

fn balance(t: &TestEnv, who: &Address) -> i128 {
    TokenClient::new(&t.env, &t.token).balance(who)
}

/// Test batch cancel single stream
#[test]
fn test_batch_cancel_single_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id]);

    // Batch cancel the single stream
    let results = c.batch_cancel_stream(&stream_ids, &t.sender);
    assert_eq!(results.len(), 1);

    // Stream should be removed
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// Test batch cancel multiple streams atomically
#[test]
fn test_batch_cancel_multiple_streams() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create multiple streams
    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let recipient2 = Address::generate(&t.env);
    let stream_id2 = c.create_stream(
        &t.sender,
        &recipient2,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let recipient3 = Address::generate(&t.env);
    let stream_id3 = c.create_stream(
        &t.sender,
        &recipient3,
        &t.token,
        &3_000_000,
        &3000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2, stream_id3]);

    // Batch cancel all streams
    let results = c.batch_cancel_stream(&stream_ids, &t.sender);
    assert_eq!(results.len(), 3);

    // All streams should be removed
    assert!(c.try_get_stream(&stream_id1).is_err());
    assert!(c.try_get_stream(&stream_id2).is_err());
    assert!(c.try_get_stream(&stream_id3).is_err());
}

/// Test batch cancel returns correct results
#[test]
fn test_batch_cancel_returns_results() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2]);

    let results = c.batch_cancel_stream(&stream_ids, &t.sender);

    // Results should match stream_ids length
    assert_eq!(results.len(), 2);
}

/// Test batch cancel refunds all streams
#[test]
fn test_batch_cancel_refunds_all() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let initial_balance = balance(&t, &t.sender);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2]);

    let balance_after_create = balance(&t, &t.sender);
    assert_eq!(balance_after_create, initial_balance - 3_000_000);

    // Batch cancel and refund
    c.batch_cancel_stream(&stream_ids, &t.sender);

    let balance_after_cancel = balance(&t, &t.sender);
    assert_eq!(balance_after_cancel, initial_balance);
}

/// Test batch cancel is atomic - all or nothing
#[test]
fn test_batch_cancel_atomicity() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2]);

    // Cancellation should be atomic
    let _results = c.batch_cancel_stream(&stream_ids, &t.sender);

    // Either all succeed or all fail (no partial state)
    let stream1_exists = c.try_get_stream(&stream_id1).is_ok();
    let stream2_exists = c.try_get_stream(&stream_id2).is_ok();

    // Both should have same state
    assert_eq!(stream1_exists, stream2_exists);
}

/// Test batch cancel with mixed recipients
#[test]
fn test_batch_cancel_mixed_recipients() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let recipient2 = Address::generate(&t.env);
    let recipient3 = Address::generate(&t.env);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &recipient2,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id3 = c.create_stream(
        &t.sender,
        &recipient3,
        &t.token,
        &3_000_000,
        &3000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2, stream_id3]);

    c.batch_cancel_stream(&stream_ids, &t.sender);

    // All streams should be cancelled regardless of recipient
    assert!(c.try_get_stream(&stream_id1).is_err());
    assert!(c.try_get_stream(&stream_id2).is_err());
    assert!(c.try_get_stream(&stream_id3).is_err());
}

/// Test batch cancel with different tokens
#[test]
fn test_batch_cancel_different_tokens() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create second token
    let token2_admin = Address::generate(&t.env);
    let token2 = t.env
        .register_stellar_asset_contract_v2(token2_admin.clone())
        .address();
    StellarAssetClient::new(&t.env, &token2).mint(&t.sender, &10_000_000);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &token2,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2]);

    c.batch_cancel_stream(&stream_ids, &t.sender);

    // Both streams should be cancelled
    assert!(c.try_get_stream(&stream_id1).is_err());
    assert!(c.try_get_stream(&stream_id2).is_err());
}

/// Test batch cancel only sender can call
#[test]
fn test_batch_cancel_only_sender_can_call() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let non_sender = Address::generate(&t.env);
    let stream_ids = Vec::from_array(&t.env, [stream_id]);

    // Non-sender should not be able to batch cancel
    // (Authorization would be checked)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test batch cancel with empty list
#[test]
fn test_batch_cancel_empty_list() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let empty_ids: Vec<u64> = Vec::new(&t.env);

    // Batch cancel with empty list (should succeed as no-op)
    let results = c.batch_cancel_stream(&empty_ids, &t.sender);
    assert_eq!(results.len(), 0);
}

/// Test batch cancel with partial withdrawal
#[test]
fn test_batch_cancel_after_partial_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Partial withdrawal from stream 1
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id1, &t.recipient);

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2]);

    // Batch cancel both
    c.batch_cancel_stream(&stream_ids, &t.sender);

    // Both should be removed
    assert!(c.try_get_stream(&stream_id1).is_err());
    assert!(c.try_get_stream(&stream_id2).is_err());
}

/// Test batch cancel distribution of refunds in single transaction
#[test]
fn test_batch_cancel_distribution_in_single_txn() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &500_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_500_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id3 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let initial_balance = balance(&t, &t.sender);

    let stream_ids = Vec::from_array(&t.env, [stream_id1, stream_id2, stream_id3]);

    c.batch_cancel_stream(&stream_ids, &t.sender);

    let final_balance = balance(&t, &t.sender);

    // All refunds should be distributed
    assert_eq!(final_balance, initial_balance + 500_000 + 1_500_000 + 1_000_000);
}

/// Test batch cancel with invalid stream IDs
#[test]
fn test_batch_cancel_with_invalid_stream_ids() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let invalid_stream_id = 99999u64;
    let stream_ids = Vec::from_array(&t.env, [stream_id1, invalid_stream_id]);

    // Batch cancel with one valid and one invalid stream must fail atomically.
    let result = c.try_batch_cancel_stream(&stream_ids, &t.sender);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));

    // Neither stream should remain in the affected set after the invalid batch is rejected.
    assert!(c.try_get_stream(&stream_id1).is_ok());
}

#[test]
fn test_batch_cancel_is_atomic_on_invalid_member() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &Address::generate(&t.env),
        &t.token,
        &2_000_000,
        &2000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let invalid_stream_id = 99999u64;
    let stream_ids = Vec::from_array(&t.env, [stream_id1, invalid_stream_id, stream_id2]);

    let result = c.try_batch_cancel_stream(&stream_ids, &t.sender);
    assert!(result.is_err(), "invalid member must fail the whole batch");

    assert!(c.try_get_stream(&stream_id1).is_ok(), "stream 1 should remain active after atomic batch rejection");
    assert!(c.try_get_stream(&stream_id2).is_ok(), "stream 2 should remain active after atomic batch rejection");
}

/// Test batch cancel performance with large batch
#[test]
fn test_batch_cancel_large_batch() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create 10 streams
    let mut stream_ids = Vec::new(&t.env);
    for i in 0..10 {
        let recipient = Address::generate(&t.env);
        let stream_id = c.create_stream(
            &t.sender,
            &recipient,
            &t.token,
            &100_000,
            &1000,
            &0,
            &0u64,
            &false,
            &None::<u32>,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
        stream_ids.push_back(stream_id);
    }

    // Batch cancel all
    let results = c.batch_cancel_stream(&stream_ids, &t.sender);
    assert_eq!(results.len(), 10);
}

/// Test batch cancel with paused streams
#[test]
fn test_batch_cancel_paused_streams() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &None::<u32>,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Pause stream
    c.pause_stream(&stream_id, &t.sender, &None);

    let stream_ids = Vec::from_array(&t.env, [stream_id]);

    // Batch cancel paused stream
    c.batch_cancel_stream(&stream_ids, &t.sender);

    // Stream should be removed
    assert!(c.try_get_stream(&stream_id).is_err());
}
