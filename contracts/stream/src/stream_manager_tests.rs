//! Tests for Issue #467: restricted stream-manager delegation.
//!
//! A manager may pause, resume, and update the flow rate of a stream on the
//! sender's behalf, but — unlike the pre-existing generic `delegate` — can
//! never cancel/stop the stream or redirect its funds.

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient, StreamError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
    manager: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let manager = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&sender, &0u64);

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    TestEnv {
        env,
        contract,
        token,
        sender,
        recipient,
        manager,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract)
}

fn make_stream(t: &TestEnv, c: &SoroStreamContractClient, nonce: u64) -> u64 {
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_000,
        &1000,
        &0,
        &nonce,
        &false,
        &0u64,
        &false,
    )
}

// ── Set / revoke lifecycle ──────────────────────────────────────────────

#[test]
fn set_and_get_manager() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);

    assert_eq!(c.get_stream_manager(&stream_id), None);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);
    assert_eq!(c.get_stream_manager(&stream_id), Some(t.manager.clone()));
}

#[test]
fn revoke_manager_clears_rights() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);

    c.set_stream_manager(&t.sender, &stream_id, &t.manager);
    c.revoke_stream_manager(&t.sender, &stream_id);
    assert_eq!(c.get_stream_manager(&stream_id), None);

    // Former manager can no longer pause.
    let result = c.try_pause_stream(&stream_id, &t.manager, &None);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));
}

#[test]
fn only_sender_can_set_or_revoke_manager() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    let not_sender = Address::generate(&t.env);

    let result = c.try_set_stream_manager(&not_sender, &stream_id, &t.manager);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));

    c.set_stream_manager(&t.sender, &stream_id, &t.manager);
    let result = c.try_revoke_stream_manager(&not_sender, &stream_id);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));
}

// ── Manager CAN pause / resume / update rate ────────────────────────────

#[test]
fn manager_can_pause_and_resume() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = make_stream(&t, &c, 0);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    c.pause_stream(&stream_id, &t.manager, &None);
    assert_eq!(
        c.get_stream(&stream_id).status,
        crate::StreamStatus::Paused
    );

    t.env.ledger().set_timestamp(100);
    c.resume_stream(&stream_id, &t.manager);
    assert_eq!(
        c.get_stream(&stream_id).status,
        crate::StreamStatus::Active
    );
}

#[test]
fn manager_can_update_stream_rate() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = make_stream(&t, &c, 0);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    // Original flow_rate = 1_000_000 / 1000 = 1000/sec.
    c.update_stream_rate(&stream_id, &t.manager, &2000i128);
    assert_eq!(c.get_stream(&stream_id).flow_rate, 2000);
}

// ── Manager CANNOT cancel, stop, or redirect ────────────────────────────

#[test]
fn manager_cannot_cancel_stream() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    let result = c.try_cancel_stream(&stream_id, &t.manager);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn manager_cannot_stop_stream() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    let result = c.try_stop_stream(&stream_id, &t.manager);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn manager_cannot_partial_cancel_stream() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    let result = c.try_partial_cancel_stream(&stream_id, &t.manager, &100_000i128);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

// ── Non-manager, non-sender is blocked from manager-gated actions ───────

#[test]
fn stranger_cannot_pause_or_update_rate() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    let stranger = Address::generate(&t.env);

    assert_eq!(
        c.try_pause_stream(&stream_id, &stranger, &None),
        Err(Ok(StreamError::NotSender))
    );
    assert_eq!(
        c.try_update_stream_rate(&stream_id, &stranger, &2000i128),
        Err(Ok(StreamError::NotSender))
    );
}

// ── Setting a manager doesn't touch the separate legacy `delegate` ──────

#[test]
fn manager_is_independent_of_delegate() {
    let t = setup();
    let c = client(&t);
    let stream_id = make_stream(&t, &c, 0);
    let delegate = Address::generate(&t.env);

    c.set_delegate(&t.sender, &stream_id, &delegate);
    c.set_stream_manager(&t.sender, &stream_id, &t.manager);

    assert_eq!(c.get_delegate(&stream_id), Some(delegate.clone()));
    assert_eq!(c.get_stream_manager(&stream_id), Some(t.manager.clone()));

    // The delegate (not manager) retains its own separate cancel rights.
    c.cancel_stream(&stream_id, &delegate);
    assert!(c.try_get_stream(&stream_id).is_err());
}
