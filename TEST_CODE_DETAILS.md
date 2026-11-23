# Detailed Test Code

## Test 1: Issue #1 - Proposal Removal After Approval

**Location**: `contracts/streaming/src/test.rs` lines 1067–1083

```rust
/// After approve_proposal() succeeds, get_proposal() must return ProposalNotFound,
/// confirming that the proposal storage was actually removed and not cached by the client.
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

**What it tests**:
- Creates a proposal via `setup_proposal()`
- Calls `approve_proposal()` which internally calls `storage::remove_proposal()`
- Verifies that subsequent `get_proposal()` returns the correct typed error

**Why this matters**:
- Ensures proposal storage is actually removed (not just marked inactive)
- Prevents accidental retrieval of old proposals with the same ID
- Validates the contract's storage lifecycle

---

## Test 2: Issue #3 - Atomic Failure on Insufficient Allowance

**Location**: `contracts/streaming/src/test.rs` lines 498–549

```rust
/// When renewing, transfer_from must pull the new deposit from the sender's allowance.
/// If that allowance is insufficient, the entire renew() call must fail atomically:
/// the vested balance must NOT be flushed to the recipient while the deposit pull fails.
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

    // Check recipient's initial balance (should be 0, nothing claimed yet).
    let recipient_balance_before = t.token_client().balance(&t.recipient);

    // Set sender's allowance to the contract to 500, which is LESS than the
    // 1_000 token deposit required for renewal.
    t.token_client()
        .approve(&t.sender, &t.contract, &500, &(t.seq() + 1_000));

    t.set_time(end);
    t.env.mock_auths(&[]);

    // Attempt to renew — must fail because allowance < deposit
    let result = client.try_renew(&stream_id);
    assert!(result.is_err(), "renew should fail due to insufficient allowance");

    // Verify the renewal did NOT partially execute:
    // - Recipient should still have balance_before (not received the vested 1_000)
    // - Stream should still have its original start_time and end_time (not renewed)
    let recipient_balance_after = t.token_client().balance(&t.recipient);
    assert_eq!(
        recipient_balance_after, recipient_balance_before,
        "recipient should not have received vested tokens on failed renewal"
    );

    let stream = client.get_stream(&stream_id);
    assert_eq!(
        stream.start_time, start,
        "stream should not have been renewed"
    );
    assert_eq!(stream.end_time, end, "stream should not have been renewed");
}
```

**What it tests**:
1. Creates an auto-renewing stream with 1_000 token deposit
2. Sets recipient's initial balance baseline
3. Sets sender's allowance to only 500 tokens (insufficient)
4. Advances time past expiry and calls `renew()`
5. Verifies the call fails
6. Confirms no partial execution:
   - Recipient's balance unchanged (no vested tokens sent)
   - Stream's times unchanged (not renewed)

**Why this matters**:
- Token safety: ensures incomplete state doesn't lock funds
- Atomicity: prevents half-updated streams
- Keeper safety: keepers can't accidentally partially drain sender's allowance

**Key invariants checked**:
- `result.is_err()` → renewal failed as expected
- `recipient_balance_after == recipient_balance_before` → no vested tokens transferred
- `stream.start_time == start && stream.end_time == end` → stream not modified

---

## Test 3: Issue #4 - Self-Cosigning

**Location**: `contracts/streaming/src/test.rs` lines 1010–1047

```rust
/// When a sender proposes a stream with themselves as the cosigner, the current
/// contract accepts it. This test verifies that the sender can then call
/// approve_proposal() themselves (as the cosigner), unilaterally activating
/// their own proposal without requiring a second party.
///
/// **This documents the current (permissive) behavior. Future versions may add
/// a check to enforce sender ≠ cosigner if desirable.**
#[test]
fn test_sender_can_self_cosign_own_proposal() {
    let t = TestEnv::new();
    let client = t.client();

    // Sender proposes a stream, naming themselves as the cosigner.
    let proposal_id = client.propose_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &1_000_u64,
        &false,
        &t.sender,  // cosigner == sender (self-cosign)
        &100,
    );

    // Verify the proposal was created with sender as both sender and cosigner.
    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.sender, t.sender);
    assert_eq!(proposal.cosigner, t.sender);

    // The sender can now approve their own proposal, acting in the cosigner role.
    // This demonstrates that the contract does NOT enforce sender ≠ cosigner.
    let stream_id = client.approve_proposal(&proposal_id);

    // Verify the stream was activated.
    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
    assert_eq!(stream.deposit, 1_000);
}
```

**What it tests**:
1. Sender calls `propose_stream()` with themselves as cosigner
2. Verifies proposal stored correctly (`sender == cosigner`)
3. Sender calls `approve_proposal()` (acting as cosigner)
4. Verifies stream is activated

**Why this matters**:
- Documents that the contract doesn't validate `sender ≠ cosigner`
- Makes it explicit that the 2-of-2 flow is permissive (allows self-approval)
- Guides integrators on whether client-side validation is needed

**Key checks**:
- `proposal.sender == proposal.cosigner` → self-cosigning allowed
- `approve_proposal()` doesn't reject → no validation on sender ≠ cosigner
- Stream is fully activated → self-approval is functional

---

## Helper Functions Used

All tests use the `TestEnv` helper defined earlier in the file:

```rust
struct TestEnv {
    env: Env,
    sender: Address,
    recipient: Address,
    token: Address,
    contract: Address,
}
```

**Key methods**:
- `new()` → Creates fresh environment with pre-funded accounts
- `client()` → Returns contract client
- `token_client()` → Returns token client
- `set_time(ts)` → Advances ledger timestamp
- `set_seq(seq)` → Advances ledger sequence
- `approve_renewals()` → Sets token allowance for renewal

**Utility function**:
```rust
fn setup_proposal(t: &TestEnv, window: u32) -> u64 {
    t.client().propose_stream(
        &t.sender,
        &t.recipient,
        &t.token,
        &1_000_i128,
        &1_000_u64,
        &false,
        &t.recipient, // co-signer = recipient
        &window,
    )
}
```

---

## Running the Tests

To run all three new tests:

```bash
cd /workspaces/sorostream-contracts/contracts/streaming
cargo test test_get_proposal_returns_not_found_after_approval \
            test_renew_fails_atomically_on_insufficient_allowance \
            test_sender_can_self_cosign_own_proposal -- --nocapture
```

Or run all tests:

```bash
cargo test --lib
```

---

## Expected Test Output

All three tests should **pass** with output showing:
- `test test_get_proposal_returns_not_found_after_approval ... ok`
- `test test_renew_fails_atomically_on_insufficient_allowance ... ok`
- `test test_sender_can_self_cosign_own_proposal ... ok`

