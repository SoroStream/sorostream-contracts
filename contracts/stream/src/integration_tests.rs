
extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::types::StreamStatus;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct IntegrationEnv {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_integration() -> IntegrationEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Disable minimum duration for tests
    SoroStreamContractClient::new(&env, &contract).set_min_duration(&sender, &0u64);

    IntegrationEnv {
        env,
        contract,
        token,
        sender,
        recipient,
    }
}

fn client(ie: &IntegrationEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&ie.env, &ie.contract)
}

fn mint(ie: &IntegrationEnv, to: &Address, amount: &i128) {
    StellarAssetClient::new(&ie.env, &ie.token).mint(to, amount);
}

fn balance(ie: &IntegrationEnv, who: &Address) -> i128 {
    TokenClient::new(&ie.env, &ie.token).balance(who)
}

// ── Full lifecycle: mint → create → withdraw → verify balances ──────────────

#[test]
fn integration_full_lifecycle() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);

    mint(&ie, &ie.sender, &1_000_000);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );

    assert_eq!(balance(&ie, &ie.sender), 0);
    assert_eq!(balance(&ie, &ie.contract), 1_000_000);
    assert_eq!(balance(&ie, &ie.recipient), 0);

    // Partial withdraw at t=250
    ie.env.ledger().set_timestamp(250);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 250_000);
    assert_eq!(balance(&ie, &ie.contract), 750_000);

    // Another partial withdraw at t=600
    ie.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 600_000);
    assert_eq!(balance(&ie, &ie.contract), 400_000);

    // Final withdraw at t=1000 (stream ends, gets removed)
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);

    // Stream should be removed after completion (non-auto-renew)
    assert!(c.try_get_stream(&stream_id).is_err());
}

// ── Full lifecycle with cliff ───────────────────────────────────────────────

#[test]
fn integration_lifecycle_with_cliff() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    // flow_rate = 1_000_000 / 1000 = 1000 stroops/sec
    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &500,
        &0u64,
        &false, &0u64,
    );

    // Before cliff: claimable is zero
    ie.env.ledger().set_timestamp(300);
    assert_eq!(c.get_claimable(&stream_id), 0);

    // At cliff: tokens accrued from start become available
    // elapsed = 500 - 0 = 500, claimable = 1000 * 500 = 500_000
    ie.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 500_000);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 500_000);

    // Post-cliff linear vesting
    // elapsed = 750 - 500 = 250, claimable = 1000 * 250 = 250_000
    ie.env.ledger().set_timestamp(750);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 750_000);

    // Complete
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);
}

// ── Create → Cancel → Verify splits ────────────────────────────────────────

#[test]
fn integration_create_cancel_split() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false, &0u64,
    );

    ie.env.ledger().set_timestamp(400);
    c.cancel_stream(&stream_id, &ie.sender);

    // Recipient gets 400 seconds of flow (400 * 1000 = 400_000)
    assert_eq!(balance(&ie, &ie.recipient), 400_000);
    // Sender gets refund of unstreamed portion
    assert_eq!(balance(&ie, &ie.sender), 600_000);
    // Total conserved
    assert_eq!(
        balance(&ie, &ie.recipient) + balance(&ie, &ie.sender),
        1_000_000
    );
    // Stream removed after cancel (storage cleanup)
    assert!(c.try_get_stream(&stream_id).is_err());
}

// ── Top-up extends duration correctly ───────────────────────────────────────

#[test]
fn integration_topup_extends_and_pays() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &2_000_000);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false, &0u64,
    );

    // Top up at t=200 with 500_000 more
    ie.env.ledger().set_timestamp(200);
    c.top_up(&stream_id, &ie.sender, &ie.token, &500_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 1_500_000);
    assert_eq!(stream.end_time, 1500); // extended by 500_000/1000 = 500 seconds

    // Withdraw at original end_time: stream should still be active
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);

    // Withdraw at new end_time
    ie.env.ledger().set_timestamp(1500);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_500_000);
}

// ── Treasury/fees integration ───────────────────────────────────────────────

#[test]
fn integration_treasury_fees_on_batch_withdraw() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&500u32); // 5% fee (500 bps)
    c.set_treasury_address(&treasury);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false, &0u64,
    );

    ie.env.ledger().set_timestamp(500);

    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    let amounts = c.batch_withdraw(&stream_ids, &ie.recipient);

    // Claimable = 500 * 1000 = 500_000
    assert_eq!(amounts.get_unchecked(0), 500_000);

    // Fee = 500_000 * 500 / 10_000 = 25_000
    let fee = 500_000_i128 * 500 / 10_000;
    assert_eq!(fee, 25_000);

    // Recipient gets claimable - fee
    assert_eq!(balance(&ie, &ie.recipient), 500_000 - 25_000);
    // Treasury gets the fee
    assert_eq!(balance(&ie, &treasury), 25_000);
}

#[test]
fn integration_zero_fee_no_treasury_deduction() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    // fee is 0 by default

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );

    ie.env.ledger().set_timestamp(500);
    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids, &ie.recipient);

    // Full amount goes to recipient (no fee)
    assert_eq!(balance(&ie, &ie.recipient), 500_000);
}

