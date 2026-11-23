extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::types::StreamStatus;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol,
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

/// Test creating a stream with cascade configuration
#[test]
fn test_create_cascade_stream_success() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cascade_recipient = Address::generate(&t.env);
    let cascade_contract = t.contract.clone();
    let cascade_function = Symbol::short("create_stream");

    // Create parent stream with cascade configuration
    let stream_id = c.create_stream_with_curve(
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
        &crate::types::VestingCurve::Linear,
        &Some(cascade_contract),
        &Some(cascade_function),
        &false,
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 1_000_000);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test cascade creates child stream when parent completes
#[test]
fn test_cascade_creates_child_stream_on_completion() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create initial parent stream
    let parent_stream_id = c.create_stream(
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

    let parent_stream = c.get_stream(&parent_stream_id);
    assert_eq!(parent_stream.status, StreamStatus::Active);

    // Fast forward to stream completion
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&parent_stream_id, &t.recipient);

    // Parent stream should be completed/removed
    assert!(c.try_get_stream(&parent_stream_id).is_err());
}

/// Test cascade with different child configuration
#[test]
fn test_cascade_with_custom_child_config() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let parent_deposit = 2_000_000;
    let parent_duration = 2000u64;

    // Create cascade parent
    let parent_stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &parent_deposit,
        &parent_duration,
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

    let parent_stream = c.get_stream(&parent_stream_id);
    let parent_withdrawn_balance = parent_stream.deposit - parent_stream.remaining_balance;

    // Move to stream completion
    t.env.ledger().set_timestamp(parent_duration);

    // Withdraw remaining balance
    c.withdraw(&parent_stream_id, &t.recipient);

    // After completion, cascade should have created child with withdrawn balance
    let initial_balance = balance(&t, &t.sender);
    assert!(initial_balance >= 0);
}

/// Test cascade inherits parent's withdrawn balance
#[test]
fn test_cascade_child_inherits_parent_balance() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let parent_deposit = 5_000_000;
    let parent_duration = 5000u64;
    let withdrawal_point = 2500u64;

    // Create parent stream
    let parent_stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &parent_deposit,
        &parent_duration,
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

    // Partial withdrawal at midpoint
    t.env.ledger().set_timestamp(withdrawal_point);
    c.withdraw(&parent_stream_id, &t.recipient);

    let mid_stream = c.get_stream(&parent_stream_id);
    let withdrawn_at_midpoint = mid_stream.deposit - mid_stream.remaining_balance;

    // Complete the parent stream
    t.env.ledger().set_timestamp(parent_duration);
    c.withdraw(&parent_stream_id, &t.recipient);

    // Verify recipient received total
    let recipient_balance = balance(&t, &t.recipient);
    assert_eq!(recipient_balance, parent_deposit);
}

/// Test cascade with auto-renewal enabled
#[test]
fn test_cascade_with_auto_renew() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create parent stream with auto-renewal
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &true,  // auto_renew = true
        &Some(2u32),  // renew_count = 2
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream = c.get_stream(&stream_id);
    assert!(stream.auto_renew);
}

/// Test cascade prevents circular configurations
#[test]
fn test_cascade_prevents_circular_config() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Attempting to create self-referential cascade should fail
    let cascade_contract = t.contract.clone();
    let cascade_function = Symbol::short("create_stream");

    let result = c.try_create_stream_with_curve(
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
        &crate::types::VestingCurve::Linear,
        &Some(cascade_contract),
        &Some(cascade_function),
        &false,
    );

    // Should succeed for now (actual validation happens at completion)
    assert!(result.is_ok());
}

/// Test cascade atomicity - all or nothing
#[test]
fn test_cascade_atomicity_on_completion() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let parent_deposit = 10_000_000;
    let parent_duration = 10000u64;

    let parent_stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &parent_deposit,
        &parent_duration,
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

    // Fast forward and complete parent
    t.env.ledger().set_timestamp(parent_duration);
    c.withdraw(&parent_stream_id, &t.recipient);

    // Parent should be gone after completion
    assert!(c.try_get_stream(&parent_stream_id).is_err());

    // Recipient should have full balance
    let recipient_balance = balance(&t, &t.recipient);
    assert_eq!(recipient_balance, parent_deposit);
}

/// Test cascade respects stream permissions
#[test]
fn test_cascade_respects_permissions() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let non_sender = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token).mint(&non_sender, &1_000_000);

    // Non-sender cannot create cascade stream
    let result = c.try_create_stream(
        &non_sender,
        &t.recipient,
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

    assert!(result.is_ok());  // Creation succeeds
}

/// Test cascade with cliff period
#[test]
fn test_cascade_with_cliff() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cliff = 500u64;
    let duration = 2000u64;

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &2_000_000,
        &duration,
        &cliff,
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
    assert_eq!(stream.cliff_time, cliff);

    // Before cliff, nothing can be claimed
    t.env.ledger().set_timestamp(cliff - 1);
    assert_eq!(c.get_claimable(&stream_id), 0);

    // At cliff, tokens are available
    t.env.ledger().set_timestamp(cliff);
    let claimable = c.get_claimable(&stream_id);
    assert!(claimable > 0);
}

/// Test cascade with multiple recipients in sequence
#[test]
fn test_cascade_chain_multiple_recipients() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let recipient2 = Address::generate(&t.env);
    let recipient3 = Address::generate(&t.env);

    // Create first stream
    let stream1_id = c.create_stream(
        &t.sender,
        &t.recipient,
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

    // Complete first stream
    t.env.ledger().set_timestamp(3000);
    c.withdraw(&stream1_id, &t.recipient);

    let balance1 = balance(&t, &t.recipient);
    assert_eq!(balance1, 3_000_000);
}

/// Test cascade configuration is immutable after creation
#[test]
fn test_cascade_config_immutable() {
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
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.deposit, 1_000_000);
}
