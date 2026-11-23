# Tests Added for Four Issues

This document lists all the tests that were added to verify the fixes.

## Issue 1: Reputation Decrement on Cancel

### Test: `test_reputation_decrements_on_cancel()`
**Location:** `contracts/streaming/src/test.rs`

**What it tests:**
1. Creates a stream and renews it once to build reputation (reputation = 1)
2. Creates a second stream and cancels it
3. Verifies reputation decrements from 1 to 0

**Verifies:** Cancel operation properly decrements reputation score

```rust
#[test]
fn test_reputation_decrements_on_cancel() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    // Build initial reputation by renewing once
    let stream_id = client.create_stream(...);
    t.approve_renewals(&t.sender, 1_000);
    t.set_time(end);
    client.renew(&stream_id);
    assert_eq!(client.get_reputation(&t.sender), 1);

    // Cancel the new stream and verify reputation decrements
    let stream_id2 = client.create_stream(...);
    t.set_time(end + 500);
    client.cancel(&stream_id2);
    
    assert_eq!(client.get_reputation(&t.sender), 0);
}
```

### Test: `test_reputation_does_not_underflow()`
**Location:** `contracts/streaming/src/test.rs`

**What it tests:**
1. Creates a stream with no prior reputation
2. Cancels it immediately
3. Verifies reputation stays at 0 (saturating subtraction prevents underflow)

**Verifies:** Reputation uses saturating subtraction and cannot go negative

```rust
#[test]
fn test_reputation_does_not_underflow() {
    let t = TestEnv::new();
    let client = t.client();
    let start = t.now();
    let end = start + 1_000;

    // Cancel without any prior reputation (must not panic or underflow)
    let stream_id = client.create_stream(...);
    t.set_time(start + 500);
    client.cancel(&stream_id);

    // Reputation should remain at 0 (saturating subtraction)
    assert_eq!(client.get_reputation(&t.sender), 0);
}
```

---

## Issue 3: bump_ttl() Public Access

### Test: `test_unrelated_address_can_call_bump_ttl()`
**Location:** `contracts/streaming/src/test.rs`

**What it tests:**
1. Creates a stream
2. Advances the TTL near expiration
3. Calls `bump_ttl()` from an unrelated address (not sender, not recipient)
4. Verifies:
   - Function succeeds (returns `Ok(())`)
   - TTL is extended
   - `TtlBumped` event is emitted

**Verifies:** The entry point comment "Anyone may call this entry point" is accurate and tested

```rust
#[test]
fn test_unrelated_address_can_call_bump_ttl() {
    let t = TestEnv::new();
    let start = t.now();
    let stream_id = t.client().create_stream(...);

    let ttl_before = t.env.as_contract(&t.contract, || {
        t.env.storage().persistent().get_ttl(&DataKey::Stream(stream_id))
    });

    // Advance TTL near expiry
    t.set_seq(t.seq() + ttl_before - 40);

    // A third party calls bump_ttl without authentication
    t.env.mock_auths(&[]);
    let result = t.client().try_bump_ttl(&stream_id);

    // Should succeed
    assert!(result.is_ok());

    // TTL should be extended
    let extended_ttl = t.env.as_contract(&t.contract, || {
        t.env.storage().persistent().get_ttl(&DataKey::Stream(stream_id))
    });
    assert!(extended_ttl > ttl_before - 40);

    // Event should be emitted
    let event = t.env.events().all().iter().find_map(|(_, topics, data)| {
        let topic: soroban_sdk::Symbol = topics.get(0)?.into_val(&t.env);
        (topic == soroban_sdk::Symbol::new(&t.env, "TtlBumped"))
            .then(|| data.clone())
    });
    assert!(event.is_some());
}
```

---

## Issue 4: Treasury set_admin() Clears Pending Admin

### Test: `test_set_admin_clears_pending_admin()`
**Location:** `contracts/treasury/src/lib.rs`

**What it tests:**
1. Initializes treasury with admin A
2. Calls `propose_admin()` to propose admin B as pending
3. Calls `set_admin()` to immediately transfer to admin C
4. Verifies:
   - New admin C is now the current admin
   - Pending admin B can no longer accept
   - Attempting to call `accept_admin()` as B returns `NoPendingAdmin` error

**Verifies:** `set_admin()` properly clears the pending admin, preventing old proposals from succeeding

```rust
#[test]
fn test_set_admin_clears_pending_admin() {
    let t = setup();
    let c = TreasuryContractClient::new(&t.env, &t.treasury_id);
    c.initialize(&t.admin);

    let pending_admin = Address::generate(&t.env);
    let new_admin = Address::generate(&t.env);

    // Propose a pending admin
    c.propose_admin(&pending_admin);
    assert_eq!(c.get_admin(), Some(t.admin.clone()));

    // Use set_admin to immediately transfer to a different admin
    c.set_admin(&new_admin);

    // Verify the new admin is set and pending admin is cleared
    assert_eq!(c.get_admin(), Some(new_admin.clone()));

    // Verify that the old pending admin can no longer accept
    let result = c.try_accept_admin(&pending_admin);
    assert_eq!(result, Err(Ok(TreasuryError::NoPendingAdmin)));
}
```

---

## Summary

| Issue | Test Name | Location | Type | 
|-------|-----------|----------|------|
| 1 | `test_reputation_decrements_on_cancel()` | streaming/test.rs | Verify decrement works |
| 1 | `test_reputation_does_not_underflow()` | streaming/test.rs | Verify saturation behavior |
| 3 | `test_unrelated_address_can_call_bump_ttl()` | streaming/test.rs | Verify public entry point |
| 4 | `test_set_admin_clears_pending_admin()` | treasury/lib.rs | Verify pending admin cleared |

**Total Tests Added: 4**

All tests follow the existing test patterns in the codebase and use the established test utilities (`TestEnv`, `setup()`, client creation, etc.).
