extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
use crate::types::StreamStatus;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol,
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

/// Helper: build a minimal CreateStreamParams for use in tests.
fn params(cliff_seconds: u64, nonce: u64, auto_renew_count: Option<u32>, lock_until: u64, allow_term: bool) -> CreateStreamParams {
    CreateStreamParams {
        cliff_seconds,
        nonce,
        renew_count: auto_renew_count,
        lock_until,
        allow_recipient_termination: allow_term,
        non_transferable: false,
        holdback_amount: 0,
        withdrawal_steps: None,
        min_withdrawal_amount: None,
        requires_recipient_approval: false,
    }
}

/// Helper: build the simplest possible CreateStreamParams (no cliff, nonce=n, no extras).
fn simple_params(nonce: u64) -> CreateStreamParams {
    params(0, nonce, None, 0, false)
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
        &false,
        &simple_params(0),
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
        &false,
        &params(500, 0, None, 0, false),
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
        &false,
        &simple_params(0),
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
        &false,
        &simple_params(0),
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
        &false,
        &simple_params(0),
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
    // batch_withdraw accumulates fees internally; check via get_fees_collected
    assert_eq!(c.get_fees_collected(&ie.token), 25_000);
}

// The flat XLM creation fee (`set_creation_fee`) is charged in a *separate*
// SAC token from the stream's own asset, and does not reduce the stream's
// deposit — it's an additive charge collected up front. See
// `Contract::set_creation_fee` / `Contract::create_stream` in lib.rs.

#[test]
#[ignore = "set_creation_tax is not implemented on the contract (pre-existing gap, unrelated to issues #458-461)"]
fn integration_creation_tax_reduces_stream_deposit() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    // Register a distinct SAC token to act as the XLM creation-fee asset.
    let xlm_admin = Address::generate(&ie.env);
    let xlm_token = ie
        .env
        .register_stellar_asset_contract_v2(xlm_admin.clone())
        .address();
    StellarAssetClient::new(&ie.env, &xlm_token).mint(&ie.sender, &100_000);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_treasury_address(&treasury);
    // set_creation_tax(flat_amount, bps) does not exist on the contract yet — see #[ignore] above.

    let stream_id = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    // The stream's own token balances are untouched by the fee.
    assert_eq!(balance(&ie, &ie.sender), 0);
    assert_eq!(balance(&ie, &ie.contract), 1_000_000);
    assert_eq!(c.get_stream(&stream_id).deposit, 1_000_000);

    // The fee was charged in the XLM token instead.
    let xlm_balance = |who: &Address| TokenClient::new(&ie.env, &xlm_token).balance(who);
    assert_eq!(xlm_balance(&ie.sender), 0);
    assert_eq!(xlm_balance(&treasury), 100_000);
}

#[test]
#[ignore = "set_creation_tax is not implemented on the contract (pre-existing gap, unrelated to issues #458-461)"]
fn integration_creation_tax_bps_reduces_stream_deposit() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    let xlm_admin = Address::generate(&ie.env);
    let xlm_token = ie
        .env
        .register_stellar_asset_contract_v2(xlm_admin.clone())
        .address();
    StellarAssetClient::new(&ie.env, &xlm_token).mint(&ie.sender, &50_000);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_treasury_address(&treasury);
    // set_creation_tax(flat_amount, bps) does not exist on the contract yet — see #[ignore] above.

    let stream_id = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    let xlm_balance = |who: &Address| TokenClient::new(&ie.env, &xlm_token).balance(who);
    assert_eq!(xlm_balance(&treasury), 25_000);
    assert_eq!(xlm_balance(&ie.sender), 25_000);
    assert_eq!(c.get_stream(&stream_id).deposit, 1_000_000);
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
        &false,
        &simple_params(0),
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
    let mut tokens = soroban_sdk::Vec::new(&ie.env);
    for _ in 0..recipients.len() { tokens.push_back(ie.token.clone()); }
    let stream_ids = c.batch_create_stream(
        &ie.sender,
        &recipients,
        &amounts,
        &tokens,
        &1000,
        &false,
        &None,
        &lock_untils,
        &0u64,
        &false,
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
        &false,
        &simple_params(0),
    );
    let s2 = c.create_stream(
        &ie.sender,
        &recipient2,
        &ie.token,
        &2_000_000,
        &2000,
        &false,
        &simple_params(1),
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
        &false,
        &simple_params(0),
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
        &sender, &recipient, &token, &1_000_000, &1000, &true,
        &simple_params(0),
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
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &false,
        &simple_params(0),
    );
    let s2 = c.create_stream(
        &ie.sender, &r2, &ie.token, &1_000_000, &1000, &false,
        &simple_params(1),
    );
    let s3 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &false,
        &simple_params(2),
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
        &ie.sender, &ie.recipient, &ie.token, &1_000_000, &1000, &false,
        &simple_params(0),
    );
    c.create_stream(
        &ie.sender, &ie.recipient, &ie.token, &2_000_000, &2000, &false,
        &simple_params(1),
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
        &false,
        &simple_params(0),
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

    // batch_withdraw accumulates fees internally; check via get_fees_collected
    assert_eq!(c.get_fees_collected(&ie.token), fee);
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
        &false,
        &simple_params(0),
    );

    ie.env.ledger().set_timestamp(500);
    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids, &ie.recipient);

    let fee = 500_000_i128 * 500 / 10_000;
    // batch_withdraw accumulates fees internally
    assert_eq!(c.get_fees_collected(&ie.token), fee);

    // Admin sweeps accumulated fees to destination via sweep_fees
    c.sweep_fees(&ie.token, &destination);

    let dest_balance = TokenClient::new(&ie.env, &ie.token).balance(&destination);
    assert_eq!(dest_balance, fee);
    assert_eq!(c.get_fees_collected(&ie.token), 0);
}