// ── Batch create + batch withdraw lifecycle ─────────────────────────────────

#[test]
fn integration_batch_create_withdraw_lifecycle() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &5_000_000);

    let recipient2 = Address::generate(&ie.env);
    let recipients = soroban_sdk::vec![&ie.env, ie.recipient.clone(), recipient2.clone()];
    let amounts = soroban_sdk::vec![&ie.env, 1_000_000_i128, 2_000_000_i128];

    let lock_untils = soroban_sdk::vec![&ie.env, 0u64, 0u64];
    let stream_ids = c.batch_create_stream(
        &ie.sender,
        &recipients,
        &amounts,
        &ie.token,
        &1000,
        &false,
        &lock_untils,
    );

    assert_eq!(stream_ids.len(), 2);
    assert_eq!(balance(&ie, &ie.sender), 2_000_000); // 5M - 3M

    // Withdraw from first stream at t=500
    ie.env.ledger().set_timestamp(500);
    let ids1 = soroban_sdk::vec![&ie.env, stream_ids.get_unchecked(0)];
    c.batch_withdraw(&ids1, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 500_000); // 1M * 500/1000

    // Withdraw from second stream at t=500
    let ids2 = soroban_sdk::vec![&ie.env, stream_ids.get_unchecked(1)];
    c.batch_withdraw(&ids2, &recipient2);
    assert_eq!(balance(&ie, &recipient2), 1_000_000); // 2M * 500/1000
}

// ── Multiple streams, multiple recipients, interleaved operations ───────────

#[test]
fn integration_multi_stream_interleaved() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &3_000_000);

    let recipient2 = Address::generate(&ie.env);

    // Create two streams with different durations
    let s1 = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );
    let s2 = c.create_stream(
        &ie.sender,
        &recipient2,
        &ie.token,
        &2_000_000,
        &2000,
        &0,
        &1u64,
        &false,
        &0u64,
    );

    // t=500: withdraw from both
    ie.env.ledger().set_timestamp(500);
    c.withdraw(&s1, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 500_000);

    let ids2 = soroban_sdk::vec![&ie.env, s2];
    c.batch_withdraw(&ids2, &recipient2);
    assert_eq!(balance(&ie, &recipient2), 500_000); // 2M/2000 * 500

    // t=1000: s1 completes, s2 continues
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&s1, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);
    assert!(c.try_get_stream(&s1).is_err()); // removed after completion

    let ids2 = soroban_sdk::vec![&ie.env, s2];
    c.batch_withdraw(&ids2, &recipient2);
    assert_eq!(balance(&ie, &recipient2), 1_000_000);

    // t=2000: s2 completes
    ie.env.ledger().set_timestamp(2000);
    let ids2 = soroban_sdk::vec![&ie.env, s2];
    c.batch_withdraw(&ids2, &recipient2);
    assert_eq!(balance(&ie, &recipient2), 2_000_000);

    // Total: sender spent 3M, recipients received 3M
    assert_eq!(
        balance(&ie, &ie.recipient) + balance(&ie, &recipient2),
        3_000_000
    );
}

// ── Partial cancel integration ──────────────────────────────────────────────

#[test]
fn integration_partial_cancel_lifecycle() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );

    // At t=200, partial cancel reclaiming 300_000
    ie.env.ledger().set_timestamp(200);
    let new_stream_id = c.partial_cancel_stream(&stream_id, &ie.sender, &300_000);

    // Original stream is cancelled
    assert_eq!(c.get_stream(&stream_id).status, StreamStatus::Cancelled);

    // Recipient received earned amount (200 * 1000 = 200_000)
    assert_eq!(balance(&ie, &ie.recipient), 200_000);

    // Sender got 300_000 refund
    // Sender started with 0 (spent 1M on create), now has 300_000
    assert_eq!(balance(&ie, &ie.sender), 300_000);

    // New stream has remaining deposit
    let new_stream = c.get_stream(&new_stream_id);
    assert_eq!(new_stream.deposit, 500_000); // 1M - 200K earned - 300K cancelled
    assert_eq!(new_stream.status, StreamStatus::Active);
    assert_eq!(new_stream.flow_rate, 1000); // same flow rate

    // Withdraw from new stream at its end
    let new_duration = (500_000 / 1000) as u64; // 500 seconds
    ie.env.ledger().set_timestamp(200 + new_duration);
    c.withdraw(&new_stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 700_000); // 200K + 500K

    // Total conserved: 300K (sender) + 700K (recipient) = 1M
    assert_eq!(
        balance(&ie, &ie.sender) + balance(&ie, &ie.recipient),
        1_000_000
    );
}

// ── Auto-renew with SAC token ───────────────────────────────────────────────

#[test]
fn integration_auto_renew_with_sac() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&sender, &2_000_000);
    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);
    let token_client = TokenClient::new(&env, &token);
    env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &sender, &recipient, &token, &1_000_000, &1000, &0, &0u64, &true, &0u64,
    );

    // Complete first cycle
    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);
    assert_eq!(token_client.balance(&recipient), 1_000_000);

    // Stream should have auto-renewed
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);

    // Complete second cycle
    env.ledger().set_timestamp(2000);
    c.withdraw(&stream_id, &recipient);
    assert_eq!(token_client.balance(&recipient), 2_000_000);
}

