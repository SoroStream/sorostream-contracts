# Before & After Comparison

## Issue #1: Proposal Removal Test

### Before
No test verifying proposal removal after approval.

### After
```rust
#[test]
fn test_get_proposal_returns_not_found_after_approval() {
    let t = TestEnv::new();
    let proposal_id = setup_proposal(&t, 100);
    
    // Approve the proposal
    t.client().approve_proposal(&proposal_id);
    
    // Attempting to retrieve the proposal must return ProposalNotFound
    let result = t.client().try_get_proposal(&proposal_id);
    assert!(matches!(
        result,
        Err(Ok(StreamingError::ProposalNotFound)),
    ));
}
```

**Impact**: Developers now have confidence that proposals are properly cleaned up.

---

## Issue #2: Approval Window Documentation

### Before
```rust
/// Propose a stream that requires a co-signer's approval before activation.
///
/// The sender's deposit is held by the contract. If the co-signer does not
/// approve within `approval_window_ledgers` ledgers, anyone can call
/// `discard_proposal` to return the deposit. Before expiry, only the sender
/// can retract the proposal.
```

**Problem**: No explanation that timeout is ledger-sequence-based, not wall-clock.

### After
```rust
/// Propose a stream that requires a co-signer's approval before activation.
///
/// The sender's deposit is held by the contract. If the co-signer does not
/// approve within `approval_window_ledgers` ledgers, anyone can call
/// `discard_proposal` to return the deposit. Before expiry, only the sender
/// can retract the proposal.
///
/// # Approval Window
/// The `approval_window_ledgers` parameter specifies a ledger-sequence-based
/// timeout: the proposal expires after `env.ledger().sequence() + approval_window_ledgers`.
/// **There is no parallel wall-clock timeout.** Ledger production rate is not
/// perfectly constant (typically ~6 seconds on average in Soroban mainnet, but
/// can vary significantly). Integrators reasoning in wall-clock time should
/// convert the approval window accordingly. For example, to allow 1 hour:
/// - Assume typical ~6-second ledger time: 1 hour ÷ 6s/ledger ≈ 600 ledgers
/// - Use a safety margin; e.g., request 500–600 ledgers depending on risk tolerance
```

**Impact**: Integrators now understand the timeout mechanism and how to convert wall-clock time.

---

## Issue #3: Atomic Renewal Failure Test

### Before
No test verifying atomicity on allowance failure.

### After
```rust
#[test]
fn test_renew_fails_atomically_on_insufficient_allowance() {
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

    // Check recipient's initial balance
    let recipient_balance_before = t.token_client().balance(&t.recipient);

    // Set insufficient allowance (500 < 1_000)
    t.token_client()
        .approve(&t.sender, &t.contract, &500, &(t.seq() + 1_000));

    t.set_time(end);
    t.env.mock_auths(&[]);

    // Attempt to renew — must fail
    let result = client.try_renew(&stream_id);
    assert!(result.is_err(), "renew should fail");

    // Verify no partial execution
    let recipient_balance_after = t.token_client().balance(&t.recipient);
    assert_eq!(recipient_balance_after, recipient_balance_before);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.start_time, start);
    assert_eq!(stream.end_time, end);
}
```

**Impact**: Developers and keepers can rely on atomicity—renewal either fully succeeds or fails cleanly.

---

## Issue #4: Self-Cosigning Behavior

### Before (Documentation)
```rust
/// Co-signer approves a pending proposal, activating the stream.
///
/// The stream's `start_time` is set to the current ledger timestamp at the
/// moment of approval.
```

**Problem**: No mention of self-cosigning behavior.

### After (Documentation)
```rust
/// Co-signer approves a pending proposal, activating the stream.
///
/// The stream's `start_time` is set to the current ledger timestamp at the
/// moment of approval.
///
/// # Note on Self-Cosigning
/// The contract does not enforce that `cosigner ≠ sender`. A sender can propose
/// a stream naming themselves as the cosigner, then approve their own proposal
/// unilaterally. If the 2-of-2 flow should strictly require two distinct parties,
/// integrators must validate `sender ≠ cosigner` on the client side.
```

### Before (Test)
No test for self-cosigning scenario.

### After (Test)
```rust
#[test]
fn test_sender_can_self_cosign_own_proposal() {
    let t = TestEnv::new();
    let client = t.client();

    // Sender proposes with themselves as cosigner
    let proposal_id = client.propose_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &1_000_u64,
        &false,
        &t.sender,  // cosigner == sender
        &100,
    );

    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.sender, t.sender);
    assert_eq!(proposal.cosigner, t.sender);

    // Sender approves their own proposal
    let stream_id = client.approve_proposal(&proposal_id);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
    assert_eq!(stream.deposit, 1_000);
}
```

**Impact**: Behavior is explicitly documented and tested; integrators can decide if they need stricter validation.

---

## Summary of Changes

| Aspect | Before | After |
|--------|--------|-------|
| Proposal removal verification | ❌ Not tested | ✅ Test added |
| Approval window documentation | ⚠️ Vague | ✅ Clear with examples |
| Renewal atomicity | ❌ Not tested | ✅ Test added |
| Self-cosigning behavior | ❌ Undocumented | ✅ Documented + tested |
| Total tests | 37 | 40 |
| Breaking changes | N/A | 0 |
| Integration guidance | Limited | Comprehensive |

---

## Code Quality Improvements

### Documentation
- **Before**: 2 function comments
- **After**: 2 function comments + 2 detailed sections = 4 information blocks

### Test Coverage
- **Before**: 37 unit tests covering core flows
- **After**: 40 unit tests including edge cases and failure scenarios

### Developer Experience
- **Before**: Ambiguous behavior around proposals, timeouts, and self-cosigning
- **After**: Clear semantics with working examples and guidance

---

## Migration Path

No migration needed. All changes are:
- ✅ Backward compatible
- ✅ Additive (only new tests and docs)
- ✅ Non-breaking

Existing integrations continue to work unchanged.

