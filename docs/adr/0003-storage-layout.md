# ADR-0003: Storage layout and key encoding

## Status

Accepted

## Context

Soroban offers three storage durability levels (instance, persistent, temporary) and charges rent proportional to key+value size. The contract must store:

- Global configuration (admin, pause flag, fee settings, ID counter)
- Per-stream state (the `Stream` struct)
- Per-address indexes (which streams belong to a sender/recipient)
- Idempotency guards (sender + nonce pairs)

An early version stored indexes in temporary storage, causing silent data loss when TTLs lapsed (see issue #1). The storage layout needed to be redesigned with clear durability assignments and a scalable key scheme.

## Decision

### Durability assignments

| Data | Storage type | Rationale |
|------|-------------|-----------|
| `admin`, `paused`, `next_id`, `fee_bps`, `treasury` | Instance | Small, fixed-cardinality config read on many code paths. Survives WASM upgrades. |
| Stream records | Persistent | Must outlive transactions; queried by ID. |
| Sender/recipient indexes | Persistent | Must have same durability as streams to avoid silent inconsistency. |
| Nonce guards | Persistent | Must persist to prevent replay across ledger boundaries. |

Temporary storage is **not used** and should not be reintroduced for any data that clients rely on after the creating transaction completes.

### Key encoding — counter+slot pattern for indexes

Rather than storing a growable `Vec<u64>` per address, sender/recipient indexes use a counter+slot pattern:

- **Counter key**: `("sc", sender_address)` or `("rc", recipient_address)` — stores the number of streams for that address.
- **Slot key**: `("s", sender_address, index)` or `("r", recipient_address, index)` — stores the stream ID at position `index`.

Appending a new stream ID is O(1): read the counter, write the slot at `counter`, increment the counter. Reading all IDs for an address is O(N) where N is the count.

### Key encoding — other keys

| Key | Type | Encoding |
|-----|------|----------|
| Stream record | `u64` | Bare stream ID as persistent key |
| Nonce guard | `(Symbol("n"), Address, u64)` | Tuple of tag, sender, nonce |
| Instance config | `Symbol("admin")`, `Symbol("paused")`, etc. | Short string symbols |

## Alternatives considered

### Vec-based indexes in persistent storage

Store `Vec<u64>` under a single key per address. Rejected because every append re-serializes the entire vector — O(N) writes that grow with stream count. The counter+slot pattern keeps appends at O(1).

### Vec-based indexes in temporary storage (original implementation)

Used in the initial version. Rejected after issue #1: temporary entries expire silently, causing `get_streams_by_sender` to return empty results while `get_stream(id)` still works.

### Enum-based storage keys (e.g., `DataKey::Stream(u64)`)

A single `#[contracttype] enum DataKey` with variants for each key type. This is idiomatic in some Soroban examples. Rejected because:

- Every key would carry the enum discriminant overhead.
- The counter+slot pattern for indexes requires tuple keys `(tag, address, index)` that don't map cleanly to enum variants.
- Bare `u64` keys for streams are maximally compact.

## Consequences

### Positive

- Indexes and streams share the same durability, eliminating silent inconsistency.
- O(1) append for new streams; no re-serialization of growing vectors.
- Compact keys minimize storage rent.
- Instance storage for config survives WASM upgrades with zero migration code.

### Negative

- Counter+slot keys are more numerous than a single Vec key — N+1 persistent entries per address instead of 1. Storage rent scales linearly with total streams per address.
- The short symbol tags (`"s"`, `"r"`, `"sc"`, `"rc"`, `"n"`) are terse and could collide with future key schemes. New key patterns must check for conflicts with these prefixes.
- Slot entries are never compacted: if a sender creates 100 streams and 99 are deleted, the index still has 100 slot entries (some pointing to removed streams). Reads filter stale entries at query time.

### Neutral

- The `remove_stream` function only deletes the stream record from persistent storage; it does not remove the stream ID from sender/recipient indexes. This is by design — the index is append-only, and stale entries are filtered during reads.
