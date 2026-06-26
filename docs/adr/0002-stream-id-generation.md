# ADR-0002: Monotonic counter for stream IDs

## Status

Accepted

## Context

Every stream needs a unique identifier that can be used as a storage key, returned to callers, and referenced in events. The ID scheme must satisfy:

1. **Uniqueness** — no two streams may share an ID, even across different senders.
2. **Determinism** — the same transaction must produce the same ID in simulation and submission.
3. **Compact storage** — IDs are used as persistent storage keys; smaller keys reduce rent.
4. **Enumeration** — `get_all_stream_ids` iterates over all streams by scanning the ID range `[0, next_id)`.

## Decision

Use a global monotonic `u64` counter stored in instance storage under the key `"next_id"`. The function `next_stream_id` atomically reads the current value, increments it, and returns the old value as the new stream's ID.

```
stream_id = counter
counter   = counter + 1
```

IDs start at 0 and are never reused: when a stream is removed (on non-auto-renew completion), its ID slot in persistent storage is deleted, but the counter is not decremented.

## Alternatives considered

### Hash-based IDs (e.g., SHA-256 of sender + nonce + timestamp)

Would produce collision-resistant IDs without a shared counter. Rejected because:

- Hash IDs are 32 bytes vs 8 bytes for `u64`, increasing storage cost for every key that references a stream ID (the stream itself, sender/recipient index slots, events).
- Enumeration (`get_all_stream_ids`) would require maintaining a separate list or set, adding complexity.
- The nonce-based deduplication mechanism (`DuplicateStream` error) already ensures uniqueness per sender; a global counter trivially extends this to cross-sender uniqueness.

### Per-sender sequential IDs (sender, sequence_number)

Would avoid the global counter contention. Rejected because:

- Composite keys `(Address, u64)` are larger than a bare `u64`.
- Cross-sender queries (`get_all_stream_ids`, `get_stats`) would need to iterate over all known senders, which is O(senders × streams_per_sender) instead of O(total_streams).
- Soroban transactions are single-threaded per contract instance, so counter contention is not a concern.

### UUID / random IDs

Rejected because Soroban's execution environment does not provide a secure random number generator, and `ledger().timestamp()` does not have sufficient entropy for collision resistance.

## Consequences

### Positive

- IDs are compact (8 bytes), reducing storage key size.
- `get_all_stream_ids(start, limit)` is a simple range scan — no auxiliary index needed.
- ID assignment is O(1) — one read and one write to instance storage.
- IDs are human-readable and sequential, simplifying debugging and event correlation.

### Negative

- Deleted streams leave gaps in the ID space. `get_all_stream_ids` must check `load_stream` for each ID in the range, making it O(range) rather than O(live_streams). This is acceptable because the pagination limit (20) bounds the per-call cost.
- The counter lives in instance storage, which is shared across all contract calls. This is fine for Soroban's single-threaded execution model but would be a bottleneck in a concurrent execution environment.
- IDs are predictable, which could be a concern if stream IDs were used as authorization tokens. They are not — all mutations require `require_auth()` from the sender or recipient.

### Neutral

- The `u64` counter will not overflow in any realistic scenario. At one stream per second, it would take ~584 billion years to exhaust.
