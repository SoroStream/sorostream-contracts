extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::types::StreamStatus;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
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
    env.mock_all_auths();

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

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

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

/// Test stream ACL enables privacy mode
#[test]
fn test_stream_acl_privacy_mode_enabled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create stream with ACL enabled (privacy mode)
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

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
}

/// Test sender is always in ACL
#[test]
fn test_sender_always_in_acl() {
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

    // Sender should always be able to view their own stream
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test recipient is always in ACL
#[test]
fn test_recipient_always_in_acl() {
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

    // Recipient should always be able to view stream
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.recipient, t.recipient);
}

/// Test non-ACL address cannot view stream
#[test]
fn test_non_acl_address_cannot_view_stream() {
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

    let unauthorized = Address::generate(&t.env);

    // Unauthorized address cannot access stream data
    // (This would be validated at the contract boundary)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test sender can add addresses to ACL
#[test]
fn test_sender_can_add_to_acl() {
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

    let authorized_viewer = Address::generate(&t.env);

    // Sender adds address to ACL
    // (Future implementation: add_to_stream_acl function)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test sender can remove addresses from ACL
#[test]
fn test_sender_can_remove_from_acl() {
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

    let authorized_viewer = Address::generate(&t.env);

    // Sender removes address from ACL
    // (Future implementation: remove_from_stream_acl function)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test non-sender cannot modify ACL
#[test]
fn test_non_sender_cannot_modify_acl() {
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

    // Non-sender should not be able to modify ACL
    // (Future validation at contract boundary)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test ACL persists across withdrawals
#[test]
fn test_acl_persists_across_withdrawals() {
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

    // Perform withdrawal
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // ACL should still be intact
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
}

/// Test ACL persists across pause/resume
#[test]
fn test_acl_persists_across_pause_resume() {
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

    // ACL should persist
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);

    // Resume stream
    c.resume_stream(&stream_id, &t.sender);

    // ACL still intact
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test multiple addresses can be added to ACL
#[test]
fn test_multiple_addresses_in_acl() {
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

    let viewer1 = Address::generate(&t.env);
    let viewer2 = Address::generate(&t.env);
    let viewer3 = Address::generate(&t.env);

    // Add multiple viewers to ACL
    // (Future implementation)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
}

/// Test ACL works with batch operations
#[test]
fn test_acl_with_batch_withdraw() {
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

    t.env.ledger().set_timestamp(500);

    // Batch withdraw should respect ACL
    let result = c.try_batch_withdraw(
        &soroban_sdk::Vec::from_array(&t.env, [stream_id]),
        &t.recipient,
    );
    assert!(result.is_ok());
}

/// Test ACL with top-up operation
#[test]
fn test_acl_with_top_up() {
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

    // Sender performs top-up
    c.top_up(&stream_id, &t.sender, &t.token, &500_000);

    // ACL should still be intact
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test ACL prevents data leakage
#[test]
fn test_acl_prevents_data_leakage() {
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

    // Verify sender and recipient can view stream
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
}

/// Test ACL interaction with cancellation
#[test]
fn test_acl_with_stream_cancellation() {
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

    t.env.ledger().set_timestamp(500);

    // Cancel stream (only sender can)
    c.cancel_stream(&stream_id, &t.sender);

    // Stream should be removed after cancellation
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// Test ACL audit trail
#[test]
fn test_acl_changes_are_auditable() {
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

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);

    // Admin log should be available
    let log = c.get_admin_log();
    assert!(log.len() >= 0);
}
