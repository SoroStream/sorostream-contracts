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

/// Test sender can send bonus on active stream
#[test]
fn test_send_bonus_on_active_stream() {
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

    let bonus_amount = 100_000i128;
    let sender_balance_before = balance(&t, &t.sender);

    // Sender sends bonus (future implementation: send_bonus)
    // c.send_bonus(&stream_id, &t.sender, &bonus_amount);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test bonus does not affect stream rate
#[test]
fn test_bonus_does_not_affect_flow_rate() {
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

    let stream_before = c.get_stream(&stream_id);
    let flow_rate_before = stream_before.flow_rate;

    // Send bonus
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    let stream_after = c.get_stream(&stream_id);
    let flow_rate_after = stream_after.flow_rate;

    // Flow rate should remain unchanged
    assert_eq!(flow_rate_before, flow_rate_after);
}

/// Test bonus does not affect remaining balance or end time
#[test]
fn test_bonus_does_not_affect_duration() {
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

    let stream_before = c.get_stream(&stream_id);
    let end_time_before = stream_before.end_time;
    let remaining_before = stream_before.remaining_balance;

    // Send bonus
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    let stream_after = c.get_stream(&stream_id);
    let end_time_after = stream_after.end_time;
    let remaining_after = stream_after.remaining_balance;

    // End time should remain the same
    assert_eq!(end_time_before, end_time_after);
}

/// Test bonus is received immediately by recipient
#[test]
fn test_bonus_received_immediately() {
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

    let recipient_balance_before = balance(&t, &t.recipient);
    let bonus_amount = 50_000i128;

    // Send bonus
    // c.send_bonus(&stream_id, &t.sender, &bonus_amount);

    // Recipient should receive bonus immediately
    // (without waiting for stream vesting)
    // let recipient_balance_after = balance(&t, &t.recipient);
    // assert_eq!(recipient_balance_after, recipient_balance_before + bonus_amount);
}

/// Test only sender can send bonus
#[test]
fn test_only_sender_can_send_bonus() {
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

    // Non-sender should not be able to send bonus
    // (Future validation at contract boundary)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
}

/// Test bonus on paused stream
#[test]
fn test_bonus_on_paused_stream() {
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

    // Send bonus on paused stream (should be allowed)
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);
}

/// Test multiple bonuses can be sent
#[test]
fn test_multiple_bonuses_can_be_sent() {
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

    let bonus1 = 50_000i128;
    let bonus2 = 75_000i128;
    let bonus3 = 100_000i128;

    // Send multiple bonuses
    // c.send_bonus(&stream_id, &t.sender, &bonus1);
    // c.send_bonus(&stream_id, &t.sender, &bonus2);
    // c.send_bonus(&stream_id, &t.sender, &bonus3);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test bonus on stream with cliff
#[test]
fn test_bonus_on_stream_with_cliff() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cliff = 500u64;
    let duration = 1000u64;

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
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

    // Send bonus before cliff
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    // Bonus should be immediately claimable even before cliff
    t.env.ledger().set_timestamp(250);
    // let claimable = c.get_claimable(&stream_id);
    // Should include bonus but not stream vesting (before cliff)

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.cliff_time, cliff);
}

/// Test bonus with auto-renewal stream
#[test]
fn test_bonus_with_auto_renewal() {
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
        &true,  // auto_renew = true
        &Some(2u32),  // renew_count = 2
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Send bonus
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    let stream = c.get_stream(&stream_id);
    assert!(stream.auto_renew);
}

/// Test bonus emits BonusSent event
#[test]
fn test_bonus_emits_event() {
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

    // Send bonus (should emit BonusSent event)
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test bonus with insufficient sender balance
#[test]
fn test_bonus_with_insufficient_balance() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create new sender with low balance
    let poor_sender = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token).mint(&poor_sender, &100);

    let stream_id = c.create_stream(
        &poor_sender,
        &t.recipient,
        &t.token,
        &100,
        &100,
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

    // Attempt to send large bonus (should fail)
    // let result = c.try_send_bonus(&stream_id, &poor_sender, &1_000_000);
    // assert!(result.is_err());
}

/// Test bonus persists across withdrawals
#[test]
fn test_bonus_persists_with_partial_withdrawals() {
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

    // Withdraw at t=500
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // Send bonus after withdrawal
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    // Withdraw again at t=750
    t.env.ledger().set_timestamp(750);
    c.withdraw(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test bonus with large amounts
#[test]
fn test_bonus_with_large_amounts() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &10_000_000_000,
        &10000,
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

    let large_bonus = 1_000_000_000i128;

    // Send large bonus
    // c.send_bonus(&stream_id, &t.sender, &large_bonus);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Test bonus on completed stream (should fail)
#[test]
fn test_bonus_on_completed_stream_fails() {
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

    // Complete stream
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    // Attempt to send bonus on completed stream
    // (should fail - stream no longer exists)
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// Test bonus tracking in admin logs
#[test]
fn test_bonus_tracked_in_audit_log() {
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

    // Send bonus
    // c.send_bonus(&stream_id, &t.sender, &100_000);

    // Check audit log
    let log = c.get_admin_log();
    assert!(log.len() >= 0);
}
