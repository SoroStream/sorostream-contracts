//! Tests for issue #625 — Storage version migration can be front-run by the caller.
//!
//! Acceptance criteria:
//!   • `migrate_storage` can only be called by the designated migration admin.
//!   • Unauthorized callers panic with `StreamError::NotAuthorized`.
//!   • Unit test: non-admin storage migration is rejected.

#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
use soroban_sdk::{
    testutils::Address as _,
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    admin: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);

    let client = SoroStreamContractClient::new(&env, &contract_id);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    TestEnv {
        env,
        contract_id,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

// ─── Issue #625 tests ───────────────────────────────────────────────────────

/// A non-admin caller must NOT be able to trigger migrate_storage.
/// Previously any caller could call this, potentially front-running a pending
/// migration before the admin intended it.
#[test]
fn test_625_non_admin_migrate_storage_rejected() {
    let t = setup();
    let c = client(&t);
    let attacker = Address::generate(&t.env);

    let result = c.try_migrate_storage(&attacker);
    assert_eq!(
        result.err().unwrap().unwrap(),
        StreamError::NotAuthorized,
        "Non-admin caller must be rejected with NotAuthorized"
    );
}

/// The contract admin (acting as default migration admin) can run migrate_storage.
#[test]
fn test_625_admin_can_migrate_storage() {
    let t = setup();
    let c = client(&t);

    // Simulate a pre-migration state by checking the contract was
    // initialized (version = 1 via initialize), so migration is already applied.
    // We verify the admin-only path returns MigrationAlreadyApplied (not NotAuthorized).
    let result = c.try_migrate_storage(&t.admin);
    match result {
        Ok(_) => { /* migration applied successfully */ }
        Err(Ok(StreamError::MigrationAlreadyApplied)) => {
            // Storage already at current version — migration path was reached and
            // the admin auth check passed. This is the expected result after
            // initialize() stamps version 1.
        }
        Err(Ok(e)) => panic!("Admin migrate_storage failed with unexpected error: {:?}", e),
        Err(Err(e)) => panic!("Admin migrate_storage failed with invoke error: {:?}", e),
    }
}

/// A designated migration admin (different from contract admin) can run migrate_storage.
#[test]
fn test_625_migration_admin_can_migrate_storage() {
    let t = setup();
    let c = client(&t);
    let migration_admin = Address::generate(&t.env);

    // Contract admin sets a dedicated migration admin.
    c.set_migration_admin(&t.admin, &migration_admin);

    // Contract admin is no longer the migration admin — only migration_admin can proceed.
    let result_admin = c.try_migrate_storage(&t.admin);
    // Contract admin is now a non-migration-admin, so should be rejected.
    assert_eq!(
        result_admin.err().unwrap().unwrap(),
        StreamError::NotAuthorized,
        "Contract admin should be rejected when a dedicated migration admin is set"
    );

    // The designated migration admin should be able to call migrate_storage.
    let result_mig = c.try_migrate_storage(&migration_admin);
    match result_mig {
        Ok(_) => { /* migration applied */ }
        Err(Ok(StreamError::MigrationAlreadyApplied)) => {
            // Version already current — auth check passed successfully.
        }
        Err(Ok(e)) => panic!("Migration admin failed with unexpected error: {:?}", e),
        Err(Err(e)) => panic!("Migration admin failed with invoke error: {:?}", e),
    }
}

/// A random third party must be rejected even when a migration admin is set.
#[test]
fn test_625_third_party_rejected_when_migration_admin_set() {
    let t = setup();
    let c = client(&t);
    let migration_admin = Address::generate(&t.env);
    let attacker = Address::generate(&t.env);

    c.set_migration_admin(&t.admin, &migration_admin);

    let result = c.try_migrate_storage(&attacker);
    assert_eq!(
        result.err().unwrap().unwrap(),
        StreamError::NotAuthorized,
        "Third-party caller must be rejected with NotAuthorized"
    );
}
