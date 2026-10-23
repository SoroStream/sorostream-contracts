//! Test suite for SoroStream.
//!
//! Sections
//! ────────
//! 1. Core lifecycle (create / claim / cancel)
//! 2. Auto-renewal regression tests  ← issue #8  (feat/42-autorenewal-sim-tests)
//! 3. Reputation scoring
//! 4. 2-of-2 multisig proposal flow

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{storage::Persistent, Address as _, Events, Ledger, LedgerInfo},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal,
};

fn lifecycle_event(env: &Env, name: &str) -> Option<(soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> {
    env.events().all().iter().find_map(|(_, topics, data)| {
        let topic: soroban_sdk::Symbol = topics.get(0)?.into_val(env);
        (topic == soroban_sdk::Symbol::new(env, name)).then(|| (topics.clone(), data.clone()))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Shared test helpers
// ─────────────────────────────────────────────────────────────────────────────

struct TestEnv {
    env: Env,
    sender: Address,
    recipient: Address,
    token: Address,
    contract: Address,
}

impl TestEnv {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let sender = Address::generate(&env);
        let recipient = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(admin.clone());
        let token = token_id.address();
        // Mint 10 million to sender so tests have plenty of tokens.
        StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

        let contract = env.register(StreamingContract, ());
        TokenClient::new(&env, &token).approve(
            &sender,
            &contract,
            &10_000_000,
            &(env.ledger().sequence() + 1_000_000),
        );
        Self { env, sender, recipient, token, contract }
    }

    fn client(&self) -> StreamingContractClient<'_> {
        StreamingContractClient::new(&self.env, &self.contract)
    }

    fn token_client(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }

    /// Set the ledger timestamp.
    fn set_time(&self, ts: u64) {
        self.env.ledger().set(LedgerInfo {
            timestamp: ts,
            ..self.env.ledger().get()
        });
    }

    /// Set the ledger sequence number.
    fn set_seq(&self, seq: u32) {
        self.env.ledger().set(LedgerInfo {
            sequence_number: seq,
            ..self.env.ledger().get()
        });
    }

    /// Current ledger timestamp.
    fn now(&self) -> u64 {
        self.env.ledger().timestamp()
    }

    /// Current ledger sequence.
    fn seq(&self) -> u32 {
        self.env.ledger().get().sequence_number
    }

    fn approve_renewals(&self, sender: &Address, amount: i128) {
        self.token_client()
            .approve(sender, &self.contract, &amount, &(self.seq() + 1_000));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  1. Core lifecycle
// ─────────────────────────────────────────────────────────────────────────────

// ── Issue #618: prevent self-loop streams ────────────────────────────────────

/// Creating a stream where the sender is the contract itself must be rejected.
/// A self-referential stream would exhaust gas or cause undefined behaviour.
#[test]
#[should_panic(expected = "Error::InvalidSender")]
fn test_create_stream_contract_as_sender_is_rejected() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    // Use the contract address as the sender — must panic with InvalidSender.
    client.create_stream(
        &t.contract, // sender == contract_address ← must be rejected
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );
}

#[test]
fn test_create_stream_stores_correct_fields() {
    let t = TestEnv::new();
    let client = t.client();

    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.deposit, 1_000);
    assert_eq!(stream.claimed, 0);
    assert_eq!(stream.rate_per_second, 1);
    assert_eq!(stream.start_time, start);
    assert_eq!(stream.end_time, end);
    assert!(!stream.auto_renew);
    assert!(!stream.cancelled);
}

#[test]
fn test_sponsor_stream_requires_sponsor_authentication() {
    let t = TestEnv::new();
    let client = t.client();
    let sponsor = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token).mint(&sponsor, &1_000);
    let start = t.now();
    let end = start + 100;

    client.sponsor_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
        &sponsor,
    );

    let auths = t.env.auths();
    assert!(auths.iter().any(|(address, _)| address == &t.sender));
    assert!(auths.iter().any(|(address, _)| address == &sponsor));
}

#[test]
fn test_claim_returns_correct_amount_at_midpoint() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );

    t.set_time(start + 500);
    let claimed = client.claim(&stream_id);
    assert_eq!(claimed, 500);
    assert_eq!(t.token_client().balance(&t.recipient), 500);
}

