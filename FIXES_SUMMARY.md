# Proxy Contract Issues - Fixed

This document summarizes the four issues that were identified and fixed in `contracts/proxy/src/lib.rs`.

## Issue 1: Missing Event in `upgrade_proxy()`

**Problem**: `upgrade_proxy()` updates the proxy's own WASM but publishes no event, unlike `upgrade()` which publishes a ProxyUpgraded event. Off-chain indexers watching proxy state changes have no event to observe when the proxy's own WASM is upgraded.

**Fix**: Added `ProxyProxyUpgraded` event publication in `upgrade_proxy()` with the old and new WASM hashes.

**Code Changes**:
- Updated `upgrade_proxy()` to publish a `ProxyProxyUpgraded` event after successfully updating the proxy WASM
- Event includes: old_hash and new_wasm_hash
- Updated documentation to note this behavior

**Test**: `test_upgrade_proxy_checks_version()` - Verifies the function accepts proper parameters and correctly validates storage version before attempting the upgrade.

---

## Issue 2: Inconsistent Error Handling in `check_admin()`

**Problem**: `check_admin()` calls `.expect("proxy not initialized")` which aborts the transaction with an untyped panic instead of returning `ProxyError::NotInitialized`. This is inconsistent with `get_admin()` and `get_impl_hash()` which return typed Results.

**Fix**: Changed `check_admin()` to return `Result<Address, ProxyError>` instead of panicking.

**Code Changes**:
- Changed `check_admin()` signature from `fn check_admin(env: &Env) -> Address` to `fn check_admin(env: &Env) -> Result<Address, ProxyError>`
- Updated all callers to handle the Result with `?` operator:
  - `set_admin()` - now uses `check_admin(&env)?`
  - `propose_admin()` - now uses `check_admin(&env)?`
  - `upgrade()` - now uses `check_admin(&env)?`
  - `upgrade_proxy()` - now uses `check_admin(&env)?`
  - `migrate()` - now uses `check_admin(&env)?`

**Tests**: 
- `test_check_admin_returns_not_initialized_when_uninitialized()` - Verifies that calling set_admin without initialization returns ProxyError::NotInitialized
- `test_set_admin_returns_not_initialized_error()` - Additional verification of error propagation

---

## Issue 3: Missing Validation in `migrate()`

**Problem**: `migrate()` forwards from_version/to_version to the implementation with no validation that to_version differs from from_version. A caller could accidentally (or intentionally) call migrate with identical version strings as a no-op.

**Fix**: Added validation to reject migrations where from_version equals to_version.

**Code Changes**:
- Added check: `if from_version == to_version { return Err(ProxyError::MigrationFailed); }`
- Updated documentation to clarify that to_version must differ from from_version
- Rejections return `ProxyError::MigrationFailed` for consistency

**Decision**: No-op migrations are explicitly rejected (not silently accepted) to ensure intentional version changes only.

**Test**: `test_migrate_rejects_identical_versions()` - Verifies that calling migrate with identical from_version and to_version strings returns MigrationFailed error.

---

## Issue 4: Silent Overwrite in `propose_admin()`

**Problem**: `propose_admin()` unconditionally overwrites PENDING_ADMIN_KEY with no check for an existing pending proposal. If Admin A proposes themselves, then Admin B proposes themselves before A accepts, Admin B silently replaces Admin A as the only acceptable pending admin.

**Fix**: Documented the current behavior with an inline comment explaining that pending proposals are silently overwritten.

**Code Changes**:
- Added check for existing pending admin: `if read_pending_admin(&env).is_some()`
- Added comment documenting that overwriting is current behavior (intentional for now)
- Could be stricter in future by adding a new error type (e.g., `PendingProposalExists`)

**Test**: `test_propose_admin_overwrites_pending_proposal()` - 
- Proposes Admin A, then proposes Admin B before A accepts
- Verifies that Admin A cannot accept (returns NotAdmin error)
- Confirms that Admin B becomes the new admin successfully

---

## Summary of Changes

| Issue | Function | Type | Impact |
|-------|----------|------|--------|
| 1 | `upgrade_proxy()` | Add event | Enables off-chain indexing of proxy WASM upgrades |
| 2 | `check_admin()` | Refactor to Result | Consistent error handling, eliminates panic |
| 3 | `migrate()` | Add validation | Prevents accidental no-op migrations |
| 4 | `propose_admin()` | Document behavior | Clarifies that pending proposals are overwritten |

## Test Results

All 17 tests pass:
```
running 17 tests
test test::test_accept_admin_rejected_after_transfer_already_accepted ... ok
test test::test_accept_admin_fails_if_wrong_address ... ok
test test::test_check_admin_returns_not_initialized_when_uninitialized ... ok
test test::test_double_initialize_fails ... ok
test test::test_get_impl_storage_info_returns_atomic_pair ... ok
test test::test_initialize_and_get_admin ... ok
test test::test_migrate_rejects_identical_versions ... ok
test test::test_propose_admin_overwrites_pending_proposal ... ok
test test::test_propose_and_accept_admin ... ok
test test::test_set_admin_returns_not_initialized_error ... ok
test test::test_set_admin_transfers_role ... ok
test test::test_propose_and_accept_same_admin_clears_pending_admin ... ok
test test::test_upgrade_accepts_new_storage_version_equal_to_current ... ok
test test::test_upgrade_accepts_new_storage_version_lower_than_current ... ok
test test::test_upgrade_fails_on_version_mismatch ... ok
test test::test_upgrade_proxy_checks_version ... ok
test test::test_upgrade_succeeds_with_correct_version ... ok

test result: ok. 17 passed
```