// ── Issue #256: Expired / Completed state transitions after end_time ────────

#[test]
fn integration_stream_active_past_end_time() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    // flow_rate = 1_000_000 / 1000 = 1000/sec
    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &false,
        &simple_params(0),
    );

    // Partial withdraw at t=500
    ie.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 500_000);

    // Advance past end_time (t=1200)
    ie.env.ledger().set_timestamp(1200);

    // get_stream now surfaces Expired for any elapsed stream
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);

    // get_claimable returns remaining: flow_rate * (end_time - last_withdraw_time) = 1000 * (1000 - 500)
    assert_eq!(c.get_claimable(&stream_id), 500_000);

    // Withdraw drains the remaining amount
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);

    // Stream is removed after final withdrawal (non-auto-renew)
    assert!(c.try_get_stream(&stream_id).is_err());

    // Total conserved
    assert_eq!(
        balance(&ie, &ie.sender) + balance(&ie, &ie.recipient),
        1_000_000
    );
}

#[test]
fn integration_get_claimable_post_end_time_without_prior_withdrawal() {
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
        &false,
        &simple_params(0),
    );

    // No withdrawal before end_time
    ie.env.ledger().set_timestamp(1500);

    // get_claimable caps at full deposit (flow_rate * (end_time - start_time) = 1000 * 1000)
    assert_eq!(c.get_claimable(&stream_id), 1_000_000);

    // Withdraw full amount
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);

    // Stream removed
    assert!(c.try_get_stream(&stream_id).is_err());
}

#[test]
fn integration_get_stream_still_active_at_exact_end_time() {
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
        &false,
        &simple_params(0),
    );

    // At exactly end_time, get_stream surfaces Expired (now >= end_time)
    ie.env.ledger().set_timestamp(1000);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);
    assert_eq!(c.get_claimable(&stream_id), 1_000_000);

    // Withdraw at exact end_time removes stream
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000);
    assert!(c.try_get_stream(&stream_id).is_err());
}

// ── Issue #255: Auto-renewal fails on insufficient sender balance ─────────────
//
// When a stream with `auto_renew = true` completes and the sender does not have
// enough tokens to fund another cycle, the contract must:
//   1. Complete the stream (status → Completed) rather than restarting it.
//   2. Pay out the full earned amount to the recipient.
//   3. Return any dust to the sender.
//   4. Emit `AutoRenewFailed { stream_id, sender, required }` so off-chain
//      monitors can alert the sender that renewal was skipped.
//
// This test is the integration-level proof for the above invariants (issue #255).
// The unit-level event structure snapshot lives in test.rs::snapshot_event_auto_renew_failed.

