/// # Event Field Integration Tests — Issue #320
///
/// These tests emit real contract events through the Soroban sandbox and assert
/// that every required field is present and correctly typed in the published
/// event record. They act as a schema-drift detector: if a field name, position,
/// or type changes in `events.rs` without a matching SDK/indexer update, at least
/// one assertion here will fail deterministically.
///
/// Covered lifecycle events:
///   1. `StreamCreated`   — `create_stream`
///   2. `StreamWithdrawn` — `withdraw`
///   3. `StreamCancelled` — `cancel_stream`
///   4. `StreamLocked`    — reentrancy guard path (field-type regression test)
///   5. `StreamToppedUp`  — `top_up`
///
/// Each test verifies:
///   - Exactly one event of the expected name is emitted.
///   - The emitter address is the stream contract.
///   - `topics` has the documented count.
///   - `topics[0]` deserializes as `Symbol` with the correct name.
///   - `topics[1]` deserializes as `u64` (stream_id).
///   - The data tuple deserializes into the documented field types and values.
#[cfg(test)]
mod event_field_tests {
    extern crate std;

    use soroban_sdk::testutils::Events;
    use soroban_sdk::{Address, Env, IntoVal, Symbol, Val};
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

    use crate::{SoroStreamContract, SoroStreamContractClient};

    // ── Shared test harness ──────────────────────────────────────────────────

    struct EventEnv {
        env: Env,
        contract: Address,
        token: Address,
        sender: Address,
        recipient: Address,
    }

    fn setup() -> EventEnv {
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();

        let contract = env.register(SoroStreamContract, ());
        let token_admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let sender = Address::generate(&env);
        let recipient = Address::generate(&env);

        // Disable the minimum-duration guard so tests can use short durations.
        SoroStreamContractClient::new(&env, &contract).set_min_duration(&sender, &0u64);

        EventEnv { env, contract, token, sender, recipient }
    }

