# Fixed Issues Summary

This document summarizes the four issues fixed in the SoroStream streaming contract.

## Issue 1: Verify Proposal Removal After Approval

**Description**: After `approve_proposal()` succeeds and calls `storage::remove_proposal()`, confirm that `get_proposal()` returns `StreamingError::ProposalNotFound` rather than stale cached data.

**Solution**: Added test `test_get_proposal_returns_not_found_after_approval()` in `contracts/streaming/src/test.rs` (line 1067).

**What the test does**:
- Creates a proposal via `propose_stream()`
- Calls `approve_proposal()` to activate the stream
- Attempts to retrieve the proposal via `get_proposal()`
- Asserts that the result is `Err(Ok(StreamingError::ProposalNotFound))`

**Result**: Confirms the proposal storage is properly removed and not cached.

---

## Issue 2: Document Ledger-Sequence-Based Approval Window

**Description**: Frame `approval_window_ledgers` in terms of ledger sequence, not wall-clock time. Warn integrators that ledger production rate varies and they should convert accordingly.

**Solution**: Updated `propose_stream()` doc comment in `contracts/streaming/src/lib.rs` (line 309).

**Key documentation added**:
```
# Approval Window
The `approval_window_ledgers` parameter specifies a ledger-sequence-based
timeout: the proposal expires after `env.ledger().sequence() + approval_window_ledgers`.
**There is no parallel wall-clock timeout.** Ledger production rate is not
perfectly constant (typically ~6 seconds on average in Soroban mainnet, but
can vary significantly). Integrators reasoning in wall-clock time should
convert the approval window accordingly. For example, to allow 1 hour:
- Assume typical ~6-second ledger time: 1 hour ÷ 6s/ledger ≈ 600 ledgers
- Use a safety margin; e.g., request 500–600 ledgers depending on risk tolerance
```

**Result**: Integrators are now warned that the timeout is ledger-sequence-based, not wall-clock, and have guidance on conversion.

---

## Issue 3: Test Atomic Failure on Insufficient Renewal Allowance

**Description**: When `renew()` calls `token::Client::transfer_from` to pull the new deposit, test that if the sender's allowance is insufficient, the entire call fails atomically—vested tokens are NOT flushed to the recipient while the deposit pull fails.

**Solution**: Added test `test_renew_fails_atomically_on_insufficient_allowance()` in `contracts/streaming/src/test.rs` (line 498).

**What the test does**:
- Creates a stream with 1_000 token deposit and auto_renew enabled
- Revokes the sender's allowance, setting it to only 500 tokens (less than required 1_000)
- Advances time past expiry and attempts renewal
- Asserts that renewal fails (`result.is_err()`)
- Verifies that:
  - Recipient's balance is unchanged (no vested tokens flushed)
  - Stream's start_time and end_time remain unchanged (not renewed)

**Result**: Confirms that renewal fails atomically without partial execution.

---

## Issue 4: Test and Document Self-Cosigning

**Description**: The contract accepts any Address as cosigner with no check for `cosigner ≠ sender`. Add a test where a sender names themselves as cosigner, confirming whether `approve_proposal()` lets them unilaterally activate their own proposal, and document the behavior.

**Solution**: 
1. Added test `test_sender_can_self_cosign_own_proposal()` in `contracts/streaming/src/test.rs` (line 1010).
2. Updated `approve_proposal()` doc comment in `contracts/streaming/src/lib.rs` (line 393).

**Test verification**:
- Sender proposes a stream with themselves as the cosigner
- Verifies the proposal is created correctly (sender == cosigner)
- Sender calls `approve_proposal()` themselves
- Asserts the stream is activated successfully

**Documentation added**:
```
# Note on Self-Cosigning
The contract does not enforce that `cosigner ≠ sender`. A sender can propose
a stream naming themselves as the cosigner, then approve their own proposal
unilaterally. If the 2-of-2 flow should strictly require two distinct parties,
integrators must validate `sender ≠ cosigner` on the client side.
```

**Result**: The current permissive behavior is documented. Integrators are advised to add client-side validation if stricter 2-of-2 semantics are desired.

---

## Files Modified

1. **contracts/streaming/src/lib.rs**
   - Updated `propose_stream()` doc comment (issue #2)
   - Updated `approve_proposal()` doc comment (issue #4)

2. **contracts/streaming/src/test.rs**
   - Added `test_get_proposal_returns_not_found_after_approval()` (issue #1)
   - Added `test_renew_fails_atomically_on_insufficient_allowance()` (issue #3)
   - Added `test_sender_can_self_cosign_own_proposal()` (issue #4)

---

## Testing

All tests follow the existing test patterns:
- Use the shared `TestEnv` helper for setup
- Exercise both positive and negative paths
- Verify invariants after each operation
- Include clear comments explaining the test scenario and assertions

The tests are designed to be run with `cargo test` in the streaming contract directory.
