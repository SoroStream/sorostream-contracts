#![cfg(test)]

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
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    TestEnv { env, contract_id, token_id, sender, recipient }
}

fn client(t: &TestEnv) -> SoroStreamContractClient {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

#[test]
fn test_create_stream_success() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);
    assert_eq!(stream_id, 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 100_000);
    assert_eq!(stream.flow_rate, 100);
    assert_eq!(stream.status, StreamStatus::Active);
}

#[test]
fn test_withdraw_partial() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000);
}

#[test]
fn test_withdraw_full() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 100_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Completed);
}

#[test]
fn test_cancel_stream_splits_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    // sender started with 1_000_000, deposited 100_000, gets 70_000 back = 970_000
    let sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    assert_eq!(recipient_bal, 30_000);
    assert_eq!(sender_bal, 970_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Cancelled);
}

#[test]
fn test_top_up_extends_duration() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);
    let stream_before = c.get_stream(&stream_id);

    c.top_up(&stream_id, &t.sender, &50_000);

    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.end_time, stream_before.end_time + 500);
    assert_eq!(stream_after.deposit, 150_000);
}

#[test]
fn test_auto_renew_restarts_on_completion() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint enough for initial deposit + one renewal
    StellarAssetClient::new(&env, &token_id).mint(&sender, &200_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&sender, &recipient, &token_id, &100_000, &1000, &true);

    // Withdraw at end_time — triggers auto-renew re-lock from sender
    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);
    assert_eq!(stream.last_withdraw_time, 1000);
}

#[test]
fn test_cannot_withdraw_if_not_recipient() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);
    let other = Address::generate(&t.env);

    let result = c.try_withdraw(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_cannot_cancel_if_not_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);
    let other = Address::generate(&t.env);

    let result = c.try_cancel_stream(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_zero_amount_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &0, &1000, &false);
    assert!(result.is_err());
}

#[test]
fn test_get_claimable_calculates_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &false);

    t.env.ledger().set_timestamp(250);
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 25_000);
}

#[test]
fn test_get_streams_by_sender_pagination() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let sender = Address::generate(&env);

    // Mint enough for 5 streams of 10_000 each
    StellarAssetClient::new(&env, &token_id).mint(&sender, &50_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);

    // Create 5 streams to different recipients
    for _ in 0..5u32 {
        let r = Address::generate(&env);
        c.create_stream(&sender, &r, &token_id, &10_000, &1000, &false);
    }

    // Page 1: first 3
    let page1 = c.get_streams_by_sender(&sender, &0, &3);
    assert_eq!(page1.len(), 3);
    assert_eq!(page1.get(0).unwrap().id, 0);
    assert_eq!(page1.get(2).unwrap().id, 2);

    // Page 2: next 3 (only 2 remain)
    let page2 = c.get_streams_by_sender(&sender, &3, &3);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get(0).unwrap().id, 3);
    assert_eq!(page2.get(1).unwrap().id, 4);

    // Out-of-bounds start returns empty
    let empty = c.get_streams_by_sender(&sender, &10, &5);
    assert_eq!(empty.len(), 0);

    // Limit is capped at 20: requesting 100 returns at most 5
    let all = c.get_streams_by_sender(&sender, &0, &100);
    assert_eq!(all.len(), 5);
}

#[test]
fn test_get_streams_by_recipient_pagination() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&Address::generate(&env), &0);

    // Two different senders each sending to the same recipient
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&s1, &30_000);
    StellarAssetClient::new(&env, &token_id).mint(&s2, &30_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);

    c.create_stream(&s1, &recipient, &token_id, &10_000, &1000, &false);
    c.create_stream(&s1, &recipient, &token_id, &10_000, &1000, &false);
    c.create_stream(&s2, &recipient, &token_id, &10_000, &1000, &false);

    let page = c.get_streams_by_recipient(&recipient, &0, &2);
    assert_eq!(page.len(), 2);

    let rest = c.get_streams_by_recipient(&recipient, &2, &10);
    assert_eq!(rest.len(), 1);
}
