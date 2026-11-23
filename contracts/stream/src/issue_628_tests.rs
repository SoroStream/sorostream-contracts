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

    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

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
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #628: Update Stream Parameters Mid-Stream Tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_update_stream_modifies_rate_and_end_time_preserving_claimable() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(),
    );

    // Advance to halfway point
    t.env.ledger().set_timestamp(500);
    let claimable_mid = c.get_claimable(&stream_id);
    assert_eq!(claimable_mid, 500_000);

    // Update flow_rate to 2000 and end_time to 1500
    c.update_stream(&stream_id, &2000i128, &1500u64);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, 2000i128);
    assert_eq!(stream.end_time, 1500u64);

    // Advance 100 seconds (+200_000 at new rate)
    t.env.ledger().set_timestamp(600);
    let claimable_after = c.get_claimable(&stream_id);
    assert_eq!(claimable_after, 700_000);

    // Withdraw succeeds and pays recipient
    c.withdraw(&stream_id, &t.recipient);
    let recipient_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(recipient_balance, 700_000);
}

#[test]
fn test_update_stream_requires_active_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(),
    );

    c.cancel_stream(&stream_id, &t.sender);

    let result = c.try_update_stream(&stream_id, &2000i128, &1500u64);
    assert!(result.is_err());
}
