# feat/30: Multi-Recipient Stream

Closes #631

## Summary

Add `create_multi_recipient_stream` entry point that fans out a single deposit
to N recipients with configurable BPS percentage splits.

## Acceptance Criteria

- [ ] `create_multi_recipient_stream(recipients: Vec<(Address, bps)>)` entry point added
- [ ] Sum of all bps must equal 10,000
- [ ] On withdrawal, each recipient gets their proportional share
- [ ] Unit test: 3 recipients, 50% / 30% / 20% split, verify withdrawals

## Design Notes

- Each recipient gets an independent sub-stream derived from their BPS share of the total deposit
- BPS validation: `sum(bps) == 10_000` else `BatchLengthMismatch`
- Sub-stream nonces: `nonce + 0`, `nonce + 1`, ..., `nonce + N-1`
- Branch: `feat/30-multi-recipient-stream`
