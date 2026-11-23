# feat/36: Claim All Streams

Closes #639

## Summary

A convenience entry point that claims all outstanding balances across all
active streams for the caller in one transaction, optimised for gas efficiency.

## Acceptance Criteria

- [ ] `claim_all_streams()` entry point claims from all active streams for the caller
- [ ] Returns a `Vec<(u64, i128)>` of stream ID and amount claimed
- [ ] Gas-optimised: fewer storage reads than N individual withdraw calls
- [ ] Unit test: recipient with 5 streams claims all at once

## Design Notes

- Uses `get_ids_by_recipient(env, caller)` to enumerate all stream IDs for caller
- Iterates streams, skips non-Active / zero-claimable, processes up to 50 streams
- Single `require_auth` call at entry point covers all sub-withdrawals
- Returns `Vec<(stream_id, amount_claimed)>` skipping streams with 0 claimable
- Branch: `feat/36-claim-all-streams`
