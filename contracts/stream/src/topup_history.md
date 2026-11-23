# feat/38: Stream Top-Up History

Closes #637

## Summary

Maintain a historical ring buffer of all top-up operations per stream for
transparency and auditing purposes.

## Acceptance Criteria

- [ ] Each stream stores a ring buffer of up to 20 top-up events (amount, timestamp, caller)
- [ ] `get_stream_topup_history(stream_id)` returns the buffer
- [ ] Unit test: 25 top-ups, history contains only the last 20

## Design Notes

- Ring buffer key: `(Symbol("tuh"), stream_id, slot_index)` in persistent storage
- Slot index cursor stored at `(Symbol("tuhc"), stream_id)`, wraps at 20
- `TopUpEvent` struct: `{ amount: i128, timestamp: u64, caller: Address }`
- `top_up` writes a new entry into the ring buffer on every successful call
- `get_stream_topup_history` reads all 20 (or fewer) slots and returns them in order
- Branch: `feat/38-topup-history`