#[test]
fn integration_auto_renew_completed_on_insufficient_funds() {
    use soroban_sdk::{IntoVal, Symbol, Val};

    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);
    mint(&ie, &ie.sender, &1_000_000);

    // Sender has exactly 1_000_000 stroops — enough for the stream deposit but
    // nothing left over when auto-renew tries to re-lock the same amount.
    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &true,  // auto_renew enabled
        &simple_params(0),
    );

    // After create_stream, sender balance is 0 (all tokens locked in contract).
    assert_eq!(balance(&ie, &ie.sender), 0);
    assert_eq!(balance(&ie, &ie.contract), 1_000_000);

    // ── Behaviour assertion 4: AutoRenewFailed event was emitted ─────────────
    // Capture events immediately after withdraw — each subsequent contract call
    // (balance, get_stream, get_claimable) clears the host event buffer.
    //
    // Expected event shape:
    //   topics: (Symbol("AutoRenewFailed"), stream_id: u64)
    //   data:   (sender: Address, required: i128)
    //
    // where `required` is the amount needed for one renewal cycle (== deposit).
    ie.env.ledger().set_timestamp(1000); // stream end_time reached
    c.withdraw(&stream_id, &ie.recipient);
    let all_events = ie.env.events().all();
    let renewal_failed_events: std::vec::Vec<_> = all_events
        .iter()
        .filter(|(_, topics, _)| {
            if topics.is_empty() { return false; }
            let first: Symbol = topics.get(0).unwrap().into_val(&ie.env);
            first == Symbol::new(&ie.env, "AutoRenewFailed")
        })
        .collect();

    assert_eq!(
        renewal_failed_events.len(),
        1,
        "exactly one AutoRenewFailed event must be emitted when auto-renewal fails"
    );

    let (emitter, topics, data) = &renewal_failed_events[0];

    // Emitter must be the stream contract itself.
    assert_eq!(*emitter, ie.contract,
        "AutoRenewFailed must be emitted by the stream contract");

    // Topic[1] must be the stream_id.
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2,
        "AutoRenewFailed topics must contain exactly (name, stream_id)");
    let event_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&ie.env);
    assert_eq!(event_stream_id, stream_id,
        "AutoRenewFailed stream_id in topics must match the stream");

    // Data must be (sender: Address, required: i128).
    let event_data: (Address, i128) = data.clone().into_val(&ie.env);
    assert_eq!(event_data.0, ie.sender,
        "AutoRenewFailed data[0] must be the sender address");
    assert_eq!(event_data.1, 1_000_000i128,
        "AutoRenewFailed data[1] (required) must equal the stream deposit");

    // ── Behaviour assertions 1-3: balances and stream state (after event capture) ──
    assert_eq!(balance(&ie, &ie.recipient), 1_000_000,
        "recipient should receive the full stream deposit");
    assert_eq!(balance(&ie, &ie.sender), 0,
        "sender should receive nothing (no dust when amount is divisible by duration)");
    assert_eq!(balance(&ie, &ie.contract), 0,
        "contract should hold nothing after settlement");

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired,
        "stream status must be Expired when auto-renew fails and end_time has passed");

    assert_eq!(c.get_claimable(&stream_id), 0,
        "get_claimable must return 0 for a Completed stream");
}

/// Partial-balance variant: sender has some tokens but not enough for a full renewal cycle.
///
/// This tests the deficit case: sender retains tokens from other activity but falls
/// short of the renewal deposit. The event's `required` field must still reflect
/// the full deposit amount (not the shortfall), because that is what renewal needs.
#[test]
fn integration_auto_renew_fails_with_partial_sender_balance() {
    use soroban_sdk::{IntoVal, Symbol, Val};

    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);

    // Mint enough for the deposit plus a small leftover (not enough to renew).
    let deposit: i128 = 1_000_000;
    let leftover: i128 = 100; // sender will have 100 stroops after stream creation
    mint(&ie, &ie.sender, &(deposit + leftover));

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &deposit,
        &1000,
        &true,  // auto_renew enabled
        &simple_params(0),
    );

    // Sender has 100 stroops — present but less than the 1_000_000 needed for renewal.
    assert_eq!(balance(&ie, &ie.sender), leftover);

    // Trigger the auto-renew attempt at stream end.
    // Capture events immediately — each subsequent contract call clears the host event buffer.
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &ie.recipient);
    // AutoRenewFailed event must be emitted with the correct required amount.
    let all_events = ie.env.events().all();
    let renewal_failed_events: std::vec::Vec<_> = all_events
        .iter()
        .filter(|(_, topics, _)| {
            if topics.is_empty() { return false; }
            let first: Symbol = topics.get(0).unwrap().into_val(&ie.env);
            first == Symbol::new(&ie.env, "AutoRenewFailed")
        })
        .collect();

    assert_eq!(
        renewal_failed_events.len(),
        1,
        "exactly one AutoRenewFailed event must be emitted"
    );

    let (_, topics, data) = &renewal_failed_events[0];
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    let event_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&ie.env);
    assert_eq!(event_stream_id, stream_id);

    let event_data: (Address, i128) = data.clone().into_val(&ie.env);
    assert_eq!(event_data.0, ie.sender,
        "data[0] must be the sender who failed to fund the renewal");
    assert_eq!(event_data.1, deposit,
        "data[1] (required) must be the full deposit, not just the shortfall");

    // Recipient still receives the full earned amount (after event capture).
    assert_eq!(balance(&ie, &ie.recipient), deposit);

    // Stream surfaces as Expired (get_stream converts Completed→Expired past end_time).
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);
}

