
use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::StellarAssetClient,
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
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #505: Storage Layout Optimization Tests
// ─────────────────────────────────────────────────────────────────────────
// Test that storage-optimized streams function correctly with packed fields.

#[test]
fn test_issue_505_storage_optimized_boolean_fields() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &5000u64,
        &true,   // auto_renew = true
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 0,
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

        tags: None,
        metadata_uri: None,
    },
    );

    let stream = c.get_stream(&stream_id);

    // Verify all boolean fields are correctly stored and retrieved
    assert_eq!(stream.auto_renew, true, "auto_renew field not preserved");
    assert_eq!(stream.options.allow_recipient_termination, false, "allow_recipient_termination field not preserved");
    assert_eq!(stream.options.non_transferable, false, "non_transferable field not preserved");
    assert_eq!(stream.options.sender_locked, false, "sender_locked field not preserved");
    assert_eq!(stream.options.is_dual_stream, false, "is_dual_stream field not preserved");
}

#[test]
fn test_issue_505_storage_optimized_type_conversions() {
    let t = setup();
    let c = client(&t);

    // Create stream with various field sizes
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &990_000_000);
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &999_999_999,      // Large amount
        &100_000u64,       // Large duration
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 0,
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

        tags: None,
        metadata_uri: None,
    },
    );

    let stream = c.get_stream(&stream_id);

    // Verify large values are correctly preserved through storage optimization
    assert_eq!(stream.deposit, 999_999_999, "Large deposit value not preserved");
    assert_eq!(stream.end_time, 100_000, "Large duration value not preserved");
}

#[test]
fn test_issue_505_storage_multiple_boolean_combinations() {
    let t = setup();
    let c = client(&t);

    // Test multiple streams with different boolean combinations
    let stream_1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000u64,
        &true,
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 0,
            renew_count: None,
            recurrence: None,
            lock_until: 0,
            allow_recipient_termination: true,
            non_transferable: false,
            holdback_amount: 0,
            withdrawal_steps: None,
            min_withdrawal_amount: None,
            sponsor: None,
            requires_recipient_approval: false,

        tags: None,
        metadata_uri: None,
    },
    );

    let other_recipient = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &5_000_000);

    let stream_2 = c.create_stream(
        &t.sender,
        &other_recipient,
        &t.token_id,
        &250_000,
        &1000u64,
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 1,
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

        tags: None,
        metadata_uri: None,
    },
    );

    let s1 = c.get_stream(&stream_1);
    let s2 = c.get_stream(&stream_2);

    assert_eq!(s1.auto_renew, true);
    assert_eq!(s1.options.allow_recipient_termination, true);
    assert_eq!(s2.auto_renew, false);
    assert_eq!(s2.options.allow_recipient_termination, false);
}
