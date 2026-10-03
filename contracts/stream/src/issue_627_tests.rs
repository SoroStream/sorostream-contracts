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
    env.mock_all_auths();

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
    SoroStreamContractClient::new(&env, &contract_id).add_token_to_whitelist(&admin, &token_id);

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
        priority: None,
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #627: Recipient Stream Delegation Tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_recipient_can_delegate_stream_and_delegate_withdraws() {
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

    let delegate = Address::generate(&t.env);

    // Recipient delegates to delegate
    c.delegate_stream(&stream_id, &delegate);
    assert_eq!(c.get_recipient_delegate(&stream_id), Some(delegate.clone()));

    // Advance time to 500s
    t.env.ledger().set_timestamp(500);

    let initial_recipient_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Delegate withdraws on behalf of recipient
    c.withdraw(&stream_id, &delegate);

    let new_recipient_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert!(new_recipient_balance > initial_recipient_balance, "Recipient should receive withdrawn funds");
}

#[test]
fn test_unauthorized_cannot_withdraw_if_not_recipient_or_delegate() {
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

    t.env.ledger().set_timestamp(500);

    let stranger = Address::generate(&t.env);
    let result = c.try_withdraw(&stream_id, &stranger);
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}