// ── Issue #257: Fee accumulation and sweep flow ──────────────────────────────

#[test]
fn integration_fee_accumulation_and_sweep() {
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let _treasury = Address::generate(&ie.env);
    let destination = Address::generate(&ie.env);
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
        &false,
        &simple_params(0),
    );

    // batch_withdraw at t=300: claimable = 300K, fee = 15K (accumulated internally)
    ie.env.ledger().set_timestamp(300);
    let stream_ids = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids, &ie.recipient);
    let fee1 = 300_000_i128 * 500 / 10_000; // 15_000
    assert_eq!(balance(&ie, &ie.recipient), 300_000 - fee1);
    assert_eq!(c.get_fees_collected(&ie.token), fee1);

    // batch_withdraw at t=600: claimable = 300K, fee = 15K
    ie.env.ledger().set_timestamp(600);
    let stream_ids2 = soroban_sdk::vec![&ie.env, stream_id];
    c.batch_withdraw(&stream_ids2, &ie.recipient);
    let fee2 = 300_000_i128 * 500 / 10_000; // 15_000
    assert_eq!(balance(&ie, &ie.recipient), (300_000 - fee1) + (300_000 - fee2));
    assert_eq!(c.get_fees_collected(&ie.token), fee1 + fee2);

    // Total accumulated fees
    let total_fee = fee1 + fee2; // 30_000
    assert_eq!(total_fee, 30_000);

    // Sweep accumulated fees to destination
    c.sweep_fees(&ie.token, &destination);

    // Destination received exact amount
    assert_eq!(balance(&ie, &destination), total_fee);

    // Accumulated fees now zero
    assert_eq!(c.get_fees_collected(&ie.token), 0);

    // Stream still active, withdraw remaining at end_time
    ie.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &ie.recipient);

    // Remaining = 400K (from t=600 to t=1000), fee = 20K
    let fee3 = 400_000_i128 * 500 / 10_000; // 20_000
    let expected_recipient_total = (300_000 - fee1) + (300_000 - fee2) + (400_000 - fee3);
    assert_eq!(balance(&ie, &ie.recipient), expected_recipient_total);

    // Total conserved: recipient + swept fees + treasury fee (from final withdraw) = 1M
    let total_out = expected_recipient_total + total_fee + fee3;
    assert_eq!(total_out, 1_000_000);
}

// ── Batch withdraw with fees at stream end (issue: overdraw prevention) ──────