// ── Query functions with SAC token ──────────────────────────────────────────

#[test]
fn integration_query_streams_by_sender_recipient() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &5_000_000);

    let r2 = Address::generate(&ie.env);

    let s1 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &0, &0u64, &false, &0u64,
    );
    let s2 = c.create_stream(
        &ie.sender, &r2, &ie.token, &1_000_000, &1000, &0, &1u64, &false, &0u64,
    );
    let s3 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &0, &2u64, &false, &0u64,
    );

    // By sender: should find all 3
    let sender_streams = c.get_streams_by_sender(&ie.sender, &0u32, &20u32);
    assert_eq!(sender_streams.len(), 3);

    // By recipient: should find 2 for ie.recipient
    let recip_streams = c.get_streams_by_recipient(&ie.recipient, &0u32, &20u32);
    assert_eq!(recip_streams.len(), 2);
    assert_eq!(recip_streams.get_unchecked(0).id, s1);
    assert_eq!(recip_streams.get_unchecked(1).id, s3);

    // Active streams filter
    ie.env.ledger().set_timestamp(1);
    c.cancel_stream(&s1, &ie.sender);

    let active = c.get_active_streams_by_sender(&ie.sender);
    assert_eq!(active.len(), 2);
    let active_ids: std::vec::Vec<u64> = (0..active.len())
        .map(|i| active.get_unchecked(i).id)
        .collect();
    assert!(active_ids.contains(&s2));
    assert!(active_ids.contains(&s3));
}

// ── Stats integration ───────────────────────────────────────────────────────

#[test]
fn integration_stats_reflect_lifecycle() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &5_000_000);

    c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &0, &0u64, &false, &0u64,
    );
    c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &2_000_000, &2000, &0, &1u64, &false, &0u64,
    );

    let stats = c.get_stats();
    assert_eq!(stats.total_streams, 2);
    assert_eq!(stats.active_streams, 2);
    assert_eq!(stats.total_volume, 3_000_000);
}

// ── Fee configuration edge cases ────────────────────────────────────────────

#[test]
fn integration_max_fee_boundary() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));

    // Max valid fee: 10_000 bps = 100%
    c.set_protocol_fee(&10_000u32);
    let (fee, _) = c.get_protocol_fee_info();
    assert_eq!(fee, 10_000);

    // Over max should fail
    let result = c.try_set_protocol_fee(&10_001u32);
    assert!(result.is_err());
}

#[test]
fn integration_fee_with_treasury_set() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&1000u32); // 10%
    c.set_treasury_address(&treasury);

    let (fee, treas) = c.get_protocol_fee_info();
    assert_eq!(fee, 1000);
    assert_eq!(treas, Some(treasury));
}

#[test]
fn integration_treasury_contract_balance_tracking() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    // Deploy treasury contract
    let treasury_id = ie.env.register(sorostream_treasury::TreasuryContract, ());
    let treasury_client = sorostream_treasury::TreasuryContractClient::new(&ie.env, &treasury_id);
    treasury_client.initialize(&admin);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&500u32); // 5%
    c.set_treasury_address(&treasury_id);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );

    ie.env.ledger().set_timestamp(500);

    // Before withdrawal, treasury balance is 0
    assert_eq!(treasury_client.get_balance(&ie.token), 0);

    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids, &ie.recipient);

    // Claimable = 500 * 1000 = 500_000
    // Fee = 500_000 * 500 / 10_000 = 25_000
    let fee = 500_000_i128 * 500 / 10_000;
    assert_eq!(fee, 25_000);

    // Treasury contract's tracked balance should equal the fee
    assert_eq!(treasury_client.get_balance(&ie.token), fee);

    // Treasury contract's actual token balance should also equal the fee
    let treasury_token_balance = TokenClient::new(&ie.env, &ie.token).balance(&treasury_id);
    assert_eq!(treasury_token_balance, fee);
}

#[test]
fn integration_treasury_contract_withdraw() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let destination = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    let treasury_id = ie.env.register(sorostream_treasury::TreasuryContract, ());
    let treasury_client = sorostream_treasury::TreasuryContractClient::new(&ie.env, &treasury_id);
    treasury_client.initialize(&admin);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&500u32);
    c.set_treasury_address(&treasury_id);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
    );

    ie.env.ledger().set_timestamp(500);
    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids, &ie.recipient);

    let fee = 500_000_i128 * 500 / 10_000;
    assert_eq!(treasury_client.get_balance(&ie.token), fee);

    // Admin withdraws from treasury via main contract
    c.withdraw_treasury(&ie.token, &fee, &destination);

    let dest_balance = TokenClient::new(&ie.env, &ie.token).balance(&destination);
    assert_eq!(dest_balance, fee);
    assert_eq!(treasury_client.get_balance(&ie.token), 0);
}
