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
// Issue #608: get_stream must return fully-initialised state when called in
//             the same ledger as create_stream
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_608_same_ledger_create_and_get_returns_active() {
    let t = setup();
    let c = client(&t);

    // Set a timestamp that will be == end_time for a 1-second stream
    // (duration 1 → end_time = now + 1, so same-ledger get still sees now < end_time)
    t.env.ledger().set_timestamp(1000);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // get_stream in the same ledger must return Active, not Expired
    let stream = c.get_stream(&stream_id).unwrap();
    assert_eq!(
        stream.status,
        StreamStatus::Active,
        "get_stream in the same ledger as create must return Active"
    );
    assert_eq!(stream.start_time, 1000u64);
    assert_eq!(stream.end_time, 1100u64);
}

#[test]
fn test_608_stream_expires_only_after_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(1000);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // At end_time exactly: should NOT yet be expired (strictly-greater check)
    t.env.ledger().set_timestamp(1100);
    let stream = c.get_stream(&stream_id).unwrap();
    assert_eq!(
        stream.status,
        StreamStatus::Active,
        "stream at exactly end_time must not be Expired"
    );

    // One second after end_time: now expired
    t.env.ledger().set_timestamp(1101);
    let stream = c.get_stream(&stream_id).unwrap();
    assert_eq!(
        stream.status,
        StreamStatus::Expired,
        "stream one second after end_time must be Expired"
    );
}
