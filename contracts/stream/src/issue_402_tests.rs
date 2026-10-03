
use super::*;
use soroban_sdk::{
    testutils::{Address as _, IssuerFlags, Ledger},
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
    SoroStreamContractClient::new(&env, &contract_id).add_token_to_whitelist(&admin, &token_id);

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

fn params_with_metadata_uri(
    t: &TestEnv,
    metadata_uri: Option<soroban_sdk::String>,
) -> crate::types::CreateStreamParams {
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
        metadata_uri,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #402: on-chain metadata field for arbitrary annotations
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_402_metadata_uri_can_be_set_at_creation() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let uri = soroban_sdk::String::from_str(&t.env, "ipfs://invoice-42");
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &params_with_metadata_uri(&t, Some(uri.clone())),
    );

    assert_eq!(c.get_metadata_uri(&stream_id), Some(uri));
}

#[test]
fn test_issue_402_metadata_uri_defaults_to_none_when_omitted() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &params_with_metadata_uri(&t, None),
    );

    assert_eq!(c.get_metadata_uri(&stream_id), None);
}

#[test]
fn test_issue_402_metadata_uri_over_128_bytes_rejected_at_creation() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let too_long = soroban_sdk::String::from_str(
        &t.env,
        &"a".repeat(129),
    );
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &params_with_metadata_uri(&t, Some(too_long)),
    );

    assert_eq!(result, Err(Ok(StreamError::InvalidMetadataUri)));
}

#[test]
fn test_issue_402_metadata_uri_settable_post_creation_still_works() {
    // Guards against a regression where wiring the creation-time path breaks
    // the pre-existing post-creation update_metadata_uri entry point.
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &params_with_metadata_uri(&t, None),
    );
    assert_eq!(c.get_metadata_uri(&stream_id), None);

    let uri = soroban_sdk::String::from_str(&t.env, "https://example.com/receipt/7");
    c.update_metadata_uri(&t.sender, &stream_id, &Some(uri.clone()));
    assert_eq!(c.get_metadata_uri(&stream_id), Some(uri));
}
