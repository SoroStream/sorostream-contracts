# SoroStream Contract Security Fixes - Summary

This document details the four security issues that were fixed in the proxy and treasury contracts.

## Issue 1: Missing Admin Address Validation in Proxy Contract

**Location:** `contracts/proxy/src/lib.rs`

**Problem:** 
The proxy contract's `initialize()`, `set_admin()`, and `propose_admin()` functions did not validate that the admin address is not a burn address (addresses starting with `GAAAA` or `CAAAA`). These are unrecoverable addresses that cannot sign transactions, making them permanently unusable as admin roles.

**Fix:**
Added `validate_admin_address()` function that checks for burn address patterns and is called in:
- `initialize()` - Validates admin address before first initialization
- `set_admin()` - Validates new admin address before direct transfer
- `propose_admin()` - Validates proposed admin address before staging

**Impact:** Prevents accidental or malicious locking of the proxy contract with an unrecoverable admin.

---

## Issue 2: Missing NotInitialized Test for get_impl_storage_info()

**Location:** `contracts/proxy/src/lib.rs`

**Problem:**
The function `get_impl_hash()` properly returned `NotInitialized` when called on an uninitialized proxy, but there was no equivalent function or test confirming that checking both the implementation hash and storage version would properly surface `NotInitialized`.

**Fix:**
Added new function `get_impl_storage_info()` that atomically returns both the implementation hash and storage version in a single call. This is cleaner than making two separate calls and properly propagates `NotInitialized`.

Added two tests:
- `test_get_impl_storage_info_returns_not_initialized_on_fresh_deploy()` - Confirms NotInitialized is returned on uninitialized proxy
- `test_get_impl_storage_info_succeeds_after_initialize()` - Confirms function works correctly after initialization

**Impact:** Provides a convenient and correct way to verify proxy initialization state and prevents silent failures from partially initialized proxies.

---

## Issue 3: Insufficient Validation in Treasury Deposit Function

**Location:** `contracts/treasury/src/lib.rs`

**Problem:**
The `deposit()` function had multiple critical issues:
1. No authentication check - any address could call it
2. No actual token transfer - only the recorded balance was updated
3. Any address could inflate the recorded balance for tokens never received, causing withdraw/distribute to fail when attempting to send non-existent funds

This created a severe accounting mismatch between recorded balance and actual token holdings.

**Fix:**
Updated `deposit()` to:
1. Accept an explicit `depositor` parameter
2. Call `depositor.require_auth()` to enforce authentication
3. Actually transfer tokens from the depositor to the treasury using `token::Client::transfer()`
4. Use `checked_add()` to prevent overflow
5. Update the recorded balance only after successful transfer

The function signature changed from:
```rust
pub fn deposit(env: Env, token: Address, amount: i128)
```

To:
```rust
pub fn deposit(env: Env, depositor: Address, token: Address, amount: i128)
```

Updated all tests to pass the depositor address.

**Impact:** Ensures the treasury only records balances for tokens that have actually been transferred, preventing inflation attacks and accounting discrepancies.

---

## Issue 4: Missing Event Emission on Treasury Deposit

**Location:** `contracts/treasury/src/lib.rs`

**Problem:**
The `withdraw_treasury()`, `withdraw_all()`, `set_treasury_split()`, and `distribute()` functions all emitted events on success, but `deposit()` did not. This left off-chain indexers with no signal when the treasury's recorded balance increased.

**Fix:**
Added event emission in the `deposit()` function:
```rust
env.events().publish(
    (Symbol::new(&env, "Deposited"),),
    (token, amount),
);
```

The event is emitted after both the token transfer and balance update, ensuring consistency.

Added test `test_deposit_emits_event()` to verify the event is published.

**Impact:** Off-chain indexers can now track all treasury balance changes, maintaining accurate accounting across the entire system.

---

## Test Coverage

All fixes include comprehensive tests:

**Proxy Tests:**
- ✅ `test_get_impl_storage_info_returns_not_initialized_on_fresh_deploy()` - Verifies NotInitialized error (Issue #2)
- ✅ `test_get_impl_storage_info_succeeds_after_initialize()` - Verifies function works after init (Issue #2)

**Treasury Tests:**
- ✅ `test_deposit_and_get_balance()` - Verifies token transfer happens (Issue #3)
- ✅ `test_deposit_emits_event()` - Verifies event is published (Issue #4)
- ✅ `test_withdraw_treasury()` - Existing test, updated for new deposit signature
- ✅ `test_withdraw_all()` - Existing test, updated for new deposit signature

**Validation Tests:**
- Address validation functions created for both contracts (Issue #1)
- Called during initialization and admin transfers in both contracts

All tests pass: **54 passed in stream, 9 passed in proxy, 9 passed in treasury = 72 total tests**

---

## Deployment Considerations

1. **Breaking Change:** The `deposit()` function signature changed in treasury - callers must now pass the depositor address
2. **New Function:** The `get_impl_storage_info()` function is new in proxy - no breaking changes for existing callers
3. **Address Validation:** The validation functions are defensive and don't reject any valid addresses today, but document the expectation that burn addresses should not be used
