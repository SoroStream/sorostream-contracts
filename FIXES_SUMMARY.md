# Factory Contract Fixes — Summary

This document summarizes the fixes applied to `contracts/factory/src/lib.rs` and `contracts/factory/src/test.rs` to address four issues with pagination, deprecation, and edge-case handling.

## Issues Fixed

### Issue #1: Test for limit == 0 edge case
**File:** `contracts/factory/src/test.rs`

**Test Added:** `test_get_deployed_contracts_with_zero_limit_returns_empty_page()`

**Description:** 
The `get_deployed_contracts()` function computes `limit.min(MAX_DEPLOYMENT_PAGE_SIZE)`, which means `limit == 0` should produce an empty page. Rather than relying on the formula, this test explicitly verifies the behavior to prevent callers from mistakenly expecting `limit=0` to mean "no limit" (which would default to `MAX_DEPLOYMENT_PAGE_SIZE`).

**What the test does:**
- Deploys 3 contracts
- Calls `get_deployed_contracts(0, 0)` (zero limit)
- Asserts the returned page is empty
- Documents the formula behavior in a test rather than leaving it implicit

---

### Issue #2: Test for start > count edge case
**File:** `contracts/factory/src/test.rs`

**Tests Added:**
- `test_get_deployed_contracts_with_start_beyond_count_returns_empty()`
- `test_get_deployed_contracts_with_start_beyond_count_no_panic_various_limits()`

**Description:**
Both `get_deployed_contracts()` and `get_active_deployed_contracts()` compute:
```
end = start.saturating_add(limit.min(MAX_DEPLOYMENT_PAGE_SIZE)).min(count)
```

When `start > count`, the loop range becomes invalid (e.g., `10..3`). Without explicit testing, it's unclear whether this produces an empty `Vec` or panics. These tests confirm the safe behavior.

**What the tests do:**
- Deploy a known number of contracts (3–5)
- Call pagination functions with `start > count`
- Verify an empty `Vec` is returned (not a panic)
- Test multiple combinations of `start` and `limit` values
- Confirm loop range `start..end` is empty when `end < start`

---

### Issue #3: Add undeprecate_deployment function
**File:** `contracts/factory/src/lib.rs`

**Function Added:** `undeprecate_deployment(env, admin, contract_address) -> Result<(), FactoryError>`

**Tests Added:**
- `test_undeprecate_deployment_reverts_deprecated_to_active()`
- `test_undeprecate_deployment_on_unknown_contract_fails()`
- `test_undeprecate_deployment_non_admin_rejected()`

**Description:**
Previously, `DeploymentStatus` could only transition from `Active` to `Deprecated` via `deprecate_deployment()`. An admin who deprecates a deployment by mistake or whose reasons for deprecating no longer apply had no on-chain path to undo it short of a fresh deployment.

**What the fix does:**
- Adds a new `undeprecate_deployment()` function that mirrors `deprecate_deployment()` but sets status back to `Active`
- Requires admin authentication (same as `deprecate_deployment()`)
- Returns `FactoryError::UnknownDeployment` if the contract address was never deployed through the factory
- Returns `FactoryError::Unauthorized` if the caller is not the admin
- Publishes a `"DeploymentUndeprecated"` event when successful
- Allows contracts to re-appear in `get_active_deployed_contracts()` listings after being undeprecated

---

### Issue #4: Document and test get_active_deployed_contracts fallback behavior
**File:** `contracts/factory/src/test.rs`

**Test Added:** `test_get_active_deployed_contracts_missing_info_treated_as_active()`

**Description:**
The `get_active_deployed_contracts()` function computes:
```rust
let is_active = env
    .storage()
    .persistent()
    .get(&info_key(&env, &address))
    .map(|info| info.status == DeploymentStatus::Active)
    .unwrap_or(true);  // <-- Silent default: missing info = active
```

If a `contract_index_key` entry has no matching `DeploymentInfo`, the fallback treats it as active. This is a silent default rather than an explicit policy comment, so it needed documentation via test.

**What the test does:**
- Deploys two contracts normally
- Manually removes the `DeploymentInfo` for one contract to simulate storage inconsistency or legacy migration
- Calls `get_active_deployed_contracts(0, 20)`
- Asserts both contracts (including the one with missing info) appear in the result
- Confirms `get_deployment_info()` returns `None` for the incomplete entry
- Documents the fallback behavior with comments

**Rationale:**
This edge case might occur in production due to storage inconsistency or during ledger migrations. The test ensures the fallback behavior is explicit and intentional, preventing silent bugs from unclear defaults.

---

## Files Modified

1. **`contracts/factory/src/lib.rs`** (+30 lines)
   - Added `undeprecate_deployment()` function

2. **`contracts/factory/src/test.rs`** (+155 lines)
   - Added 6 new test functions:
     - Issue #1: 1 test
     - Issue #2: 2 tests
     - Issue #3: 3 tests
     - Issue #4: 1 test

## Testing

All tests follow the existing test patterns:
- Use `env.mock_all_auths()` for authority simulation
- Use `upload_child_wasm()` fixture for WASM deployments
- Use `fake_salt()` helper for deterministic salts
- Use `try_*` variants for error cases and manual assertion
- Include comments documenting the formula and expected behavior

**To run tests (requires Rust toolchain):**
```bash
cd contracts/factory
cargo test --lib
```

## Backward Compatibility

All changes are backward-compatible:
- The new `undeprecate_deployment()` function is additive and optional
- Existing tests remain unchanged
- No breaking changes to existing contract functions or storage layout
- New tests only exercise existing code paths or the new function
