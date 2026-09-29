use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    token_admin: Address,
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
        token_admin,
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
// Issue #607: clawback_stream must pay recipient their earned balance first
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_607_clawback_mid_stream_pays_recipient_first() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 1_000 tokens over 100 seconds → flow_rate = 10 tokens/s
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // Advance 50 seconds → recipient has earned 500 tokens
    t.env.ledger().set_timestamp(50);

    let recipient_balance_before =
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Clawback using the token admin (issuer)
    c.clawback_stream(&stream_id, &t.token_admin);

    // Recipient must have received their 500 earned tokens
    let recipient_balance_after =
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(
        recipient_balance_after - recipient_balance_before,
        500i128,
        "recipient should receive their earned 500 tokens before clawback"
    );
}

#[test]
fn test_607_clawback_at_start_pays_recipient_nothing() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000i128,
        &100u64,
        &false,
        &default_params(),
    );

    // Clawback immediately — recipient has earned 0
    let recipient_balance_before =
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    c.clawback_stream(&stream_id, &t.token_admin);

    let recipient_balance_after =
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(
        recipient_balance_after,
        recipient_balance_before,
        "recipient should receive nothing when clawback at stream start"
    );
}
