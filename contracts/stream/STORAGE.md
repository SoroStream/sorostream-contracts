# Soroban Storage Key Encoding Scheme

This document explains how the SoroStream contract encodes storage keys using the Soroban SDK's `Symbol` and `Val` types. Understanding this scheme is critical for contributors adding new storage entries — incorrect encoding causes silent key collisions and data corruption.

**Source:** `contracts/stream/src/storage.rs`

---

## How Soroban encodes keys

Soroban stores data in ledger entries keyed by `Val` values. The SDK provides several ways to construct keys:

| Soroban type | Encoding | Typical use |
|-------------|----------|-------------|
| `u64` (bare) | 8-byte big-endian integer | Stream records (keyed by stream ID) |
| `Symbol` | Short string interned into the environment | Config keys, tuple discriminators |
| `(Symbol, T)` | Tuple of Symbol + value | Per-address data (counts, flags) |
| `(Symbol, T, U)` | 3-element tuple | Indexed slots (address + position) |

Tuples are encoded by serializing each element in order. Soroban uses a tagged-union representation: tuples become a sequence of `Val`s. The important property is that **different tuple shapes and different Symbol prefixes occupy distinct key namespaces**, preventing collisions — as long as prefixes are chosen carefully.

---

## All storage keys used by SoroStream

### Instance storage (global config)

These use bare `Symbol` keys. Each is a short, unique string constant defined at the top of `storage.rs`:

| Constant | Symbol string | Value type | Purpose |
|----------|--------------|------------|---------|
| `ADMIN_KEY` | `"admin"` | `Address` | Contract administrator |
| `PAUSED_KEY` | `"paused"` | `bool` | Emergency pause flag |
| `PROTOCOL_FEE_KEY` | `"fee_bps"` | `u32` | Protocol fee in basis points |
| `TREASURY_KEY` | `"treasury"` | `Address` | Fee recipient address |
| `MIN_DURATION_KEY` | `"min_dur"` | `u64` | Minimum stream duration |
| `VERSION_KEY` | `"version"` | `String` | Contract version string |
| `MAX_STREAMS_KEY` | `"max_str"` | `u32` | Max streams per sender |
| `STREAM_COUNT_KEY` | `"str_cnt"` | `u32` | Global stream count |
| `PENDING_FEE_KEY` | `"pnd_fee"` | `(u32, u64)` | Pending fee proposal |
| `WITHDRAWAL_COOLDOWN_KEY` | `"wd_cd"` | `u64` | Withdrawal cooldown seconds |
| `WHITELIST_ENABLED_KEY` | `"wl_en"` | `bool` | Whitelist enabled flag |
| `GUARDIAN_KEY` | `"guardian"` | `Address` | Guardian address |
| `GOVERNANCE_KEY` | `"governance"` | `Address` | Governance address |
| `PAUSE_EXPIRES_KEY` | `"p_exp"` | `u64` | Pause auto-expiry timestamp |
| `CREATION_FEE_XLM_KEY` | `"cf_xlm"` | `i128` | Flat XLM creation fee |
| `XLM_TOKEN_KEY` | `"xlm_tok"` | `Address` | XLM SAC token address |
| `APPLIED_MIGRATIONS_KEY` | `"migrations"` | `Vec<String>` | Applied migration versions |
| `REENTRANCY_LOCK_KEY` | `"re_lk"` | `bool` | Reentrancy lock (temporary) |
| `AUDIT_HEAD_KEY` | `"al_head"` | `u32` | Audit buffer head pointer |
| `AUDIT_LEN_KEY` | `"al_len"` | `u32` | Audit buffer fill level |

**Annotated example — reading admin:**

```rust
// storage.rs:24-27
pub fn write_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set(&Symbol::new(env, "admin"), admin);
}
// Key: Symbol("admin") → 3-byte short string in Soroban intern table
// Value: Address (32-byte Ed25519 public key)
```

### Persistent storage — stream records

Streams are keyed by a bare `u64` (the stream ID). This is the simplest key type:

