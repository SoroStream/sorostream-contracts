use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
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

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&admin, &0u64);
    // The token must be on the mandatory whitelist before streams can use it.
    c.add_token_to_whitelist(&admin, &token_id);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn params(
    nonce: u64,
    cliff_seconds: u64,
    holdback_amount: i128,
    withdrawal_steps: Option<u32>,
    min_withdrawal_amount: Option<i128>,
) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds,
        nonce,
        renew_count: None,
        recurrence: None,
        lock_until: 0,
        allow_recipient_termination: false,
        non_transferable: false,
        holdback_amount,
        withdrawal_steps,
        min_withdrawal_amount,
        sponsor: None,
        requires_recipient_approval: false,
        priority: None,
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #522: strict type validation on XDR-decoded inputs
// ─────────────────────────────────────────────────────────────────────────
// Every guard returns a typed error and rejects the value before it can reach
// business logic.

#[test]
fn test_issue_522_rejects_non_positive_stream_amount() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &0i128,
        &1000u64,
        &false,
        &params(1, 0, 0, None, None),
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));

    let negative = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &-1i128,
        &1000u64,
        &false,
        &params(2, 0, 0, None, None),
    );
    assert_eq!(negative, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn test_issue_522_rejects_zero_duration() {
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
        &params(1, 0, 0, None, None),
    );
    // Issue #620 gave zero-duration streams their own, more specific error
    // (previously InvalidDuration, a catch-all shared with unrelated checks).
    assert_eq!(result, Err(Ok(StreamError::MinimumDurationNotMet)));
}

#[test]
fn test_issue_522_rejects_cliff_beyond_duration() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1, 1_001, 0, None, None),
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidCliff)));
}

#[test]
fn test_issue_522_rejects_holdback_covering_whole_deposit() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1, 0, 100_000, None, None),
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn test_issue_522_rejects_zero_withdrawal_steps() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1, 0, 0, Some(0), None),
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

#[test]
fn test_issue_522_rejects_non_positive_min_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1, 0, 0, None, Some(0)),
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn test_issue_522_rejects_zero_rate_on_non_zero_deposit() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 1 stroop over 1 000 seconds truncates to a zero flow rate.
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1i128,
        &1000u64,
        &false,
        &params(1, 0, 0, None, None),
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroFlowRate)));
}

#[test]
fn test_issue_522_rejects_over_long_federation_name() {
    let t = setup();
    let c = client(&t);
    let stellar_address = Address::generate(&t.env);

    // 70 bytes > MAX_FEDERATION_NAME_BYTES (64).
    let long_name = String::from_str(
        &t.env,
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789",
    );
    let result = c.try_register_federation(&t.admin, &long_name, &stellar_address);
    assert_eq!(result, Err(Ok(StreamError::InvalidParameter)));

    let empty_name = String::from_str(&t.env, "");
    let empty = c.try_register_federation(&t.admin, &empty_name, &stellar_address);
    assert_eq!(empty, Err(Ok(StreamError::InvalidParameter)));
}

#[test]
fn test_issue_522_rejects_over_long_stream_tag() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1, 0, 0, None, None),
    );

    let long_tag = Some(String::from_str(
        &t.env,
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789",
    ));
    let result = c.try_set_stream_tag(&stream_id, &t.sender, &long_tag);
    assert_eq!(result, Err(Ok(StreamError::InvalidParameter)));

    // A normal-length tag is still accepted.
    let ok_tag = Some(String::from_str(&t.env, "payroll"));
    assert!(c.try_set_stream_tag(&stream_id, &t.sender, &ok_tag).is_ok());
}

#[test]
fn test_issue_522_rejects_bps_above_one_hundred_percent() {
    let t = setup();
    let c = client(&t);

    assert_eq!(
        c.try_set_token_fee_tier(&t.admin, &t.token_id, &10_001u32),
        Err(Ok(StreamError::InvalidDuration))
    );
    assert_eq!(
        c.try_set_protocol_fee(&10_001u32),
        Err(Ok(StreamError::InvalidDuration))
    );
    assert_eq!(
        c.try_set_slippage_params(&t.sender, &0u64, &1i128, &10_001u32),
        Err(Ok(StreamError::InvalidSlippage))
    );
}