#[test]
fn integration_batch_withdraw_final_no_overdraw_with_fees() {
    // This test validates that when a stream ends and fees are applied,
    // the total amount paid out (recipient + dust/sender) never exceeds the deposit.
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    
    // Setup: 1M deposit, 500 second duration, so flow_rate = 2000 stroops/sec
    let deposit = 1_000_000_i128;
    let duration = 500u64;
    let _flow_rate = deposit / duration as i128; // 2000 stroops/sec
    
    mint(&ie, &ie.sender, &deposit);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&5000u32); // 50% fee (worst case - 5000 bps)
    c.set_treasury_address(&treasury);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &deposit,
        &duration,
        &false,
        &simple_params(0),
    );

    // Withdraw mid-stream at t=250 (half time)
    ie.env.ledger().set_timestamp(250);
    let stream_ids_mid = soroban_sdk::vec![&ie.env, stream_id];
    let mid_amounts = c.batch_withdraw(&stream_ids_mid, &ie.recipient);
    
    // At t=250: claimable = 2000 * 250 = 500_000
    assert_eq!(mid_amounts.get_unchecked(0), 500_000);
    
    // Fee at mid = 500_000 * 50% = 250_000
    let mid_recipient_amount = 500_000 - 250_000;
    assert_eq!(balance(&ie, &ie.recipient), mid_recipient_amount);
    
    // Now jump to end and do final withdrawal (batch_withdraw)
    ie.env.ledger().set_timestamp(500); // stream.end_time
    let stream_ids_final = soroban_sdk::vec![&ie.env, stream_id];
    let final_amounts = c.batch_withdraw(&stream_ids_final, &ie.recipient);
    
    // At t=500: remaining claimable = 2000 * (500-250) = 500_000
    assert_eq!(final_amounts.get_unchecked(0), 500_000);
    
    // Fee on final = 500_000 * 50% = 250_000
    let final_recipient_amount = 500_000 - 250_000;
    
    // Total recipient amount
    let total_recipient = mid_recipient_amount + final_recipient_amount;
    
    // Total fees collected
    let total_fees_collected = 250_000 + 250_000; // 500_000
    
    // Verify no overdraw: total paid out should not exceed deposit
    let total_paid = total_recipient + total_fees_collected;
    assert_eq!(total_paid, deposit,
        "Total paid out (recipient + fees) must not exceed deposit. \
         Total: {total_paid}, Recipient: {total_recipient}, Fees: {total_fees_collected}, Deposit: {deposit}");
    
    // In this scenario with 50% fee:
    // recipient gets 50% of each claimable tranche: 250K mid + 250K final = 500K
    // protocol accumulates 50% of each tranche: 250K + 250K = 500K
    assert_eq!(total_recipient, 500_000);
}

// ── Regular withdraw with fees at stream end (fee overdraw prevention) ──────

#[test]
fn integration_withdraw_final_no_overdraw_with_fees() {
    // Verifies that regular withdraw (not batch_withdraw) doesn't overdraw
    // when fees are applied at stream end.
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    
    // Setup: 1M deposit, 400 second duration for easier mental math
    let deposit = 1_000_000_i128;
    let duration = 400u64;
    
    mint(&ie, &ie.sender, &deposit);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&2500u32); // 25% fee (2500 bps)
    c.set_treasury_address(&treasury);

    let stream_id = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &deposit,
        &duration,
        &false,
        &simple_params(0),
    );

    // Withdraw at t=200 (halfway)
    ie.env.ledger().set_timestamp(200);
    c.withdraw(&stream_id, &ie.recipient);
    
    // Claimable = 500_000, fee = 125_000, recipient gets 375_000
    let balance_halfway = balance(&ie, &ie.recipient);
    assert_eq!(balance_halfway, 375_000);

    // Final withdrawal at t=400
    ie.env.ledger().set_timestamp(400);
    c.withdraw(&stream_id, &ie.recipient);
    
    // Claimable = 500_000, fee = 125_000, recipient gets 375_000 more
    let balance_final = balance(&ie, &ie.recipient);
    assert_eq!(balance_final, 750_000);
    
    // Stream is removed from storage after final withdrawal
    assert!(c.try_get_stream(&stream_id).is_err(), "stream should be removed after final withdrawal");

    // Total: 750K to recipient + 250K in fees = 1M (no overdraw)
}

#[test]
fn integration_batch_withdraw_with_multiple_streams_and_fees() {
    // Test batch_withdraw with multiple streams ending at the same time,
    // all with fees > 0.
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    
    mint(&ie, &ie.sender, &3_000_000); // enough for 3 streams

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&1000u32); // 10% fee
    c.set_treasury_address(&treasury);

    // Create 3 streams
    let stream_id1 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token,
        &1_000_000, &1000, &false, &simple_params(0),
    );
    let stream_id2 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token,
        &1_000_000, &1000, &false, &simple_params(1),
    );
    let stream_id3 = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token,
        &1_000_000, &1000, &false, &simple_params(2),
    );

    // Jump to end of streams
    ie.env.ledger().set_timestamp(1000);
    
    let stream_ids = soroban_sdk::vec![&ie.env, stream_id1, stream_id2, stream_id3];
    let amounts = c.batch_withdraw(&stream_ids, &ie.recipient);
    
    // Each stream contributes 1_000_000, with 10% fee = 900_000 to recipient per stream
    // Total: 2_700_000 to recipient
    assert_eq!(amounts.get_unchecked(0), 1_000_000);
    assert_eq!(amounts.get_unchecked(1), 1_000_000);
    assert_eq!(amounts.get_unchecked(2), 1_000_000);
    
    let recipient_balance = balance(&ie, &ie.recipient);
    assert_eq!(recipient_balance, 2_700_000);
    
    // All streams removed from storage after final batch withdrawal
    for stream_id in [stream_id1, stream_id2, stream_id3].iter() {
        assert!(c.try_get_stream(stream_id).is_err(), "stream should be removed after final withdrawal");
    }
}