```rust
// storage.rs:97-99
pub fn save_stream(env: &Env, stream: &Stream) {
    env.storage().persistent().set(&stream.id, stream);
}
// Key: u64 (8 bytes, big-endian)
// Value: Stream struct (~180 bytes serialized)
```

Each stream also has a persistent circular buffer of up to 10 `StreamTransition`
records. The head and length use `(Symbol, u64)` keys, and the ten slots use
`(Symbol, u64, u32)` keys with the `"st"` prefix. Transition history is retained
when the stream record is removed after completion so it remains available to
lightweight auditors.

The transition head and length keys use the `"st_head"` and `"st_len"` prefixes.

Stream context metadata is stored separately in temporary storage at
`("md", stream_id)`. Each write refreshes its TTL to 17,280 ledgers
(approximately 24 hours) and accepts at most 256 bytes.

The stream ID is derived deterministically via SHA-256 hash:

```rust
// storage.rs:42-66
pub fn derive_stream_id(env: &Env, sender: &Address, recipient: &Address, start_time: u64, nonce: u64) -> u64 {
    let mut buf = Bytes::new(env);
    buf.append(&sender.to_xdr(env));
    buf.append(&recipient.to_xdr(env));
    buf.append(&Bytes::from_array(env, &start_time.to_be_bytes()));
    buf.append(&Bytes::from_array(env, &nonce.to_be_bytes()));
    let hash = env.crypto().sha256(&buf);
    let hash_bytes = hash.to_array();
    u64::from_be_bytes([hash_bytes[0], .. hash_bytes[7]])
}
```

### Persistent storage — per-address indexes

The active sender index uses the same counter-plus-slot layout with separate
namespaces: `("asc", sender)` stores the count and `("as", sender, idx)` stores
active stream IDs. It is updated when streams are created, activated, approved,
paused, resumed, or removed, allowing active-stream queries and batch cancellation
without scanning terminal sender history.

Sender and recipient indexes use the **counter + slot** pattern. Each address has:

1. A **count key** `(Symbol, Address)` storing the number of stream IDs in the index.
2. **Slot keys** `(Symbol, Address, u32)` storing individual stream IDs at each position.

| Key pattern | Symbol prefix | Rust function | Purpose |
|-------------|--------------|---------------|---------|
| `(Symbol("sc"), addr)` | `"sc"` | `sender_count_key()` | Number of streams by sender |
| `(Symbol("s"), addr, idx)` | `"s"` | `sender_slot_key()` | Sender's idx-th stream ID |
| `(Symbol("rc"), addr)` | `"rc"` | `recipient_count_key()` | Number of streams for recipient |
| `(Symbol("r"), addr, idx)` | `"r"` | `recipient_slot_key()` | Recipient's idx-th stream ID |

**Annotated example — index by sender:**

```rust
// storage.rs:113-115
fn sender_count_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "sc"), addr.clone())
}
// Encoding: [Symbol("sc"), Address(sender)]
// Soroban serializes this as: tag(2-tuple) + Symbol_val("sc") + Address_val

// storage.rs:121-123
fn sender_slot_key(env: &Env, addr: &Address, idx: u32) -> (Symbol, Address, u32) {
    (Symbol::new(env, "s"), addr.clone(), idx)
}
// Encoding: [Symbol("s"), Address(sender), u32(idx)]
// Soroban serializes this as: tag(3-tuple) + Symbol_val("s") + Address_val + u32_val
```

**Annotated example — writing to the sender index:**

```rust
// storage.rs:134-140
pub fn index_by_sender(env: &Env, sender: &Address, stream_id: u64) {
    let cnt_key = sender_count_key(env, sender);       // ("sc", sender)
    let idx: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0u32);
    env.storage().persistent().set(
        &sender_slot_key(env, sender, idx),             // ("s", sender, idx)
        &stream_id,
    );
    let next = idx.checked_add(1).expect("sender index overflow");
    env.storage().persistent().set(&cnt_key, &next);    // increment count
}
// Three storage writes:
//   1. persistent: ("s", sender, 0) → stream_id_0
//   2. persistent: ("sc", sender) → 1
//   (then for the next stream:)
//   3. persistent: ("s", sender, 1) → stream_id_1
//   4. persistent: ("sc", sender) → 2
```