#[test]
fn test_claim_emits_lifecycle_event_payload() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let stream_id = client.create_stream(&t.sender, &t.recipient, &t.token, &1_000, &start, &(start + 1_000), &false);
    t.set_time(start + 500);
    client.claim(&stream_id);

    let (topics, data) = lifecycle_event(&t.env, "StreamWithdrawn").unwrap();
    let event_stream_id: u64 = topics.get(1).unwrap().into_val(&t.env);
    assert_eq!(event_stream_id, stream_id);
    let payload: (Address, Address, i128, i128) = data.into_val(&t.env);
    assert_eq!(payload, (t.recipient, t.token, 500, 500));
}

#[test]
fn test_cancel_splits_vested_and_remainder() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );

    t.set_time(start + 300);
    client.cancel(&stream_id);

    assert_eq!(t.token_client().balance(&t.recipient), 300);
    // sender started with 10_000_000, deposited 1_000, got back 700
    assert_eq!(t.token_client().balance(&t.sender), 9_999_700);
}

#[test]
fn test_cancel_emits_lifecycle_event_payload() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let stream_id = client.create_stream(&t.sender, &t.recipient, &t.token, &1_000, &start, &(start + 1_000), &false);
    t.set_time(start + 300);
    client.cancel(&stream_id);

    let (topics, data) = lifecycle_event(&t.env, "StreamCancelled").unwrap();
    let event_stream_id: u64 = topics.get(1).unwrap().into_val(&t.env);
    assert_eq!(event_stream_id, stream_id);
    let payload: (Address, Address, Address, i128, i128) = data.into_val(&t.env);
    assert_eq!(payload, (t.sender, t.recipient, t.token, 300, 700));
}

#[test]
fn test_renew_emits_lifecycle_event_payload() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(&t.sender, &t.recipient, &t.token, &1_000, &start, &end, &true);
    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end);
    client.renew(&stream_id);

    let (topics, data) = lifecycle_event(&t.env, "StreamRenewed").unwrap();
    let event_stream_id: u64 = topics.get(1).unwrap().into_val(&t.env);
    assert_eq!(event_stream_id, stream_id);
    let payload: (Address, Address, Address, i128, i128, u64, u64) = data.into_val(&t.env);
    assert_eq!(payload, (t.sender, t.recipient, t.token, 1_000, 1_000, end, end + 1_000));
}

#[test]
fn test_cancel_saturates_refund_when_claimed_and_claimable_exceed_deposit() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &(start + 1_000),
        &false,
    );

    let mut stream = storage::get_stream(&t.env, stream_id);
    stream.claimed = 999;
    stream.rate_per_second = 1_001;
    storage::save_stream(&t.env, stream_id, &stream);
    t.set_time(start + 1);

    client.cancel(&stream_id);

    assert_eq!(t.token_client().balance(&t.recipient), 2);
    assert_eq!(t.token_client().balance(&t.sender), 9_999_000);
}

#[test]
#[should_panic(expected = "stream already cancelled")]
fn test_double_cancel_panics() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &(start + 1_000),
        &false,
    );
    client.cancel(&stream_id);
    client.cancel(&stream_id); // must panic
}

#[test]
#[should_panic(expected = "nothing to claim")]
fn test_claim_before_start_panics() {
    let t = TestEnv::new();
    let client = t.client();
    // Stream starts 500 s in the future
    let start = t.now() + 500;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &(start + 1_000),
        &false,
    );
    client.claim(&stream_id);
}

#[test]
fn test_claimable_caps_at_deposit() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );

    // Jump way past end — claimable should not exceed deposit.
    t.set_time(end + 9_999);
    assert_eq!(client.get_claimable(&stream_id), 1_000);
}

// ─────────────────────────────────────────────────────────────────────────────
//  2. Auto-renewal regression tests  (issue #8 / feat/42-autorenewal-sim-tests)
// ─────────────────────────────────────────────────────────────────────────────