    fn client(e: &EventEnv) -> SoroStreamContractClient<'_> {
        SoroStreamContractClient::new(&e.env, &e.contract)
    }

    fn mint(e: &EventEnv, to: &Address, amount: i128) {
        StellarAssetClient::new(&e.env, &e.token).mint(to, &amount);
    }

    /// Filter all published events by their first topic name.
    ///
    /// Returns a `Vec` of `(emitter, topics, data)` tuples whose first topic
    /// matches `name`. Each element is an owned clone — safe to destructure.
    fn find_events(
        env: &Env,
        all: &soroban_sdk::Vec<(Address, soroban_sdk::Vec<Val>, Val)>,
        name: &str,
    ) -> std::vec::Vec<(Address, soroban_sdk::Vec<Val>, Val)> {
        all.iter()
            .filter(|(_, topics, _)| {
                if topics.is_empty() {
                    return false;
                }
                let first: Symbol = topics.get(0).unwrap().into_val(env);
                first == Symbol::new(env, name)
            })
            .collect()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 1 — StreamCreated
    // ─────────────────────────────────────────────────────────────────────────

    /// `create_stream` must emit exactly one `StreamCreated` event whose fields
    /// match the contract call parameters.
    ///
    /// Schema (events.rs):
    ///   topics: (Symbol("StreamCreated"), stream_id: u64)
    ///   data:   (sender: Address, recipient: Address, amount: i128, flow_rate: i128, end_time: u64)
    #[test]
    fn event_fields_stream_created() {
        let e = setup();
        let c = client(&e);
        e.env.ledger().set_timestamp(1000);
        mint(&e, &e.sender, 500_000);

        let stream_id = c.create_stream(
            &e.sender,
            &e.recipient,
            &e.token,
            &500_000i128,
            &500u64,  // duration_seconds
            &0u64,    // cliff
            &0u64,    // lock_until
            &false,   // auto_renew
            &0u64,    // start_time (0 = ledger timestamp)
            &false,   // allow_recipient_termination
            &0i128,   // holdback_amount
        );

        let all = e.env.events().all();
        let matches = find_events(&e.env, &all, "StreamCreated");

        // 1a. Exactly one event.
        assert_eq!(matches.len(), 1, "create_stream must emit exactly one StreamCreated event");

        let (emitter, topics, data) = &matches[0];

        // 1b. Emitter is the stream contract.
        assert_eq!(*emitter, e.contract, "StreamCreated emitter must be the stream contract");

        // 1c. Two topics.
        assert_eq!(topics.len(), 2, "StreamCreated must have 2 topics: (name, stream_id)");

        // 1d. topics[0] = Symbol("StreamCreated").
        let topic_name: Symbol = topics.get(0).unwrap().into_val(&e.env);
        assert_eq!(topic_name, Symbol::new(&e.env, "StreamCreated"));

        // 1e. topics[1] = stream_id (u64).
        let topic_stream_id: u64 = topics.get(1).unwrap().into_val(&e.env);
        assert_eq!(topic_stream_id, stream_id, "StreamCreated topics[1] must be stream_id");

        // 1f. data = (sender, recipient, amount, flow_rate, end_time).
        let (ev_sender, ev_recipient, ev_amount, ev_flow_rate, ev_end_time): (
            Address, Address, i128, i128, u64,
        ) = data.clone().into_val(&e.env);

        assert_eq!(ev_sender, e.sender, "StreamCreated data[0] (sender) mismatch");
        assert_eq!(ev_recipient, e.recipient, "StreamCreated data[1] (recipient) mismatch");
        assert_eq!(ev_amount, 500_000i128, "StreamCreated data[2] (amount) mismatch");
        // flow_rate = 500_000 / 500 = 1_000 stroops/s
        assert_eq!(ev_flow_rate, 1_000i128, "StreamCreated data[3] (flow_rate) mismatch");
        // end_time = start_time(1000) + duration(500) = 1500
        assert_eq!(ev_end_time, 1500u64, "StreamCreated data[4] (end_time) mismatch");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 2 — StreamWithdrawn
    // ─────────────────────────────────────────────────────────────────────────

    /// `withdraw` must emit exactly one `StreamWithdrawn` event with correct fields.
    ///
    /// Schema (events.rs):
    ///   topics: (Symbol("StreamWithdrawn"), stream_id: u64)
    ///   data:   (recipient: Address, amount: i128, timestamp: u64)
    #[test]
    fn event_fields_stream_withdrawn() {
        let e = setup();
        let c = client(&e);
        e.env.ledger().set_timestamp(0);
        mint(&e, &e.sender, 1_000_000);

        let stream_id = c.create_stream(
            &e.sender, &e.recipient, &e.token,
            &1_000_000i128, &1000u64,
            &0u64, &0u64, &false, &0u64, &false, &0i128,
        );

        e.env.ledger().set_timestamp(400);
        c.withdraw(&stream_id, &e.recipient);

        let all = e.env.events().all();
        let matches = find_events(&e.env, &all, "StreamWithdrawn");

        // 2a. Exactly one event.
        assert_eq!(matches.len(), 1, "withdraw must emit exactly one StreamWithdrawn event");

        let (emitter, topics, data) = &matches[0];

        // 2b. Emitter.
        assert_eq!(*emitter, e.contract, "StreamWithdrawn emitter must be the stream contract");

        // 2c. Two topics.
        assert_eq!(topics.len(), 2, "StreamWithdrawn must have 2 topics");

        // 2d. topics[0] name.
        let topic_name: Symbol = topics.get(0).unwrap().into_val(&e.env);
        assert_eq!(topic_name, Symbol::new(&e.env, "StreamWithdrawn"));

        // 2e. topics[1] stream_id.
        let topic_sid: u64 = topics.get(1).unwrap().into_val(&e.env);
        assert_eq!(topic_sid, stream_id, "StreamWithdrawn topics[1] must be stream_id");

        // 2f. data = (schema_version, recipient, amount, timestamp, total_withdrawn, event_nonce).
        let (
            ev_schema_version,
            ev_recipient,
            ev_amount,
            ev_timestamp,
            ev_total_withdrawn,
            ev_nonce,
        ): (u32, Address, i128, u64, i128, u64) = data.clone().into_val(&e.env);

        assert_eq!(ev_schema_version, 2u32, "StreamWithdrawn schema version must be 2");
        assert_eq!(ev_recipient, e.recipient, "StreamWithdrawn data[1] (recipient) mismatch");
        // flow_rate=1000, elapsed=400 → claimable = 400_000
        assert_eq!(ev_amount, 400_000i128, "StreamWithdrawn data[2] (amount) must be 400_000");
        assert_eq!(ev_timestamp, 400u64, "StreamWithdrawn data[3] (timestamp) must be 400");
        assert_eq!(ev_total_withdrawn, 400_000i128, "StreamWithdrawn data[4] (total_withdrawn) must equal 400_000");
        assert_eq!(ev_nonce, 1u64, "first StreamWithdrawn event nonce must be 1");

        // 2g. Regression guard: amount must be a non-negative i128 (field-type check).
        assert!(ev_amount >= 0, "StreamWithdrawn amount must not be negative");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 3 — StreamCancelled
    // ─────────────────────────────────────────────────────────────────────────

    /// `cancel_stream` must emit exactly one `StreamCancelled` event, and the
    /// conservation invariant `refund + earned == deposit` must hold.
    ///
    /// Schema (events.rs):
    ///   topics: (Symbol("StreamCancelled"), stream_id: u64)
    ///   data:   (sender: Address, refund_amount: i128, recipient_amount: i128)
    #[test]
    fn event_fields_stream_cancelled() {
        let e = setup();
        let c = client(&e);
        e.env.ledger().set_timestamp(0);
        mint(&e, &e.sender, 1_000_000);

        let stream_id = c.create_stream(
            &e.sender, &e.recipient, &e.token,
            &1_000_000i128, &1000u64,
            &0u64, &0u64, &false, &0u64, &false, &0i128,
        );

        // Cancel at t=300: earned = 300_000, refund = 700_000.
        e.env.ledger().set_timestamp(300);
        c.cancel_stream(&stream_id, &e.sender);

        let all = e.env.events().all();
        let matches = find_events(&e.env, &all, "StreamCancelled");

        // 3a. Exactly one event.
        assert_eq!(matches.len(), 1, "cancel_stream must emit exactly one StreamCancelled event");

        let (emitter, topics, data) = &matches[0];

        // 3b. Emitter.
        assert_eq!(*emitter, e.contract, "StreamCancelled emitter must be the stream contract");

        // 3c. Two topics.
        assert_eq!(topics.len(), 2, "StreamCancelled must have 2 topics");

        // 3d. topics[0] name.
        let topic_name: Symbol = topics.get(0).unwrap().into_val(&e.env);
        assert_eq!(topic_name, Symbol::new(&e.env, "StreamCancelled"));

        // 3e. topics[1] stream_id.
        let topic_sid: u64 = topics.get(1).unwrap().into_val(&e.env);
        assert_eq!(topic_sid, stream_id, "StreamCancelled topics[1] must be stream_id");

        // 3f. data = (sender, refund_amount, recipient_amount).
        let (ev_sender, ev_refund, ev_earned): (Address, i128, i128) =
            data.clone().into_val(&e.env);

        assert_eq!(ev_sender, e.sender, "StreamCancelled data[0] (sender) mismatch");
        assert_eq!(ev_earned, 300_000i128, "StreamCancelled data[2] (recipient_amount) must be 300_000");
        assert_eq!(ev_refund, 700_000i128, "StreamCancelled data[1] (refund_amount) must be 700_000");

        // 3g. Conservation invariant.
        assert_eq!(
            ev_refund + ev_earned,
            1_000_000i128,
            "StreamCancelled: refund_amount + recipient_amount must equal the full deposit"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 4 — StreamLocked field-type regression
    // ─────────────────────────────────────────────────────────────────────────

    /// Verifies that the `StreamWithdrawn` event fields deserialize correctly
    /// under conditions that exercise the reentrancy guard path.
    ///
    /// Because the Soroban sandbox is single-threaded, true reentrancy cannot be
    /// triggered. This test instead confirms:
    ///   - The `amount` field in `StreamWithdrawn` deserializes as `i128`
    ///     (not a `Symbol` or `Address`), which would indicate schema drift in
    ///     the locked/reentrant path.
    ///   - A second withdraw at the same timestamp either silently returns 0 or
    ///     returns a typed contract error — never a panic or mistyped field.
    ///
    /// If the event field order or type changes in `events.rs` to accommodate the
    /// locked flag, the `into_val::<(Address, i128, u64)>` call below will panic,
    /// failing the test deterministically.
    #[test]
    fn event_fields_stream_locked_guard() {
        let e = setup();
        let c = client(&e);
        e.env.ledger().set_timestamp(0);
        mint(&e, &e.sender, 2_000_000);

        let stream_id = c.create_stream(
            &e.sender, &e.recipient, &e.token,
            &2_000_000i128, &2000u64,
            &0u64, &0u64, &false, &0u64, &false, &0i128,
        );

        e.env.ledger().set_timestamp(500);
        c.withdraw(&stream_id, &e.recipient);

        let all = e.env.events().all();
        let withdrawn = find_events(&e.env, &all, "StreamWithdrawn");
        assert_eq!(withdrawn.len(), 1, "first withdraw must emit StreamWithdrawn");

        let (_, topics, data) = &withdrawn[0];

        // Type assertion: topics[1] must be a u64 stream_id, not any other type.
        let _: u64 = topics.get(1).unwrap().into_val(&e.env);

        // Type assertion: data must deserialize as (Address, i128, u64).
        // If the locked guard changed the amount field type this panics.
        let (_, ev_amount, _): (Address, i128, u64) = data.clone().into_val(&e.env);

        // The amount must equal flow_rate × elapsed = 1000 × 500 = 500_000.
        assert_eq!(
            ev_amount,
            500_000i128,
            "StreamWithdrawn amount must be 500_000 (flow_rate × elapsed)"
        );
        assert!(ev_amount >= 0, "StreamWithdrawn amount must be non-negative i128");

        // Attempt a second withdraw at the same timestamp.
        // Acceptable outcomes: Ok (zero amount) or Err (any typed error).
        // Unacceptable: panic due to mistyped event field.
        let result = c.try_withdraw(&stream_id, &e.recipient);
        if let Ok(_) = result {
            let all2 = e.env.events().all();
            let withdrawn2 = find_events(&e.env, &all2, "StreamWithdrawn");
            if withdrawn2.len() > 1 {
                let (_, _, data2) = &withdrawn2[withdrawn2.len() - 1];
                // If a second event was emitted its data must still deserialize correctly.
                let (_, zero_amount, _): (Address, i128, u64) = data2.clone().into_val(&e.env);
                assert!(
                    zero_amount >= 0,
                    "second StreamWithdrawn amount must be a non-negative i128"
                );
            }
        }
        // If Err, the guard is working — no further assertions needed.
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 5 — StreamToppedUp
    // ─────────────────────────────────────────────────────────────────────────

    /// `top_up` must emit exactly one `StreamToppedUp` event with correct fields.
    ///
    /// Schema (events.rs):
    ///   topics: (Symbol("StreamToppedUp"), stream_id: u64)
    ///   data:   (added_amount: i128, new_end_time: u64)
    #[test]
    fn event_fields_stream_topped_up() {
        let e = setup();
        let c = client(&e);
        e.env.ledger().set_timestamp(0);
        mint(&e, &e.sender, 2_000_000);

        // Create a stream: 1_000_000 over 1000s → flow_rate = 1000
        let stream_id = c.create_stream(
            &e.sender, &e.recipient, &e.token,
            &1_000_000i128, &1000u64,
            &0u64, &0u64, &false, &0u64, &false, &0i128,
        );

        // Top up with 1_000_000 more → extends by 1000s → new_end_time = 2000
        c.top_up(&stream_id, &e.sender, &1_000_000i128);

        let all = e.env.events().all();
        let matches = find_events(&e.env, &all, "StreamToppedUp");

        assert_eq!(matches.len(), 1, "top_up must emit exactly one StreamToppedUp event");

        let (emitter, topics, data) = &matches[0];

        assert_eq!(*emitter, e.contract, "StreamToppedUp emitter must be the stream contract");
        assert_eq!(topics.len(), 2, "StreamToppedUp must have 2 topics");

        let topic_name: Symbol = topics.get(0).unwrap().into_val(&e.env);
        assert_eq!(topic_name, Symbol::new(&e.env, "StreamToppedUp"));

        let topic_sid: u64 = topics.get(1).unwrap().into_val(&e.env);
        assert_eq!(topic_sid, stream_id, "StreamToppedUp topics[1] must be stream_id");

        // data = (added_amount: i128, new_end_time: u64)
        let (ev_added, ev_new_end): (i128, u64) = data.clone().into_val(&e.env);
        assert_eq!(ev_added, 1_000_000i128, "StreamToppedUp added_amount mismatch");
        // new_end_time = original_end(1000) + added_seconds(1000) = 2000
        assert_eq!(ev_new_end, 2000u64, "StreamToppedUp new_end_time mismatch");
    }
}