### Persistent storage — nonce guards

| Key pattern | Symbol prefix | Rust function | Purpose |
|-------------|--------------|---------------|---------|
| `(Symbol("n"), addr, nonce)` | `"n"` | (inline) | Prevents duplicate stream creation |

```rust
// storage.rs:240-243
pub fn nonce_used(env: &Env, sender: &Address, nonce: u64) -> bool {
    let key = (Symbol::new(env, "n"), sender.clone(), nonce);
    env.storage().persistent().has(&key)
}
// Encoding: [Symbol("n"), Address(sender), u64(nonce)]
// Value: bool (true) — presence of key is sufficient
```

### Persistent storage — global stream enumeration

| Key pattern | Symbol prefix | Rust function | Purpose |
|-------------|--------------|---------------|---------|
| `(Symbol("gi"), idx)` | `"gi"` | (inline) | Global stream ID at position idx |

```rust
// storage.rs:77-78
let slot_key = (Symbol::new(env, "gi"), idx);
env.storage().persistent().set(&slot_key, &stream_id);
// Encoding: [Symbol("gi"), u32(idx)]
// Value: u64 (stream ID)
```

### Persistent storage — additional per-address keys

| Key pattern | Symbol prefix | Rust function | Purpose |
|-------------|--------------|---------------|---------|
| `(Symbol("bn"), addr)` | `"bn"` | `get_batch_nonce()` | Batch nonce counter |
| `(Symbol("wl"), addr)` | `"wl"` | `whitelist_key()` | Whitelist membership |
| `(Symbol("fe"), addr)` | `"fe"` | `fee_exempt_key()` | Fee exemption status |
| `(Symbol("sl"), addr)` | `"sl"` | `sender_limit_key()` | Per-sender stream limit |
| `(Symbol("del"), stream_id)` | `"del"` | `delegate_key()` | Authorized delegate |

```rust
// storage.rs:230-231
pub fn get_batch_nonce(env: &Env, sender: &Address) -> u64 {
    let key = (Symbol::new(env, "bn"), sender.clone());
    env.storage().persistent().get(&key).unwrap_or(0u64)
}
// Encoding: [Symbol("bn"), Address(sender)]
```

### Instance storage — audit log slots

| Key pattern | Symbol prefix | Purpose |
|-------------|--------------|---------|
| `(Symbol("al"), idx)` | `"al"` | Audit entry at circular buffer position |

```rust
// storage.rs:533-535
fn audit_slot_key(env: &Env, idx: u32) -> (Symbol, u32) {
    (Symbol::new(env, "al"), idx)
}
// Encoding: [Symbol("al"), u32(idx)]
// Value: AuditEntry struct
```

---

## Complete Symbol prefix registry

All Symbol strings currently used as storage key discriminators:

| Symbol | Storage type | Key shape | Description |
|--------|-------------|-----------|-------------|
| `"admin"` | instance | bare Symbol | Contract admin |
| `"paused"` | instance | bare Symbol | Pause flag |
| `"fee_bps"` | instance | bare Symbol | Protocol fee |
| `"treasury"` | instance | bare Symbol | Treasury address |
| `"min_dur"` | instance | bare Symbol | Min stream duration |
| `"version"` | instance | bare Symbol | Contract version |
| `"max_str"` | instance | bare Symbol | Max streams/sender |
| `"str_cnt"` | instance | bare Symbol | Global stream count |
| `"pnd_fee"` | instance | bare Symbol | Pending fee proposal |
| `"wd_cd"` | instance | bare Symbol | Withdrawal cooldown |
| `"wl_en"` | instance | bare Symbol | Whitelist enabled |
| `"guardian"` | instance | bare Symbol | Guardian address |
| `"governance"` | instance | bare Symbol | Governance address |
| `"p_exp"` | instance | bare Symbol | Pause expiry |
| `"cf_xlm"` | instance | bare Symbol | Creation fee |
| `"xlm_tok"` | instance | bare Symbol | XLM token address |
| `"migrations"` | instance | bare Symbol | Applied migrations |
| `"re_lk"` | temporary | bare Symbol | Reentrancy lock |
| `"al_head"` | instance | bare Symbol | Audit buffer head |
| `"al_len"` | instance | bare Symbol | Audit buffer length |
| `"sc"` | persistent | `(Symbol, Address)` | Sender stream count |
| `"s"` | persistent | `(Symbol, Address, u32)` | Sender index slot |
| `"rc"` | persistent | `(Symbol, Address)` | Recipient stream count |
| `"r"` | persistent | `(Symbol, Address, u32)` | Recipient index slot |
| `"n"` | persistent | `(Symbol, Address, u64)` | Nonce guard |
| `"gi"` | persistent | `(Symbol, u32)` | Global index slot |
| `"bn"` | persistent | `(Symbol, Address)` | Batch nonce |
| `"wl"` | persistent | `(Symbol, Address)` | Whitelist membership |
| `"fe"` | persistent | `(Symbol, Address)` | Fee exemption |
| `"sl"` | persistent | `(Symbol, Address)` | Sender limit override |
| `"del"` | persistent | `(Symbol, u64)` | Stream delegate |
| `"al"` | instance | `(Symbol, u32)` | Audit log slot |

---

## Collision avoidance guide

When adding a new storage entry:

1. **Choose a unique Symbol prefix** — check the table above. Prefixes must be unique within each storage durability type (instance, persistent, temporary). A Symbol like `"wl"` in persistent storage does not collide with `"wl_en"` in instance storage because they use different durability types AND different string values.

2. **Match key shape to existing patterns** — if adding per-address data, use `(Symbol("xx"), Address)` for counts/flags and `(Symbol("xx"), Address, u32)` for indexed slots. Never reuse an existing Symbol string for a different purpose.

3. **Document the new key** — add it to the Symbol prefix table above before merging.

4. **Consider upgrade safety** — instance and persistent keys survive WASM upgrades. A new key that replaces an existing key's semantics will silently corrupt state.

### Example: adding a new "freeze" feature per sender

```rust
// CORRECT: unique prefix "fr", same shape as other per-address flags
fn freeze_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "fr"), addr.clone())
}

// WRONG: reusing "wl" would collide with whitelist data
fn freeze_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "wl"), addr.clone())  // COLLISION!
}
```

---

## Reading encoded keys in tests

To verify key encoding in a test, construct the key tuple directly and read from storage:

```rust
#[test]
fn test_read_encoded_key() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SoroStreamContract, ());
    let c = SoroStreamContractClient::new(&env, &contract_id);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin).address();
    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);
    c.set_min_duration(&sender, &0);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
    );

    // Read the stream record using a bare u64 key
    let stream: Stream = env.storage().persistent().get(&stream_id).unwrap();
    assert_eq!(stream.deposit, 100_000);

    // Read the sender count using (Symbol, Address) key
    let count_key = (Symbol::new(&env, "sc"), sender.clone());
    let count: u32 = env.storage().persistent().get(&count_key).unwrap();
    assert_eq!(count, 1);

    // Read the sender index slot using (Symbol, Address, u32) key
    let slot_key = (Symbol::new(&env, "s"), sender.clone(), 0u32);
    let id: u64 = env.storage().persistent().get(&slot_key).unwrap();
    assert_eq!(id, stream_id);

    // Read the nonce guard using (Symbol, Address, u64) key
    let nonce_key = (Symbol::new(&env, "n"), sender.clone(), 0u64);
    let used: bool = env.storage().persistent().get(&nonce_key).unwrap();
    assert!(used);

    // Read instance config
    let admin: Address = env.storage().instance().get(&Symbol::new(&env, "admin")).unwrap();
    assert_eq!(admin, sender); // (or the admin address)
}
```

---

## Related files

- `contracts/stream/src/storage.rs` — all storage helpers
- `contracts/stream/src/lib.rs` — contract implementation
- `docs/STORAGE.md` — storage model and durability guidance
- `docs/storage-layout.md` — storage footprint benchmarks
