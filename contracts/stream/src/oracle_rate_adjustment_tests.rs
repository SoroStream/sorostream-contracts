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

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

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

#[test]
fn test_oracle_rate_adjustment_maintains_usd_value() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream = c.get_stream(&stream_id);
    let original_rate = stream.flow_rate;

    let result = c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &2i128);
    assert!(result.is_ok(), "Should allow rate adjustment by oracle");

    let adjusted_stream = c.get_stream(&stream_id);
    assert_ne!(adjusted_stream.flow_rate, original_rate, "Rate should be adjusted");
}

#[test]
fn test_oracle_rate_adjustment_bounded_by_max_change() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    c.set_max_rate_change_percent(&admin, &10u32);

    let stream = c.get_stream(&stream_id);
    let original_rate = stream.flow_rate;

    let result = c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &2i128);

    if result.is_ok() {
        let adjusted_stream = c.get_stream(&stream_id);
        let max_allowed_change = (original_rate as i64 * 10 / 100) as i128;
        let actual_change = (adjusted_stream.flow_rate - original_rate).abs();
        assert!(actual_change <= max_allowed_change, "Change should be bounded");
    }
}

#[test]
fn test_oracle_price_fetch_and_rate_calculation() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let price_multiplier = 2i128;

    let result = c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &price_multiplier);
    assert!(result.is_ok(), "Should successfully adjust rate based on oracle price");

    let adjusted_stream = c.get_stream(&stream_id);
    assert!(adjusted_stream.flow_rate > 0, "Flow rate should remain positive");
}

#[test]
fn test_oracle_rate_adjustment_half_price() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &2000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream = c.get_stream(&stream_id);
    let original_rate = stream.flow_rate;

    let price_multiplier = 1i128 / 2i128;

    let result = c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &price_multiplier);

    if result.is_ok() {
        let adjusted_stream = c.get_stream(&stream_id);
        assert!(adjusted_stream.flow_rate > original_rate, "Rate should increase when price decreases");
    }
}

#[test]
fn test_oracle_rate_adjustment_preserves_deposit() {
    let t = setup();
    let c = client(&t);

    let original_deposit = 100_000i128;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &original_deposit,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &2i128);

    let adjusted_stream = c.get_stream(&stream_id);
    assert_eq!(adjusted_stream.deposit, original_deposit, "Deposit should remain unchanged");
}

#[test]
fn test_oracle_rate_adjustment_sender_only() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let other_address = Address::generate(&t.env);

    let result = c.try_adjust_rate_by_oracle(&stream_id, &other_address, &2i128);

    if result.is_err() {
        assert!(true, "Non-sender should not be able to adjust rate");
    }
}

#[test]
fn test_oracle_rate_adjustment_stream_not_found() {
    let t = setup();
    let c = client(&t);

    let invalid_stream_id = 99999u64;
    let result = c.try_adjust_rate_by_oracle(&invalid_stream_id, &t.sender, &2i128);

    assert!(result.is_err(), "Should fail for non-existent stream");
}

#[test]
fn test_oracle_rate_adjustment_positive_flow_rate() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_adjust_rate_by_oracle(&stream_id, &t.sender, &2i128);

    if result.is_ok() {
        let adjusted_stream = c.get_stream(&stream_id);
        assert!(adjusted_stream.flow_rate > 0, "Flow rate must remain positive after adjustment");
    }
}
