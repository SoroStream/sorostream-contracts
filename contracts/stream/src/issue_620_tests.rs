
use super::*;
use soroban_sdk::{
    testutils::{Address as _, IssuerFlags, Ledger},
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
    let token = env.register_stellar_asset_contract_v2(token_admin.clone());
    token.issuer().set_flag(IssuerFlags::ClawbackEnabledFlag);
    let token_id = token.address();

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
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #620: minimum stream duration (end_time - start_time >= 1) enforced
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_620_zero_duration_stream_is_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &0u64,
        &false,
        &default_params(),
    );

    assert_eq!(result, Err(Ok(StreamError::MinimumDurationNotMet)));
}

#[test]
fn test_issue_620_one_second_duration_is_accepted() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1u64,
        &false,
        &default_params(),
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time, stream.start_time + 1);
}

#[test]
fn test_issue_620_zero_duration_rejected_independent_of_configured_minimum() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Minimum duration is already 0 (set in `setup`), so the admin-configured
    // floor cannot be what blocks this — the hard zero-duration check must.
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &0u64,
        &false,
        &default_params(),
    );

    assert_eq!(result, Err(Ok(StreamError::MinimumDurationNotMet)));
}
