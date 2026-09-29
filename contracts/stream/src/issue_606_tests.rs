use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
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
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #606: top_up must be rejected when the stream is paused
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_606_top_up_on_paused_stream_is_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create a 100-second stream with 1_000 tokens → flow_rate = 10
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // Pause the stream
    c.pause_stream(&stream_id, &t.sender);

    // top_up must now fail with StreamPaused
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &500i128);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamPaused)),
        "top_up on a paused stream must return StreamPaused"
    );
}

#[test]
fn test_606_top_up_succeeds_on_active_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create a stream with flow_rate = 10 (1_000 / 100)
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // top_up while active must succeed
    c.top_up(&stream_id, &t.sender, &t.token_id, &500i128);

    let stream = c.get_stream(&stream_id).unwrap();
    assert_eq!(stream.status, StreamStatus::Active);
    // effective_amount = 500 - (500 % 10) = 500; extra_seconds = 50
    assert_eq!(stream.end_time, 150u64);
}
