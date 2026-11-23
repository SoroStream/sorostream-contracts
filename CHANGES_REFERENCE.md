# Exact Changes Reference

## File: contracts/streaming/src/lib.rs

### Change 1: Updated `propose_stream()` documentation (Issue #2)
**Lines 309-324** - Added `# Approval Window` section explaining:
- Ledger-sequence-based timeout (not wall-clock)
- Typical ~6-second ledger time
- Example: 1 hour ≈ 600 ledgers with safety margin

### Change 2: Updated `approve_proposal()` documentation (Issue #4)
**Lines 393-402** - Added `# Note on Self-Cosigning` section explaining:
- Contract does NOT enforce `cosigner ≠ sender`
- Sender can self-cosign and unilaterally activate
- Integrators should validate on client side if stricter semantics needed

---

## File: contracts/streaming/src/test.rs

### Addition 1: Test for Issue #1 (Lines 1067-1083)
**Function**: `test_get_proposal_returns_not_found_after_approval()`
- Creates a proposal
- Calls `approve_proposal()`
- Verifies `get_proposal()` returns `ProposalNotFound`

### Addition 2: Test for Issue #3 (Lines 498-549)
**Function**: `test_renew_fails_atomically_on_insufficient_allowance()`
- Creates auto-renewing stream
- Sets insufficient allowance (500 < 1_000)
- Attempts renewal and verifies it fails
- Confirms no partial execution:
  - Recipient doesn't receive vested tokens
  - Stream doesn't get renewed

### Addition 3: Test for Issue #4 (Lines 1010-1047)
**Function**: `test_sender_can_self_cosign_own_proposal()`
- Sender proposes stream with themselves as cosigner
- Verifies proposal structure
- Sender calls `approve_proposal()` themselves
- Verifies stream activation

---

## Code Quality Notes

- All new tests follow existing patterns from `TestEnv`
- Tests use `assert!` and `matches!` macros consistently
- Clear inline comments explain each test scenario
- No external dependencies added
- All changes are backward compatible

---

## Validation Checklist

✅ Issue #1: `get_proposal()` returns `ProposalNotFound` after `approve_proposal()`
✅ Issue #2: `propose_stream()` documents ledger-sequence-based timeout with guidance
✅ Issue #3: `renew()` fails atomically on insufficient allowance
✅ Issue #4: Self-cosigning is tested and documented as permissive behavior