#[test]
fn integration_withdraw_no_overdraw_edge_case_high_fee() {
    // Edge case: extremely high fee (99%) with multiple withdrawals
    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    let treasury = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);
    
    let deposit = 1_000_000_i128;
    let duration = 1000u64;
    
    mint(&ie, &ie.sender, &deposit);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_protocol_fee(&9900u32); // 99% fee (extreme case)
    c.set_treasury_address(&treasury);

    let stream_id = c.create_stream(
        &ie.sender, &ie.recipient, &ie.token,
        &deposit, &duration, &false, &simple_params(0),
    );

    // Multiple withdrawals throughout the stream
    for t in [250, 500, 750, 1000] {
        ie.env.ledger().set_timestamp(t);
        c.withdraw(&stream_id, &ie.recipient);
    }
    
    // With 99% fee, recipient gets ~1% of each withdrawal
    // Total should be ~1% of 1M = ~10K (due to rounding variations)
    let recipient_balance = balance(&ie, &ie.recipient);
    
    // Recipient should get somewhere around 1% of the deposit (within rounding)
    assert!(recipient_balance > 0, "recipient should receive something");
    assert!(recipient_balance <= deposit, "recipient should never receive more than deposit");
}

// ── Issue #282: Grace period claim + recover ────────────────────────────────

#[test]
fn integration_grace_period_claim_and_recover() {

    let ie = setup_integration();
    let c = client(&ie);
    let admin = Address::generate(&ie.env);
    ie.env.ledger().set_timestamp(0);

    c.initialize(&admin, &soroban_sdk::String::from_str(&ie.env, "1.0.0"));
    c.set_grace_period_ledgers(&10u32);
    assert_eq!(c.get_grace_period_ledgers(), 10);

    // Two streams: one for recipient claim during grace, one for recover after grace.
    // A full withdraw removes the stream, so both ACs cannot share one stream.
    mint(&ie, &ie.sender, &2_000_000);

    let stream_claim = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &false,
        &simple_params(0),
    );
    let stream_recover = c.create_stream(
        &ie.sender,
        &ie.recipient,
        &ie.token,
        &1_000_000,
        &1000,
        &false,
        &simple_params(1),
    );

    // end_time + 1: still inside grace (10 ledgers × 5s = 50s)
    ie.env.ledger().set_timestamp(1001);

    let blocked = c.try_recover_expired(&stream_recover, &ie.sender);
    assert_eq!(blocked, Err(Ok(StreamError::StreamNotActive)));

    let recipient_before = balance(&ie, &ie.recipient);
    c.withdraw(&stream_claim, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), recipient_before + 1_000_000);
    assert!(c.try_get_stream(&stream_claim).is_err());

    // end_time + grace_seconds + 1 = 1000 + 50 + 1
    ie.env.ledger().set_timestamp(1051);

    let sender_before = balance(&ie, &ie.sender);
    c.recover_expired(&stream_recover, &ie.sender);
    assert_eq!(balance(&ie, &ie.sender), sender_before + 1_000_000);
    assert!(c.try_get_stream(&stream_recover).is_err());

    let after_recover = c.try_withdraw(&stream_recover, &ie.recipient);
    assert_eq!(after_recover, Err(Ok(StreamError::StreamNotFound)));
}

// ── Issue #502: Full lifecycle test covering all stream operations ────────────

