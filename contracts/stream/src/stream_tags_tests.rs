//! Unit tests for issue #635: stream multi-tag support.
//!
//! Verifies that create_stream accepts optional tags (Vec<Bytes>, max 3, max 32
//! bytes each), stores them, and returns them via get_stream_tags.

use crate::{SoroStreamContract, SoroStreamContractClient, StreamError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Bytes, Env, Vec,
};

fn setup_env() -> (Env, Address, SoroStreamContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.ledger().set_timestamp(0);
    let contract_id = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);
    let client = SoroStreamContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.set_min_duration(&admin, &0u64);
    (env, admin, client)
}

fn make_token(env: &Env, admin: &Address) -> Address {
    let token_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    token_id
}

fn mint(env: &Env, token_id: &Address, to: &Address, amount: i128) {
    let asset_client = StellarAssetClient::new(env, token_id);
    asset_client.mint(to, &amount);
}

/// Helper: build minimal CreateStreamParams with optional tags.
fn params_with_tags(
    env: &Env,
    nonce: u64,
    tags: Option<soroban_sdk::Vec<Bytes>>,
) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds: 0,
        nonce,
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
        tags,
    }
}

/// A stream created with no tags returns an empty Vec from get_stream_tags.
#[test]
fn test_stream_tags_empty_when_none_provided() {
    let (env, admin, client) = setup_env();
    let token_id = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token_id, &sender, 10_000);

    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token_id,
        &10_000,
        &1_000,
        &false,
        &params_with_tags(&env, 1, None),
    );

    let tags = client.get_stream_tags(&stream_id);
    assert!(tags.is_empty(), "expected no tags when none were supplied");
}

/// A stream created with 3 tags stores and retrieves all three correctly.
#[test]
fn test_stream_with_3_tags_stores_and_retrieves_correctly() {
    let (env, admin, client) = setup_env();
    let token_id = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token_id, &sender, 10_000);

    // Build three tags: "payroll", "vesting", "subscription"
    let tag1 = Bytes::from_slice(&env, b"payroll");
    let tag2 = Bytes::from_slice(&env, b"vesting");
    let tag3 = Bytes::from_slice(&env, b"subscription");
    let mut tag_vec = Vec::new(&env);
    tag_vec.push_back(tag1.clone());
    tag_vec.push_back(tag2.clone());
    tag_vec.push_back(tag3.clone());

    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token_id,
        &10_000,
        &1_000,
        &false,
        &params_with_tags(&env, 2, Some(tag_vec)),
    );

    let stored = client.get_stream_tags(&stream_id);
    assert_eq!(stored.len(), 3, "expected 3 tags to be stored");
    assert_eq!(stored.get(0).unwrap(), tag1);
    assert_eq!(stored.get(1).unwrap(), tag2);
    assert_eq!(stored.get(2).unwrap(), tag3);
}

/// Supplying more than 3 tags returns TooManyTags error.
#[test]
fn test_stream_too_many_tags_rejected() {
    let (env, admin, client) = setup_env();
    let token_id = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token_id, &sender, 10_000);

    let mut tag_vec = Vec::new(&env);
    for b in [b"a".as_ref(), b"b", b"c", b"d"] {
        tag_vec.push_back(Bytes::from_slice(&env, b));
    }

    let result = client.try_create_stream(
        &sender,
        &recipient,
        &token_id,
        &10_000,
        &1_000,
        &false,
        &params_with_tags(&env, 3, Some(tag_vec)),
    );

    assert_eq!(
        result,
        Err(Ok(StreamError::TooManyTags)),
        "expected TooManyTags when more than 3 tags are supplied"
    );
}

/// A tag exceeding 32 bytes returns TagTooLong error.
#[test]
fn test_stream_tag_too_long_rejected() {
    let (env, admin, client) = setup_env();
    let token_id = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token_id, &sender, 10_000);

    // 33-byte tag
    let long_tag = Bytes::from_slice(&env, &[b'x'; 33]);
    let mut tag_vec = Vec::new(&env);
    tag_vec.push_back(long_tag);

    let result = client.try_create_stream(
        &sender,
        &recipient,
        &token_id,
        &10_000,
        &1_000,
        &false,
        &params_with_tags(&env, 4, Some(tag_vec)),
    );

    assert_eq!(
        result,
        Err(Ok(StreamError::TagTooLong)),
        "expected TagTooLong when a tag exceeds 32 bytes"
    );
}

/// A tag of exactly 32 bytes is accepted.
#[test]
fn test_stream_tag_exactly_32_bytes_accepted() {
    let (env, admin, client) = setup_env();
    let token_id = make_token(&env, &admin);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token_id, &sender, 10_000);

    let exact_tag = Bytes::from_slice(&env, &[b'z'; 32]);
    let mut tag_vec = Vec::new(&env);
    tag_vec.push_back(exact_tag.clone());

    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token_id,
        &10_000,
        &1_000,
        &false,
        &params_with_tags(&env, 5, Some(tag_vec)),
    );

    let stored = client.get_stream_tags(&stream_id);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.get(0).unwrap(), exact_tag);
}
