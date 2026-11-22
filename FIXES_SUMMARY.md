# Four Critical Issues Fixed

This document summarizes the four issues that have been fixed in this repository.

## Issue 1: Cancel() Doesn't Penalize Reputation

**Problem:** The `cancel()` function in `contracts/streaming/src/lib.rs` noted "Cancelled streams do NOT increment reputation," but there was also no decrement or penalty. A sender could repeatedly `create_stream()` then immediately `cancel()` with zero reputation consequence, making the reputation score easy to leave unaffected by bad-faith stream churn.

**Solution:**
1. Added `decrement_reputation()` function in `contracts/streaming/src/storage.rs` that uses saturating subtraction to prevent underflow
2. Modified `cancel()` to call `storage::decrement_reputation(&env, &stream.sender)` after marking the stream as cancelled
3. Updated the doc comment to: "Cancelled streams decrement the sender's reputation score."

**Files Changed:**
- `contracts/streaming/src/lib.rs`: Updated `cancel()` function and docstring
- `contracts/streaming/src/storage.rs`: Added `decrement_reputation()` function

**Tests Added:**
- `test_reputation_decrements_on_cancel()`: Verifies reputation decrements when a stream is cancelled after reaching positive reputation
- `test_reputation_does_not_underflow()`: Verifies that cancelling when reputation is 0 doesn't cause underflow (stays at 0)

---

## Issue 2: Duplicate Stream ID Derivation Logic

**Problem:** `contracts/streaming/src/lib.rs` implemented the identical "derive a stream id via storage::derive_stream_id, then loop incrementing retry while storage::stream_id_exists" pattern independently in `create_stream()`, `sponsor_stream()`, and `approve_proposal()`. The three copies could silently diverge if the collision-handling logic was ever changed in only one of them.

**Solution:**
1. Extracted the collision-handling logic into a new helper function `derive_stream_id_with_collision_handling()` in `contracts/streaming/src/storage.rs`
2. This function takes an initial nonce and handles collision retries internally
3. Updated all three callers (`create_stream()`, `sponsor_stream()`, `approve_proposal()`) to use this shared helper

**Files Changed:**
- `contracts/streaming/src/storage.rs`: Added `derive_stream_id_with_collision_handling()` helper
- `contracts/streaming/src/lib.rs`: Updated `create_stream()`, `sponsor_stream()`, and `approve_proposal()` to use the helper

**Benefits:**
- Single source of truth for collision-handling logic
- Prevents silent divergence between implementations
- Easier to maintain and test

---

## Issue 3: No Test for bump_ttl() Public Entry Point

**Problem:** The `bump_ttl()` doc comment stated "Anyone may call this entry point," but there was no test confirming an address with no relationship to the stream (not sender, not recipient) could successfully call `bump_ttl()` and have it succeed and emit `TtlBumped`.

**Solution:**
1. Added comprehensive test `test_unrelated_address_can_call_bump_ttl()` in `contracts/streaming/src/test.rs`
2. The test verifies:
   - A third party (unrelated address) can call `bump_ttl()` without authentication
   - The function succeeds (returns `Ok(())`)
   - The TTL is extended
   - The `TtlBumped` event is emitted

**Files Changed:**
- `contracts/streaming/src/test.rs`: Added `test_unrelated_address_can_call_bump_ttl()` test

---

## Issue 4: Treasury set_admin() Doesn't Clear PENDING_ADMIN_KEY

**Problem:** `contracts/treasury/src/lib.rs`'s `set_admin()` writes `ADMIN_KEY` directly but never removes `PENDING_ADMIN_KEY`. If `propose_admin(X)` was called and then `set_admin(Y)` was used instead, `X` could still later call `accept_admin()` and become admin, silently overriding `Y`.

**Solution:**
1. Modified `set_admin()` in `contracts/treasury/src/lib.rs` to remove `PENDING_ADMIN_KEY` after setting the new admin
2. This prevents old pending admin proposals from succeeding after a direct admin transfer
3. Added explicit comment explaining the security rationale

**Files Changed:**
- `contracts/treasury/src/lib.rs`: Updated `set_admin()` to clear `PENDING_ADMIN_KEY`

**Tests Added:**
- `test_set_admin_clears_pending_admin()`: Verifies that after calling `set_admin()`, a previous pending admin can no longer call `accept_admin()`

---

## Summary of Changes

| Issue | File | Type | Change |
|-------|------|------|--------|
| 1 | storage.rs | New Function | Added `decrement_reputation()` |
| 1 | lib.rs (streaming) | Modified Function | Updated `cancel()` to decrement reputation |
| 1 | test.rs | New Tests | Added 2 reputation decrement tests |
| 2 | storage.rs | New Function | Added `derive_stream_id_with_collision_handling()` |
| 2 | lib.rs (streaming) | Modified Functions | Updated 3 functions to use helper |
| 3 | test.rs | New Test | Added `test_unrelated_address_can_call_bump_ttl()` |
| 4 | lib.rs (treasury) | Modified Function | Updated `set_admin()` to clear pending admin |
| 4 | lib.rs (treasury) | New Test | Added `test_set_admin_clears_pending_admin()` |

## Verification

All changes maintain backward compatibility and existing behavior:
- Cancel still works exactly as before, just with reputation penalty now
- Stream ID derivation logic is functionally identical, just centralized
- bump_ttl works as documented (anyone can call it)
- set_admin works the same way, now with correct cleanup

The fixes are defensive and prevent potential attack vectors while maintaining all existing functionality.