/// After expiry + renewal, the renewed stream's start_time must equal the
/// original end_time — NOT the current timestamp.
#[test]
fn test_renewal_start_time_equals_original_end_time() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 1_000);
    // Advance past expiry by an arbitrary delta (should NOT affect start_time).
    t.set_time(end + 42);
    t.env.mock_auths(&[]);
    client.renew(&stream_id);

    let stream = client.get_stream(&stream_id);
    assert_eq!(
        stream.start_time, end,
        "renewed stream.start_time must equal original end_time"
    );
    assert_eq!(
        stream.end_time,
        end + 1_000,
        "renewed stream.end_time is wrong"
    );
}

/// A keeper can renew after the sender grants the contract token allowance;
/// the sender does not need to authorize the keeper's transaction.
#[test]
fn test_keeper_renews_with_sender_allowance() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 100;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );
    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end);
    // Model a keeper transaction with no sender authorization.
    t.env.mock_auths(&[]);

    client.renew(&stream_id);

    assert_eq!(t.token_client().balance(&t.sender), 9_998_000);
    assert_eq!(client.get_stream(&stream_id).start_time, end);
}

#[test]
fn test_keeper_cannot_renew_without_sender_allowance() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 100;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );
    let sender_balance = t.token_client().balance(&t.sender);
    t.set_time(end);

    assert!(client.try_renew(&stream_id).is_err());
    assert_eq!(t.token_client().balance(&t.sender), sender_balance);
    assert_eq!(client.get_stream(&stream_id).start_time, start);
}

/// At the exact ledger of renewal, the claimable balance must be zero
/// (the flush during renew() settled all vested tokens).
#[test]
fn test_claimable_at_renewal_ledger_is_zero() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end); // exactly at expiry
    client.renew(&stream_id);

    // Right after renewal the new cycle hasn't accumulated anything.
    assert_eq!(
        client.get_claimable(&stream_id),
        0,
        "claimable at renewal ledger must be zero"
    );
}

/// After 10 seconds into the renewed cycle, the claimable balance must be
/// exactly rate_per_second * 10.
#[test]
fn test_claimable_ten_ledgers_into_renewed_stream() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    // rate = 1 token/second (deposit 1000 / duration 1000)
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end);
    client.renew(&stream_id);

    // Advance 10 seconds into the new cycle.
    t.set_time(end + 10);
    assert_eq!(
        client.get_claimable(&stream_id),
        10,
        "after 10 ledgers into renewed stream, claimable should equal flow_rate * 10"
    );
}

/// Renewing before expiry must panic.
#[test]
#[should_panic(expected = "stream has not expired yet")]
fn test_renew_before_expiry_panics() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );
    t.set_time(end - 1);
    client.renew(&stream_id);
}

/// Renewing a stream with auto_renew=false must panic.
#[test]
#[should_panic(expected = "auto_renew is not enabled for this stream")]
fn test_renew_without_flag_panics() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );
    t.set_time(end + 1);
    client.renew(&stream_id);
}

/// Multiple successive renewals preserve correct time windowing.
#[test]
fn test_multiple_renewals_chain_correctly() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let duration = 1_000_u64;
    let end = start + duration;

    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 3_000);
    for cycle in 1u64..=3 {
        t.set_time(start + duration * cycle);
        client.renew(&stream_id);
        let stream = client.get_stream(&stream_id);
        assert_eq!(
            stream.start_time,
            start + duration * cycle,
            "cycle {cycle}: start_time mismatch"
        );
        assert_eq!(
            stream.end_time,
            start + duration * (cycle + 1),
            "cycle {cycle}: end_time mismatch"
        );
        assert_eq!(
            stream.claimed, 0,
            "cycle {cycle}: claimed should reset to 0"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  3. Reputation scoring
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reputation_starts_at_zero() {
    let t = TestEnv::new();
    let client = t.client();
    assert_eq!(client.get_reputation(&t.sender), 0);
}

#[test]
fn test_reputation_increments_on_renewal() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end);
    client.renew(&stream_id);

    assert_eq!(client.get_reputation(&t.sender), 1);
}

