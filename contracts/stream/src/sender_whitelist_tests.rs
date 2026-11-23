//! Tests for Issue #469: sender (stream-creation) whitelist.
//!
//! Distinct from the pre-existing recipient whitelist: this gates who may
//! *create* streams, not who may receive them.

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{
    testutils::Address as _,
    token::StellarAssetClient,
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract: Address,
    token: Address,
    admin: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&sender, &0u64);

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    TestEnv {
        env,
        contract,
        token,
        admin,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract)
}

macro_rules! try_create {
    ($t:expr, $c:expr, $nonce:expr) => {
        $c.try_create_stream(
            &$t.sender,
            &$t.recipient,
            &$t.token,
            &1_000_000,
            &1000,
            &0,
            &$nonce,
            &false,
            &0u64,
            &false,
        )
    };
}

// ── Disabled by default: existing permissionless behaviour is preserved ────

#[test]
fn whitelist_disabled_by_default_allows_any_sender() {
    let t = setup();
    let c = client(&t);

    assert!(!c.is_sender_whitelist_enabled());
    let stream_id = try_create!(t, c, 0u64).unwrap().unwrap();
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id);
}

// ── Enabling enforcement blocks non-whitelisted senders ─────────────────────

#[test]
fn whitelist_enabled_blocks_non_whitelisted_sender() {
    let t = setup();
    let c = client(&t);

    c.set_sender_whitelist_enabled(&t.admin, &true);
    assert!(c.is_sender_whitelist_enabled());

    let result = try_create!(t, c, 0u64);
    assert_eq!(result, Err(Ok(crate::StreamError::SenderNotWhitelisted)));
}

#[test]
fn whitelist_enabled_allows_whitelisted_sender() {
    let t = setup();
    let c = client(&t);

    c.set_sender_whitelist_enabled(&t.admin, &true);
    c.add_sender_to_whitelist(&t.admin, &t.sender);
    assert!(c.is_sender_whitelisted(&t.sender));

    let stream_id = try_create!(t, c, 0u64).unwrap().unwrap();
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id);
}

#[test]
fn removed_sender_is_blocked_again() {
    let t = setup();
    let c = client(&t);

    c.set_sender_whitelist_enabled(&t.admin, &true);
    c.add_sender_to_whitelist(&t.admin, &t.sender);
    assert!(try_create!(t, c, 0u64).unwrap().is_ok());

    c.remove_sender_from_whitelist(&t.admin, &t.sender);
    assert!(!c.is_sender_whitelisted(&t.sender));

    let result = try_create!(t, c, 1u64);
    assert_eq!(result, Err(Ok(crate::StreamError::SenderNotWhitelisted)));
}

#[test]
fn recipient_whitelist_toggle_does_not_enable_sender_whitelist() {
    let t = setup();
    let c = client(&t);

    // Enabling the pre-existing (unrelated) recipient whitelist must not
    // implicitly enable or otherwise affect sender-whitelist enforcement.
    c.set_whitelist_enabled(&t.admin, &true);

    assert!(!c.is_sender_whitelist_enabled());
    assert!(!c.is_sender_whitelisted(&t.sender));
}