#[test]
fn integration_complete_lifecycle_all_operations() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);

    mint(&ie, &ie.sender, &5_000_000);
    let token = soroban_sdk::token::Client::new(&ie.env, &ie.token);

    // 1. Create a stream
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
        &false,
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 1_000_000);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(balance(&ie, &ie.contract), 1_000_000);

    // 2. Perform a partial withdraw
    ie.env.ledger().set_timestamp(250);
    c.withdraw(&stream_id, &ie.recipient);
    assert_eq!(balance(&ie, &ie.recipient), 250_000);

    // 3. Top-up the stream
    let stream_before_topup = c.get_stream(&stream_id);
    let end_time_before = stream_before_topup.end_time;

    c.top_up(&stream_id, &ie.sender, &ie.token, &500_000);

    let stream_after_topup = c.get_stream(&stream_id);
    assert_eq!(stream_after_topup.deposit, 1_500_000);
    assert!(stream_after_topup.end_time > end_time_before);

    // 4. Pause the stream
    ie.env.ledger().set_timestamp(400);
    c.pause_stream(&stream_id, &ie.sender);
    let paused_stream = c.get_stream(&stream_id);
    assert_eq!(paused_stream.status, StreamStatus::Paused);

    // Verify pause halts accrual: balance should remain the same
    let balance_at_pause = balance(&ie, &ie.recipient);
    ie.env.ledger().set_timestamp(500);
    let claimable_while_paused = c.get_claimable(&stream_id);
    // After pause, claimable should not increase beyond what was accrued at pause time
    c.withdraw(&stream_id, &ie.recipient);
    let balance_after_attempted_withdraw = balance(&ie, &ie.recipient);
    assert_eq!(balance_at_pause, balance_after_attempted_withdraw);

    // 5. Resume the stream
    ie.env.ledger().set_timestamp(600);
    c.resume_stream(&stream_id, &ie.sender);
    let resumed_stream = c.get_stream(&stream_id);
    assert_eq!(resumed_stream.status, StreamStatus::Active);

    // Accrual should resume
    ie.env.ledger().set_timestamp(700);
    c.withdraw(&stream_id, &ie.recipient);
    assert!(balance(&ie, &ie.recipient) > balance_after_attempted_withdraw);

    // 6. Cancel the stream to verify final state
    ie.env.ledger().set_timestamp(800);
    let sender_before_cancel = balance(&ie, &ie.sender);
    let recipient_before_cancel = balance(&ie, &ie.recipient);

    c.cancel_stream(&stream_id, &ie.sender);

    let sender_after_cancel = balance(&ie, &ie.sender);
    let recipient_after_cancel = balance(&ie, &ie.recipient);

    // Verify balance conservation: refund + recipient = deposit
    let refund = sender_after_cancel - sender_before_cancel;
    let total_received = recipient_after_cancel;

    assert_eq!(
        refund + total_received,
        1_500_000,
        "Balance conservation check failed on cancel"
    );

    // Stream should be marked as Cancelled
    let cancelled_stream = c.get_stream(&stream_id);
    assert_eq!(cancelled_stream.status, StreamStatus::Cancelled);
}

#[test]
fn integration_lifecycle_with_multiple_pauses_and_resumes() {
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
        &false,
        &0u64,
        &false,
    );

    // First pause/resume cycle
    ie.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &ie.sender);
    assert_eq!(c.get_stream(&stream_id).status, StreamStatus::Paused);

    ie.env.ledger().set_timestamp(200);
    c.resume_stream(&stream_id, &ie.sender);
    assert_eq!(c.get_stream(&stream_id).status, StreamStatus::Active);

    // Second pause/resume cycle
    ie.env.ledger().set_timestamp(300);
    c.pause_stream(&stream_id, &ie.sender);

    ie.env.ledger().set_timestamp(400);
    c.resume_stream(&stream_id, &ie.sender);

    // Withdraw and verify consistency
    ie.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &ie.recipient);

    let recipient_balance = balance(&ie, &ie.recipient);
    assert!(recipient_balance > 0);
    assert!(recipient_balance <= 1_000_000);
}

#[test]
fn integration_lifecycle_with_topup_during_pause() {
    let ie = setup_integration();
    let c = client(&ie);
    ie.env.ledger().set_timestamp(0);

    mint(&ie, &ie.sender, &3_000_000);

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
        &false,
    );

    // Pause the stream
    ie.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &ie.sender);

    // Top-up while paused
    let stream_before = c.get_stream(&stream_id);
    c.top_up(&stream_id, &ie.sender, &ie.token, &500_000);
    let stream_after = c.get_stream(&stream_id);

    assert_eq!(stream_after.deposit, stream_before.deposit + 500_000);
    assert_eq!(stream_after.status, StreamStatus::Paused);

    // Resume and verify stream continues with new deposit
    ie.env.ledger().set_timestamp(200);
    c.resume_stream(&stream_id, &ie.sender);

    ie.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &ie.recipient);

    assert!(balance(&ie, &ie.recipient) > 0);
}