#[test]
fn test_reputation_increments_on_each_renewal() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let duration = 1_000_u64;
    let end = start + duration;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 5_000);
    for i in 1u64..=5 {
        t.set_time(start + duration * i);
        client.renew(&stream_id);
        assert_eq!(
            client.get_reputation(&t.sender),
            i as u64,
            "reputation after {i} renewals should be {i}"
        );
    }
}

#[test]
fn test_reputation_not_incremented_on_cancel() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;
    let stream_id = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &false,
    );

    t.set_time(start + 500);
    client.cancel(&stream_id);

    assert_eq!(
        client.get_reputation(&t.sender),
        0,
        "cancellation must not increment reputation"
    );
}

#[test]
fn test_reputation_is_per_sender() {
    let t = TestEnv::new();
    let client = t.client();

    let sender2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token).mint(&sender2, &10_000_000);

    let start = t.now();
    let end = start + 1_000;

    // sender1 creates + renews once
    let s1 = client.create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );
    // sender2 creates + renews twice
    let s2a = client.create_stream(
        &sender2,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );
    let s2b = client.create_stream(
        &sender2,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &end,
        &true,
    );

    t.approve_renewals(&t.sender, 1_000);
    t.approve_renewals(&sender2, 2_000);
    t.set_time(end);
    client.renew(&s1);
    client.renew(&s2a);
    client.renew(&s2b);

    assert_eq!(client.get_reputation(&t.sender), 1);
    assert_eq!(client.get_reputation(&sender2), 2);
}

// ─────────────────────────────────────────────────────────────────────────────
//  4. 2-of-2 multisig proposal flow
// ─────────────────────────────────────────────────────────────────────────────

fn setup_proposal(t: &TestEnv, window: u32) -> u64 {
    t.client().propose_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &1_000_u64,
        &false,
        &t.recipient, // co-signer = recipient for simplicity
        &window,
    )
}

#[test]
fn test_proposal_stores_fields_correctly() {
    let t = TestEnv::new();
    let seq_before = t.seq();
    let proposal_id = setup_proposal(&t, 100);
    let proposal = t.client().get_proposal(&proposal_id);

    assert_eq!(proposal.deposit, 1_000);
    assert_eq!(proposal.duration, 1_000);
    assert_eq!(proposal.rate_per_second, 1);
    assert_eq!(proposal.cosigner, t.recipient);
    assert_eq!(proposal.expiry_ledger, seq_before + 100);
}

#[test]
fn test_deposit_escrowed_on_propose() {
    let t = TestEnv::new();
    let bal_before = t.token_client().balance(&t.sender);
    setup_proposal(&t, 100);
    let bal_after = t.token_client().balance(&t.sender);
    assert_eq!(
        bal_before - bal_after,
        1_000,
        "deposit should be escrowed on proposal"
    );
}

#[test]
fn test_approve_creates_stream_and_removes_proposal() {
    let t = TestEnv::new();
    let proposal_id = setup_proposal(&t, 100);

    let stream_id = t.client().approve_proposal(&proposal_id);
    let stream = t.client().get_stream(&stream_id);

    assert_eq!(stream.deposit, 1_000);
    assert_eq!(stream.rate_per_second, 1);
    assert!(!stream.cancelled);
}

#[test]
#[should_panic(expected = "proposal has expired")]
fn test_approve_after_expiry_panics() {
    let t = TestEnv::new();
    let proposal_id = setup_proposal(&t, 10);

    // Advance ledger sequence past expiry.
    t.set_seq(t.seq() + 11);
    t.client().approve_proposal(&proposal_id);
}

#[test]
fn test_discard_expired_proposal_refunds_sender() {
    let t = TestEnv::new();
    let bal_before = t.token_client().balance(&t.sender);

    let proposal_id = setup_proposal(&t, 10);
    // Advance past expiry.
    t.set_seq(t.seq() + 11);
    // The sender may discard the proposal even after expiry.
    t.client().discard_proposal(&proposal_id, &t.sender);

    let bal_after = t.token_client().balance(&t.sender);
    assert_eq!(bal_after, bal_before, "sender should be fully refunded");
}

