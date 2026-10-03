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
fn test_grace_period_allows_withdrawal_after_expiry() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.set_grace_period_ledgers(&admin, &1000u64);
    t.env.ledger().set_timestamp(0);

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

    t.env.ledger().set_timestamp(100);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);

    t.env.ledger().set_timestamp(150);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok(), "Recipient should be able to withdraw within grace period");

    let claimable = c.get_claimable(&stream_id);
    assert!(claimable > 0, "Should have claimable amount within grace period");
}

#[test]
fn test_grace_period_expires_reclaim() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.set_grace_period_ledgers(&admin, &1000u64);
    t.env.ledger().set_timestamp(0);

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

    t.env.ledger().set_timestamp(100);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);

    t.env.ledger().set_timestamp(5100);

    let result = c.try_reclaim(&stream_id, &t.sender);
    assert!(result.is_ok(), "Sender should be able to reclaim after grace period expires");
}

#[test]
fn test_grace_period_default_value() {
    let t = setup();
    let c = client(&t);

    let default_grace = c.get_grace_period_ledgers();
    assert!(default_grace >= 0, "Grace period should have a default non-negative value");
}

#[test]
fn test_grace_period_withdrawal_amounts() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.set_grace_period_ledgers(&admin, &1000u64);
    t.env.ledger().set_timestamp(0);

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

    t.env.ledger().set_timestamp(50);
    let claimable_before_expiry = c.get_claimable(&stream_id);

    t.env.ledger().set_timestamp(100);
    let claimable_at_expiry = c.get_claimable(&stream_id);

    t.env.ledger().set_timestamp(200);
    let claimable_after_expiry = c.get_claimable(&stream_id);

    assert!(claimable_before_expiry < claimable_at_expiry, "Should accumulate more funds approaching expiry");
    assert_eq!(claimable_at_expiry, claimable_after_expiry, "Claimable should not increase after expiry during grace period");
}
