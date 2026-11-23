//! Issue #815: sweep every entry point guarded by `check_admin` before initialize.
//!
//! Before this fix, each route reached `expect("contract not initialized")` in
//! `check_admin` and panicked. They now uniformly return the typed
//! `StreamError::NotInitialized` error.

#![cfg(test)]

use crate::{errors::StreamError, SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn every_check_admin_entry_point_returns_not_initialized_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SoroStreamContract, ());
    let client = SoroStreamContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let text = String::from_str(&env, "v1");

    macro_rules! assert_not_initialized {
        ($call:expr) => {
            assert_eq!($call, Err(Ok(StreamError::NotInitialized)));
        };
    }

    assert_not_initialized!(client.try_set_admin(&a));
    assert_not_initialized!(client.try_emergency_pause());
    assert_not_initialized!(client.try_emergency_resume());
    assert_not_initialized!(client.try_set_guardian(&a));
    assert_not_initialized!(client.try_set_governance(&a));
    assert_not_initialized!(client.try_add_fee_exempt(&a));
    assert_not_initialized!(client.try_remove_fee_exempt(&a));
    assert_not_initialized!(client.try_sweep_fees(&a, &b));
    assert_not_initialized!(client.try_set_max_streams(&1));
    assert_not_initialized!(client.try_set_sender_stream_limit(&a, &1));
    assert_not_initialized!(client.try_set_sender_cap(&1));
    assert_not_initialized!(client.try_migrate(&text, &text));
    assert_not_initialized!(client.try_set_withdrawal_cooldown(&a, &1));
    assert_not_initialized!(client.try_set_stream_creation_cooldown(&a, &1));
    assert_not_initialized!(client.try_register_federation(&a, &text, &b));
    assert_not_initialized!(client.try_unregister_federation(&a, &text));
    assert_not_initialized!(client.try_set_rate_limit_window(&a, &1));
    assert_not_initialized!(client.try_set_rate_limit_max(&a, &1));
    assert_not_initialized!(client.try_add_rate_limit_exempt(&a, &b));
    assert_not_initialized!(client.try_remove_rate_limit_exempt(&a, &b));
    assert_not_initialized!(client.try_set_whitelist_enabled(&a, &true));
    assert_not_initialized!(client.try_set_expiry_warning_window(&1));
    assert_not_initialized!(client.try_set_new_sender_stream_cap(&1));
    assert_not_initialized!(client.try_set_sender_promotion_threshold(&1));
    assert_not_initialized!(client.try_set_token_whitelist_enabled(&a, &true));
    assert_not_initialized!(client.try_add_token_to_whitelist(&a, &b));
    assert_not_initialized!(client.try_remove_token_from_whitelist(&a, &b));
    assert_not_initialized!(client.try_set_max_streams_per_token(&1));
    assert_not_initialized!(client.try_set_max_deposit_per_token(&a, &1i128));
    assert_not_initialized!(client.try_add_to_blocklist(&a));
    assert_not_initialized!(client.try_remove_from_blocklist(&a));
    assert_not_initialized!(client.try_set_grace_period_ledgers(&1));
    assert_not_initialized!(client.try_set_creation_fee(&1i128, &a));
    assert_not_initialized!(client.try_withdraw_treasury(&a, &1i128, &b));
    assert_not_initialized!(client.try_withdraw_all_from_treasury(&a, &b));
    assert_not_initialized!(client.try_recalibrate_stats(&a));
}
