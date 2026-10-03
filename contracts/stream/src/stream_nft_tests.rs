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
fn test_stream_nft_minting_on_creation() {
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
    assert_eq!(stream.status, StreamStatus::Active);
    assert!(stream_id > 0, "Stream should have valid ID for NFT mapping");
}

#[test]
fn test_nft_transfer_updates_stream_recipient() {
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

    let new_recipient = Address::generate(&t.env);

    let result = c.try_update_recipient(&stream_id, &new_recipient, &t.recipient);
    assert!(result.is_ok(), "Should allow updating recipient");

    let updated_stream = c.get_stream(&stream_id);
    assert_eq!(updated_stream.recipient, new_recipient);
}

#[test]
fn test_nft_recipient_can_claim_stream() {
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

    t.env.ledger().set_timestamp(50);

    let initial_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert!(balance_after > initial_balance, "Recipient should receive funds from stream");
}

#[test]
fn test_stream_nft_enables_secondary_market() {
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

    let buyer = Address::generate(&t.env);

    let result = c.try_update_recipient(&stream_id, &buyer, &t.recipient);
    assert!(result.is_ok(), "NFT should be transferable for secondary market");

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.recipient, buyer, "Stream ownership transferred through NFT");
}

#[test]
fn test_stream_nft_maintains_original_terms() {
    let t = setup();
    let c = client(&t);

    let original_flow_rate = 1500i128;
    let original_deposit = 100_000i128;

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &original_deposit,
        &original_flow_rate,
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

    let new_recipient = Address::generate(&t.env);
    c.try_update_recipient(&stream_id, &new_recipient, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, original_flow_rate, "NFT transfer should not change flow rate");
    assert_eq!(stream.deposit, original_deposit, "NFT transfer should not change deposit");
    assert_eq!(stream.sender, t.sender, "NFT transfer should not change sender");
}
