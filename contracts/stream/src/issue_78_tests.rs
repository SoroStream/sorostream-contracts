use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct TestEnv {
    env: Env,
    admin: Address,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    c.add_token_to_whitelist(&admin, &token_id);
    c.set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        admin,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn default_params() -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds: 0,
        nonce: 0,
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
        priority: None,
        tags: None,
        metadata_uri: None,
    }
}

fn create_stream(t: &TestEnv, amount: i128, duration: u64) -> u64 {
    client(t).create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &amount,
        &duration,
        &false,
        &default_params(),
    )
}

// -------------------------------------------------------------------------
// Issue #78: Modify flow rate after stream creation
// -------------------------------------------------------------------------

#[test]
fn test_modify_flow_rate_up_locks_claimable_and_shortens_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 1_000 per second for 1_000 seconds.
    let stream_id = create_stream(&t, 1_000_000, 1000);

    t.env.ledger().set_timestamp(500);
    let claimable_before = c.get_claimable(&stream_id);
    assert_eq!(claimable_before, 500_000);

    // New rate 2_000/s. Remaining unaccrued balance = 500_000 -> 250 seconds.
    c.modify_flow_rate(&stream_id, &t.sender, &2000i128);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, 2000);
    assert_eq!(stream.end_time, 750);

    // The balance earned at the old rate is locked and still claimable.
    assert_eq!(c.get_claimable(&stream_id), claimable_before);

    // The new rate applies from the modification timestamp onward.
    t.env.ledger().set_timestamp(600);
    assert_eq!(c.get_claimable(&stream_id), 700_000);

    t.env.ledger().set_timestamp(750);
    assert_eq!(c.get_claimable(&stream_id), 1_000_000);

    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient),
        1_000_000
    );
}

#[test]
fn test_modify_flow_rate_down_locks_claimable_and_extends_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stream(&t, 1_000_000, 1000);

    t.env.ledger().set_timestamp(400);
    let claimable_before = c.get_claimable(&stream_id);
    assert_eq!(claimable_before, 400_000);

    // New rate 500/s. Remaining unaccrued balance = 600_000 -> 1_200 seconds.
    c.modify_flow_rate(&stream_id, &t.sender, &500i128);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, 500);
    assert_eq!(stream.end_time, 1600);
    assert_eq!(c.get_claimable(&stream_id), claimable_before);

    // 200 seconds at the new rate adds 100_000 to the locked claimable.
    t.env.ledger().set_timestamp(600);
    assert_eq!(c.get_claimable(&stream_id), 500_000);

    t.env.ledger().set_timestamp(1600);
    assert_eq!(c.get_claimable(&stream_id), 1_000_000);
}

#[test]
fn test_modify_flow_rate_only_sender_can_modify() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stream(&t, 1_000_000, 1000);
    let stranger = Address::generate(&t.env);

    let result = c.try_modify_flow_rate(&stream_id, &stranger, &2000i128);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));

    let result = c.try_modify_flow_rate(&stream_id, &t.recipient, &2000i128);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));

    assert_eq!(c.get_stream(&stream_id).flow_rate, 1000);
}

#[test]
fn test_modify_flow_rate_rejects_zero_or_negative_rate() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stream(&t, 1_000_000, 1000);

    assert_eq!(
        c.try_modify_flow_rate(&stream_id, &t.sender, &0i128),
        Err(Ok(StreamError::ZeroFlowRate))
    );
    assert_eq!(
        c.try_modify_flow_rate(&stream_id, &t.sender, &-1i128),
        Err(Ok(StreamError::ZeroFlowRate))
    );
}

#[test]
fn test_modify_flow_rate_requires_active_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stream(&t, 1_000_000, 1000);
    c.pause_stream(&stream_id, &t.sender, &None::<soroban_sdk::String>);

    assert_eq!(
        c.try_modify_flow_rate(&stream_id, &t.sender, &2000i128),
        Err(Ok(StreamError::StreamNotActive))
    );
}

#[test]
fn test_modify_flow_rate_claimable_continuous_after_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stream(&t, 1_000_000, 1000);

    // Withdraw the first 200 seconds' worth at the old rate.
    t.env.ledger().set_timestamp(200);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_stream(&stream_id).options.total_withdrawn, 200_000);

    // At t=500, 300_000 has accrued since the withdrawal.
    t.env.ledger().set_timestamp(500);
    let claimable_before = c.get_claimable(&stream_id);
    assert_eq!(claimable_before, 300_000);

    c.modify_flow_rate(&stream_id, &t.sender, &2000i128);

    // Locked 300_000 + remaining unaccrued 500_000 / 2_000 = 250 seconds.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time, 750);
    assert_eq!(c.get_claimable(&stream_id), claimable_before);

    t.env.ledger().set_timestamp(600);
    assert_eq!(c.get_claimable(&stream_id), 500_000);

    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient),
        700_000
    );
}
