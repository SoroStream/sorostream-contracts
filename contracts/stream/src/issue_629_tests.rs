//! Tests for issue #629: Metadata blob size validation.
//!
//! Validates that:
//! - Metadata blobs at or below 256 bytes are accepted.
//! - Oversized blobs are rejected with `MetadataTooLong`.
//! - A valid blob can be retrieved after being set.
//! - Updating with an oversized blob does not overwrite a previously valid blob.

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::StellarAssetClient,
    Address, Bytes, Env,
};

struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_629() -> Setup {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.ledger().set_timestamp(0);

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);

    Setup { env, contract, token, sender, recipient }
}

fn make_stream(s: &Setup, nonce: u64) -> u64 {
    let c = SoroStreamContractClient::new(&s.env, &s.contract);
    c.create_stream(
        &s.sender,
        &s.recipient,
        &s.token,
        &100_000,
        &1000,
        &false,
        &0,
        &CreateStreamParams {
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
        },
    )
    .unwrap()
}

/// A metadata blob of exactly 256 bytes is accepted (boundary value).
#[test]
fn test_629_max_size_blob_accepted() {
    let s = setup_629();
    let stream_id = make_stream(&s, 1);
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let exactly_256 = Bytes::from_slice(&s.env, &[0xab_u8; 256]);
    assert!(
        c.update_metadata(&s.sender, &stream_id, &exactly_256).is_ok(),
        "256-byte blob must be accepted"
    );
    assert_eq!(c.get_metadata(&stream_id), Some(exactly_256));
}

/// A metadata blob of 257 bytes (one byte over the limit) is rejected.
#[test]
fn test_629_oversized_blob_rejected() {
    let s = setup_629();
    let stream_id = make_stream(&s, 2);
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let oversized = Bytes::from_slice(&s.env, &[0xff_u8; 257]);
    assert_eq!(
        c.update_metadata(&s.sender, &stream_id, &oversized),
        Err(crate::StreamError::MetadataTooLong),
        "257-byte blob must be rejected with MetadataTooLong"
    );
}

/// An oversized update does not overwrite the previously stored valid blob.
#[test]
fn test_629_oversized_update_does_not_overwrite_valid_blob() {
    let s = setup_629();
    let stream_id = make_stream(&s, 3);
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // Store a valid 32-byte blob first.
    let valid_blob = Bytes::from_slice(&s.env, b"valid-metadata-payload-32-bytes!");
    c.update_metadata(&s.sender, &stream_id, &valid_blob).unwrap();

    // Attempt an oversized update.
    let oversized = Bytes::from_slice(&s.env, &[0xcc_u8; 300]);
    let _ = c.update_metadata(&s.sender, &stream_id, &oversized);

    // The original blob must still be retrievable.
    assert_eq!(
        c.get_metadata(&stream_id),
        Some(valid_blob),
        "valid blob must survive a rejected oversized update attempt"
    );
}

/// An empty metadata blob (0 bytes) is accepted.
#[test]
fn test_629_empty_blob_accepted() {
    let s = setup_629();
    let stream_id = make_stream(&s, 4);
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let empty = Bytes::from_slice(&s.env, &[]);
    assert!(
        c.update_metadata(&s.sender, &stream_id, &empty).is_ok(),
        "empty blob must be accepted"
    );
}

/// A non-sender caller cannot update metadata.
#[test]
fn test_629_non_sender_cannot_update_metadata() {
    let s = setup_629();
    let stream_id = make_stream(&s, 5);
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let blob = Bytes::from_slice(&s.env, b"payload");
    // recipient is not the sender — must be rejected
    let result = c.update_metadata(&s.recipient, &stream_id, &blob);
    assert!(result.is_err(), "non-sender must be rejected");
}