#[test]
fn test_sender_can_retract_before_expiry() {
    let t = TestEnv::new();
    let bal_before = t.token_client().balance(&t.sender);

    let proposal_id = setup_proposal(&t, 100);
    // Still within the window — sender retracts.
    t.client().discard_proposal(&proposal_id, &t.sender);

    let bal_after = t.token_client().balance(&t.sender);
    assert_eq!(
        bal_after, bal_before,
        "sender should be refunded after retraction"
    );
}

#[test]
#[should_panic(expected = "proposal not found")]
fn test_discard_proposal_twice_refunds_only_once() {
    let t = TestEnv::new();
    let bal_before = t.token_client().balance(&t.sender);
    let proposal_id = setup_proposal(&t, 100);
    let escrowed_balance = t.token_client().balance(&t.contract);

    t.client().discard_proposal(&proposal_id, &t.sender);
    assert_eq!(t.token_client().balance(&t.sender), bal_before);
    assert_eq!(
        t.token_client().balance(&t.contract),
        escrowed_balance - 1_000
    );

    // The proposal has been removed, so this fails before a second transfer.
    t.client().discard_proposal(&proposal_id, &t.sender);
}

#[test]
fn test_missing_stream_and_proposal_return_typed_errors() {
    let t = TestEnv::new();
    assert!(matches!(
        t.client().try_get_stream(&42),
        Err(Ok(StreamingError::StreamNotFound)),
    ));
    assert!(matches!(
        t.client().try_get_proposal(&42),
        Err(Ok(StreamingError::ProposalNotFound)),
    ));
}

#[test]
fn test_bump_ttl_extends_dormant_stream_without_mutation() {
    let t = TestEnv::new();
    let start = t.now();
    let stream_id = t.client().create_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &start,
        &(start + 1_000),
        &false,
    );
    let ttl_before = t.env.as_contract(&t.contract, || {
        t.env
            .storage()
            .persistent()
            .get_ttl(&DataKey::Stream(stream_id))
    });
    t.set_seq(t.seq() + ttl_before - 40);
    let dormant_ttl = t.env.as_contract(&t.contract, || {
        t.env
            .storage()
            .persistent()
            .get_ttl(&DataKey::Stream(stream_id))
    });

    t.client().bump_ttl(&stream_id);

    let extended_ttl = t.env.as_contract(&t.contract, || {
        t.env
            .storage()
            .persistent()
            .get_ttl(&DataKey::Stream(stream_id))
    });
    assert!(dormant_ttl < 50);
    assert!(extended_ttl > dormant_ttl);
    assert_eq!(t.client().get_stream(&stream_id).claimed, 0);
}

#[test]
#[should_panic(expected = "Error::Unauthorized")]
fn test_third_party_cannot_discard_active_proposal() {
    let t = TestEnv::new();
    let proposal_id = setup_proposal(&t, 100);

    // A stranger tries to discard before expiry — must panic.
    let stranger = Address::generate(&t.env);
    t.client().discard_proposal(&proposal_id, &stranger);
}

// ── Expired proposal cleanup ─────────────────────────────────────────────────

/// Anyone can discard an expired proposal and return its escrow to the sender.
#[test]
fn test_third_party_can_discard_expired_proposal() {
    let t = TestEnv::new();
    let bal_before = t.token_client().balance(&t.sender);
    let proposal_id = setup_proposal(&t, 10);

    // Advance past expiry.
    t.set_seq(t.seq() + 11);

    let third_party = Address::generate(&t.env);
    t.env.mock_auths(&[]);
    t.client().discard_proposal(&proposal_id, &third_party);

    assert_eq!(t.token_client().balance(&t.sender), bal_before);
    assert_eq!(t.token_client().balance(&third_party), 0);
}

#[test]
fn test_stream_active_after_approval_has_correct_start_time() {
    let t = TestEnv::new();
    t.set_time(5_000);
    let proposal_id = setup_proposal(&t, 100);

    t.set_time(5_500); // time passes before approval
    let stream_id = t.client().approve_proposal(&proposal_id);
    let stream = t.client().get_stream(&stream_id);

    // start_time should be the approval timestamp, not proposal timestamp
    assert_eq!(stream.start_time, 5_500);
    assert_eq!(stream.end_time, 5_500 + 1_000);
}
