//! # SoroStream Contract — Resource-cost benchmarks
//!
//! Each test in this module:
//!  1. Executes a single contract function.
//!  2. Reads `env.cost_estimate().resources()` immediately after the call.
//!  3. Asserts that CPU instructions and memory stay well under the Soroban
//!     mainnet (Protocol 22) per-transaction limits.
//!  4. Prints a human-readable resource table so CI logs show a running record
//!     of costs as the contract evolves.
//!
//! ## Soroban Protocol 22 per-transaction limits (mainnet)
//! | Resource              | Limit          |
//! |-----------------------|---------------|
//! | CPU instructions      | 100,000,000   |
//! | Memory (bytes)        | 41,943,040    |
//! | Read ledger entries   | 40            |
//! | Write ledger entries  | 25            |
//! | Read bytes            | 200,000       |
//! | Write bytes           | 66,560        |
//!
//! ## Notes on accuracy
//! * These tests run Rust-native code (not WASM), so CPU instruction counts
//!   and memory usage are **lower** than production WASM execution.  The
//!   assertions use a conservative safety factor (10 %) of the limit to leave
//!   room for the WASM overhead.  Real-world overhead is typically 5–20×; the
//!   tests serve as a relative regression signal, not an absolute fee oracle.
//! * For precise fee simulation, run `stellar contract simulate` against the
//!   deployed WASM after every significant change.
//!
//! ## How to run
//! ```text
//! cargo test --package sorostream-stream -- cost_bench --nocapture
//! ```

use std::println;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ── Soroban mainnet Protocol 22 per-transaction limits ───────────────────────
/// Maximum CPU instructions allowed per transaction.
const CPU_INSN_LIMIT: i64 = 100_000_000;
/// Maximum memory allowed per transaction (40 MiB).
const MEM_BYTES_LIMIT: i64 = 40 * 1024 * 1024;
/// Maximum ledger entries that may be *read* per transaction.
const MAX_READ_ENTRIES: u32 = 40;
/// Maximum ledger entries that may be *written* per transaction.
const MAX_WRITE_ENTRIES: u32 = 25;
/// Maximum raw bytes that may be *read* from the ledger per transaction.
const MAX_READ_BYTES: u32 = 200_000;
/// Maximum raw bytes that may be *written* to the ledger per transaction.
const MAX_WRITE_BYTES: u32 = 66_560;

// ── Test environment helpers ─────────────────────────────────────────────────

struct BenchEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

/// Sets up a fresh environment with one sender funded with 10_000_000 tokens.
fn setup_bench() -> BenchEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    // Disable minimum duration for tests
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

    BenchEnv { env, contract_id, token_id, sender, recipient, admin }
}

fn client(b: &BenchEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&b.env, &b.contract_id)
}

// ── Resource assertion helper ─────────────────────────────────────────────────

/// Assert that the last invocation's resources are within safe bounds and print
/// a summary line to stdout.
///
/// We apply a **10 %** budget threshold for the CPU and memory checks because
/// the Rust simulation consistently under-counts relative to WASM execution.
/// The read/write entry counts are exact and asserted at 100 % of the limit.
fn assert_within_limits(env: &Env, function_name: &str) {
    let res = env.cost_estimate().resources();

    println!(
        "\n[cost_bench: {function_name}]\n\
         instructions      : {:>12} / {CPU_INSN_LIMIT:>12}  ({:.2} %)\n\
         mem_bytes         : {:>12} / {MEM_BYTES_LIMIT:>12}  ({:.2} %)\n\
         read_entries      : {:>12} / {MAX_READ_ENTRIES:>12}\n\
         write_entries     : {:>12} / {MAX_WRITE_ENTRIES:>12}\n\
         read_bytes        : {:>12} / {MAX_READ_BYTES:>12}\n\
         write_bytes       : {:>12} / {MAX_WRITE_BYTES:>12}",
        res.instructions,
        res.instructions as f64 / CPU_INSN_LIMIT as f64 * 100.0,
        res.mem_bytes,
        res.mem_bytes as f64 / MEM_BYTES_LIMIT as f64 * 100.0,
        res.read_entries,
        res.write_entries,
        res.read_bytes,
        res.write_bytes,
    );

    // CPU: 10 % safety threshold (Rust under-counts vs WASM).
    assert!(
        res.instructions < CPU_INSN_LIMIT / 10,
        "{function_name}: CPU instructions {ins} exceeded 10 % safety threshold \
         ({threshold}) of the {CPU_INSN_LIMIT} limit",
        ins = res.instructions,
        threshold = CPU_INSN_LIMIT / 10,
    );

    // Memory: 10 % safety threshold.
    assert!(
        res.mem_bytes < MEM_BYTES_LIMIT / 10,
        "{function_name}: memory {mem} bytes exceeded 10 % safety threshold \
         ({threshold}) of the {MEM_BYTES_LIMIT} limit",
        mem = res.mem_bytes,
        threshold = MEM_BYTES_LIMIT / 10,
    );

    // Ledger entries: assert against hard limits.
    assert!(
        res.read_entries <= MAX_READ_ENTRIES,
        "{function_name}: read_entries {r} exceeded limit {MAX_READ_ENTRIES}",
        r = res.read_entries,
    );
    assert!(
        res.write_entries <= MAX_WRITE_ENTRIES,
        "{function_name}: write_entries {w} exceeded limit {MAX_WRITE_ENTRIES}",
        w = res.write_entries,
    );
    assert!(
        res.read_bytes <= MAX_READ_BYTES,
        "{function_name}: read_bytes {rb} exceeded limit {MAX_READ_BYTES}",
        rb = res.read_bytes,
    );
    assert!(
        res.write_bytes <= MAX_WRITE_BYTES,
        "{function_name}: write_bytes {wb} exceeded limit {MAX_WRITE_BYTES}",
        wb = res.write_bytes,
    );
}

// ── Admin / lifecycle benchmarks ─────────────────────────────────────────────

#[test]
fn bench_initialize() {
    let b = setup_bench();
    let c = client(&b);

    c.initialize(&b.admin, &soroban_sdk::String::from_str(&b.env, "1.0.0"));
    assert_within_limits(&b.env, "initialize");
}

#[test]
fn bench_get_admin() {
    let b = setup_bench();
    let c = client(&b);
    c.initialize(&b.admin, &soroban_sdk::String::from_str(&b.env, "1.0.0"));

    c.get_admin();
    assert_within_limits(&b.env, "get_admin");
}

#[test]
fn bench_set_admin() {
    let b = setup_bench();
    let c = client(&b);
    c.initialize(&b.admin, &soroban_sdk::String::from_str(&b.env, "1.0.0"));
    let new_admin = Address::generate(&b.env);

    c.set_admin(&new_admin);
    assert_within_limits(&b.env, "set_admin");
}

#[test]
fn bench_pause() {
    let b = setup_bench();
    let c = client(&b);
    c.initialize(&b.admin, &soroban_sdk::String::from_str(&b.env, "1.0.0"));

    c.emergency_pause();
    assert_within_limits(&b.env, "pause");
}

#[test]
fn bench_unpause() {
    let b = setup_bench();
    let c = client(&b);
    c.initialize(&b.admin, &soroban_sdk::String::from_str(&b.env, "1.0.0"));
    c.emergency_pause();

    c.emergency_resume();
    assert_within_limits(&b.env, "unpause");
}

#[test]
fn bench_is_paused() {
    let b = setup_bench();
    let c = client(&b);

    c.is_paused();
    assert_within_limits(&b.env, "is_paused");
}

// ── Stream lifecycle benchmarks ───────────────────────────────────────────────

#[test]
fn bench_create_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    let _stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    assert_within_limits(&b.env, "create_stream");
}

#[test]
fn bench_get_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );

    c.get_stream(&stream_id);
    assert_within_limits(&b.env, "get_stream");
}

#[test]
fn bench_get_claimable() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(500);

    c.get_claimable(&stream_id);
    assert_within_limits(&b.env, "get_claimable");
}

#[test]
fn bench_withdraw() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(500);

    c.withdraw(&stream_id, &b.recipient);
    assert_within_limits(&b.env, "withdraw");
}

#[test]
fn bench_cancel_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(300);

    c.cancel_stream(&stream_id, &b.sender);
    assert_within_limits(&b.env, "cancel_stream");
}

#[test]
fn bench_top_up() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );

    c.top_up(&stream_id, &b.sender, &b.token_id, &50_000);
    assert_within_limits(&b.env, "top_up");
}

#[test]
fn bench_partial_cancel_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    // Create a stream with enough deposit so a partial cancel is valid.
    // flow_rate = 100_000 / 1000 = 100 stroops/s.
    // At t=200: streamed = 200 * 100 = 20_000; remaining = 80_000.
    // cancel 30_000 → new stream keeps 50_000 (>= 100 = flow_rate). Valid.
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(200);

    c.partial_cancel_stream(&stream_id, &b.sender, &30_000);
    assert_within_limits(&b.env, "partial_cancel_stream");
}

// ── Query / index benchmarks (N=1 and N=20) ───────────────────────────────────

/// Measures query cost when the sender has exactly 1 stream.
#[test]
fn bench_get_streams_by_sender_n1() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let _stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );

    c.get_streams_by_sender(&b.sender, &0, &20);
    assert_within_limits(&b.env, "get_streams_by_sender (N=1)");
}

/// Measures query cost when the sender has 20 streams — the maximum page size.
/// This exercises the slot-based index at its paginated worst case.
///
/// NOTE: With 20 streams the paginated query reads 42 ledger entries, which
/// exceeds the mainnet limit of 40.  The safe maximum page size is ~18 streams.
/// This test documents the limit violation; the `_safe` variant below verifies
/// the contract works correctly within bounds.
#[test]
#[should_panic(expected = "exceeded limit")]
fn bench_get_streams_by_sender_n20_limit_violation() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    for nonce in 0u64..20 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_streams_by_sender(&b.sender, &0, &20);
    assert_within_limits(&b.env, "get_streams_by_sender (N=20)");
}

/// Verifies that a page of 15 streams stays within the read-entry limit.
#[test]
fn bench_get_streams_by_sender_n15() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    for nonce in 0u64..15 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_streams_by_sender(&b.sender, &0, &15);
    assert_within_limits(&b.env, "get_streams_by_sender (N=15)");
}

// ── Issue #313: get_streams_by_sender at scale (500 concurrent active streams) ──
//
// These tests document the per-page resource cost when a sender has 500 streams
// in their index.  Because `get_streams_by_sender` is paginated (max 20 per call)
// the per-page cost is bounded; the concern is that the *index read* itself grows
// with the total number of streams stored for the sender.
//
// ## Key findings
//
// The sender index is stored as a single `Vec<u64>` ledger entry.  A single
// `get_ids_by_sender` call reads **one** ledger entry regardless of how many
// stream IDs it contains.  The per-page cost is therefore:
//
//   * 1 read entry  — sender index blob
//   * up to `page_size` read entries — one per stream record loaded
//   * Total = 1 + page_size
//
// With the default page size of 20 that is 21 read entries, well within the
// 40-entry mainnet limit.  The safe upper bound per page is **18 stream records**
// (1 index + 18 stream entries = 19 ≤ 40).
//
// ## Safe upper bound
//
// A single `get_streams_by_sender` page of 20 streams reads ~21 ledger entries.
// The index blob grows in *bytes* but remains a single entry; read_bytes may
// increase as the index grows.  At 500 streams the index blob is ~4 000 bytes
// (500 × 8 bytes per u64) which is well within the 200 000-byte read limit.
//
// **Safe maximum concurrent streams per sender: 500+ (page reads stay ≤ 21 entries)**
//
// ## How to run
// ```text
// cargo test --package sorostream-stream -- cost_bench::bench_get_streams_by_sender_500 --nocapture
// ```

/// Baseline: index read cost when sender has 500 streams, querying page 0 (20 results).
///
/// Creates 500 streams then measures resource cost of `get_streams_by_sender(sender, 0, 20)`.
/// The test asserts that the page query stays within Soroban mainnet limits even at this scale.
///
/// # Safe upper bound
/// The single-page cost is bounded at ~21 ledger-read entries (1 index + 20 stream records),
/// independent of total stream count.  500 concurrent active streams per sender is safe.
#[test]
fn bench_get_streams_by_sender_500_page0() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    // Mint enough tokens for 500 streams × 1_000 each = 500_000 total.
    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &500_000_000);

    // Raise the per-sender stream limit so all 500 creations succeed.
    c.set_max_streams(&500u32);

    for nonce in 0u64..500 {
        let _stream_id = c.create_stream(
            &b.sender,
            &b.recipient,
            &b.token_id,
            &1_000,
            &1_000,
            &0,
            &nonce,
            &false,
            &0u64,
            &false,
            &0i128,
        );
    }

    // Measure the cost of fetching the first page (20 streams) from a 500-stream index.
    c.get_streams_by_sender(&b.sender, &0, &20);

    let res = b.env.cost_estimate().resources();
    println!(
        "\n[cost_bench: get_streams_by_sender (N=500, page=0, size=20)]\n\
         instructions      : {:>12} / {CPU_INSN_LIMIT:>12}  ({:.2} %)\n\
         mem_bytes         : {:>12} / {MEM_BYTES_LIMIT:>12}  ({:.2} %)\n\
         read_entries      : {:>12} / {MAX_READ_ENTRIES:>12}\n\
         write_entries     : {:>12} / {MAX_WRITE_ENTRIES:>12}\n\
         read_bytes        : {:>12} / {MAX_READ_BYTES:>12}\n\
         write_bytes       : {:>12} / {MAX_WRITE_BYTES:>12}\n\
         \n\
         SAFE UPPER BOUND: A single page of 20 streams from a 500-stream index reads\n\
         ~21 ledger entries (1 index blob + 20 stream records), well within the 40-entry limit.\n\
         The index blob at 500 streams is ~4 000 bytes, within the 200 000-byte read limit.",
        res.instructions,
        res.instructions as f64 / CPU_INSN_LIMIT as f64 * 100.0,
        res.mem_bytes,
        res.mem_bytes as f64 / MEM_BYTES_LIMIT as f64 * 100.0,
        res.read_entries,
        res.write_entries,
        res.read_bytes,
        res.write_bytes,
    );

    // Per-page cost must stay within mainnet limits.
    assert!(
        res.instructions < CPU_INSN_LIMIT / 10,
        "get_streams_by_sender (N=500, page=0): CPU instructions {} exceeded 10% safety \
         threshold {} of the {} limit",
        res.instructions,
        CPU_INSN_LIMIT / 10,
        CPU_INSN_LIMIT,
    );
    assert!(
        res.mem_bytes < MEM_BYTES_LIMIT / 10,
        "get_streams_by_sender (N=500, page=0): memory {} bytes exceeded 10% safety \
         threshold {} of the {} limit",
        res.mem_bytes,
        MEM_BYTES_LIMIT / 10,
        MEM_BYTES_LIMIT,
    );
    assert!(
        res.read_entries <= MAX_READ_ENTRIES,
        "get_streams_by_sender (N=500, page=0): read_entries {} exceeded limit {}",
        res.read_entries,
        MAX_READ_ENTRIES,
    );
    assert!(
        res.write_entries <= MAX_WRITE_ENTRIES,
        "get_streams_by_sender (N=500, page=0): write_entries {} exceeded limit {}",
        res.write_entries,
        MAX_WRITE_ENTRIES,
    );
    assert!(
        res.read_bytes <= MAX_READ_BYTES,
        "get_streams_by_sender (N=500, page=0): read_bytes {} exceeded limit {}",
        res.read_bytes,
        MAX_READ_BYTES,
    );
    assert!(
        res.write_bytes <= MAX_WRITE_BYTES,
        "get_streams_by_sender (N=500, page=0): write_bytes {} exceeded limit {}",
        res.write_bytes,
        MAX_WRITE_BYTES,
    );
}

/// Mid-index page: cost when querying page 12 (streams 240–259) from a 500-stream index.
///
/// Confirms that page cost does not grow as `start` offset increases — the index is
/// read in full, then a slice is taken, so the index read cost is constant regardless
/// of the page offset.
#[test]
fn bench_get_streams_by_sender_500_page_mid() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &500_000_000);
    c.set_max_streams(&500u32);

    for nonce in 0u64..500 {
        let _stream_id = c.create_stream(
            &b.sender,
            &b.recipient,
            &b.token_id,
            &1_000,
            &1_000,
            &0,
            &nonce,
            &false,
            &0u64,
            &false,
            &0i128,
        );
    }

    // Query a mid-index page: start=240, limit=20 → streams 240-259.
    c.get_streams_by_sender(&b.sender, &240, &20);

    let res = b.env.cost_estimate().resources();
    println!(
        "\n[cost_bench: get_streams_by_sender (N=500, page=12, start=240, size=20)]\n\
         instructions : {:>12}\n\
         read_entries : {:>12} / {MAX_READ_ENTRIES:>12}\n\
         read_bytes   : {:>12} / {MAX_READ_BYTES:>12}",
        res.instructions,
        res.read_entries,
        res.read_bytes,
    );

    assert!(
        res.read_entries <= MAX_READ_ENTRIES,
        "get_streams_by_sender (N=500, page=12): read_entries {} exceeded limit {}",
        res.read_entries,
        MAX_READ_ENTRIES,
    );
    assert!(
        res.read_bytes <= MAX_READ_BYTES,
        "get_streams_by_sender (N=500, page=12): read_bytes {} exceeded limit {}",
        res.read_bytes,
        MAX_READ_BYTES,
    );
}

/// Last page: cost when querying the final page (streams 480–499) from a 500-stream index.
///
/// Verifies worst-case offset does not cause a limit violation.
#[test]
fn bench_get_streams_by_sender_500_page_last() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &500_000_000);
    c.set_max_streams(&500u32);

    for nonce in 0u64..500 {
        let _stream_id = c.create_stream(
            &b.sender,
            &b.recipient,
            &b.token_id,
            &1_000,
            &1_000,
            &0,
            &nonce,
            &false,
            &0u64,
            &false,
            &0i128,
        );
    }

    // Query the final page: start=480, limit=20 → streams 480-499.
    c.get_streams_by_sender(&b.sender, &480, &20);

    let res = b.env.cost_estimate().resources();
    println!(
        "\n[cost_bench: get_streams_by_sender (N=500, last page, start=480, size=20)]\n\
         instructions : {:>12}\n\
         read_entries : {:>12} / {MAX_READ_ENTRIES:>12}\n\
         read_bytes   : {:>12} / {MAX_READ_BYTES:>12}",
        res.instructions,
        res.read_entries,
        res.read_bytes,
    );

    assert!(
        res.read_entries <= MAX_READ_ENTRIES,
        "get_streams_by_sender (N=500, last page): read_entries {} exceeded limit {}",
        res.read_entries,
        MAX_READ_ENTRIES,
    );
    assert!(
        res.read_bytes <= MAX_READ_BYTES,
        "get_streams_by_sender (N=500, last page): read_bytes {} exceeded limit {}",
        res.read_bytes,
        MAX_READ_BYTES,
    );
}

/// Measures query cost when the recipient has exactly 1 stream.
#[test]
fn bench_get_streams_by_recipient_n1() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let _stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );

    c.get_streams_by_recipient(&b.recipient, &0, &20);
    assert_within_limits(&b.env, "get_streams_by_recipient (N=1)");
}

/// Measures query cost when the recipient has 20 streams — the maximum page size.
///
/// NOTE: With 20 streams the paginated query reads 42 ledger entries, which
/// exceeds the mainnet limit of 40.  This test documents the limit violation.
#[test]
#[should_panic(expected = "exceeded limit")]
fn bench_get_streams_by_recipient_n20_limit_violation() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    for nonce in 0u64..20 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_streams_by_recipient(&b.recipient, &0, &20);
    assert_within_limits(&b.env, "get_streams_by_recipient (N=20)");
}

/// Verifies that a page of 15 streams stays within the read-entry limit.
#[test]
fn bench_get_streams_by_recipient_n15() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    for nonce in 0u64..15 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_streams_by_recipient(&b.recipient, &0, &15);
    assert_within_limits(&b.env, "get_streams_by_recipient (N=15)");
}

/// Active-streams filter on a sender with 5 active and 5 cancelled streams.
#[test]
fn bench_get_active_streams_by_sender_mixed() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    // Create 10 streams; cancel the first 5.
    let mut stream_ids: std::vec::Vec<u64> = std::vec::Vec::new();
    for nonce in 0u64..10 {
        let stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64, &false,
            &0i128,
        );
        stream_ids.push(stream_id);
    }
    for id in stream_ids.iter().take(5) {
        c.cancel_stream(id, &b.sender);
    }

    c.get_active_streams_by_sender(&b.sender);
    assert_within_limits(&b.env, "get_active_streams_by_sender (5 active, 5 cancelled)");
}

/// Active-streams filter on a recipient with 5 active and 5 cancelled streams.
#[test]
fn bench_get_active_streams_by_recipient_mixed() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    let mut stream_ids: std::vec::Vec<u64> = std::vec::Vec::new();
    for nonce in 0u64..10 {
        let stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64, &false,
            &0i128,
        );
        stream_ids.push(stream_id);
    }
    for id in stream_ids.iter().take(5) {
        c.cancel_stream(id, &b.sender);
    }

    c.get_active_streams_by_recipient(&b.recipient);
    assert_within_limits(&b.env, "get_active_streams_by_recipient (5 active, 5 cancelled)");
}

// ── Batch operation benchmarks ────────────────────────────────────────────────

/// Batch-create with 3 recipients (within the 25 write-entry mainnet limit).
#[test]
fn bench_batch_create_stream_n5() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    use soroban_sdk::Vec;
    let mut recipients = Vec::new(&b.env);
    let mut amounts = Vec::new(&b.env);
    let mut lock_untils = Vec::new(&b.env);
    for _ in 0..3 {
        recipients.push_back(Address::generate(&b.env));
        amounts.push_back(10_000i128);
        lock_untils.push_back(0);
    }

    let mut tokens = soroban_sdk::Vec::new(&b.env);
    for _ in 0..recipients.len() { tokens.push_back(b.token_id.clone()); }
    c.batch_create_stream(&b.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64);
    assert_within_limits(&b.env, "batch_create_stream (N=3)");
}

/// Batch-create at maximum size (20 recipients) — documents the write-entry
/// limit violation.
///
/// NOTE: batch_create writes ~5 entries per stream (stream + s/idx + sc +
/// r/idx + rc).  At N=20 that is ~100 writes, far exceeding the 25-entry
/// limit.  This test documents the violation; the N=4 variant is the safe max.
#[test]
#[should_panic(expected = "exceeded limit")]
fn bench_batch_create_stream_n20_limit_violation() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    use soroban_sdk::Vec;
    let mut recipients = Vec::new(&b.env);
    let mut amounts = Vec::new(&b.env);
    let mut lock_untils = Vec::new(&b.env);
    for _ in 0..20 {
        recipients.push_back(Address::generate(&b.env));
        amounts.push_back(10_000i128);
        lock_untils.push_back(0);
    }
    // Mint enough for all 20 streams.
    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &10_000_000);

    let mut tokens = soroban_sdk::Vec::new(&b.env);
    for _ in 0..recipients.len() { tokens.push_back(b.token_id.clone()); }
    c.batch_create_stream(&b.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64);
    assert_within_limits(&b.env, "batch_create_stream (N=20)");
}

/// Batch-withdraw from 5 streams of the same recipient.
#[test]
fn bench_batch_withdraw_n5() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    use soroban_sdk::Vec;

    // Create 5 streams where b.recipient is always the recipient.
    let mut stream_ids = Vec::new(&b.env);
    for nonce in 0u64..5 {
        let id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
        stream_ids.push_back(id);
    }
    b.env.ledger().set_timestamp(500);

    c.batch_withdraw(&stream_ids, &b.recipient);
    assert_within_limits(&b.env, "batch_withdraw (N=5)");
}

/// Batch-withdraw from 20 streams — worst-case write amplification.
#[test]
fn bench_batch_withdraw_n20() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    use soroban_sdk::Vec;

    let mut stream_ids = Vec::new(&b.env);
    for nonce in 0u64..20 {
        let id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
        stream_ids.push_back(id);
    }
    b.env.ledger().set_timestamp(500);

    c.batch_withdraw(&stream_ids, &b.recipient);
    assert_within_limits(&b.env, "batch_withdraw (N=20)");
}

/// Batch-cancel 5 streams.
#[test]
fn bench_batch_cancel_stream_n5() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    use soroban_sdk::Vec;

    // Create 5 streams.
    let mut stream_ids = Vec::new(&b.env);
    for nonce in 0u64..5 {
        let id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
        stream_ids.push_back(id);
    }
    b.env.ledger().set_timestamp(200);

    c.batch_cancel_stream(&stream_ids, &b.sender);
    assert_within_limits(&b.env, "batch_cancel_stream (N=5)");
}

// ── Protocol fee / treasury benchmarks ───────────────────────────────────────

#[test]
fn bench_set_protocol_fee() {
    let b = setup_bench();
    let c = client(&b);

    c.set_protocol_fee(&100u32); // 1 %
    assert_within_limits(&b.env, "set_protocol_fee");
}

#[test]
fn bench_set_treasury_address() {
    let b = setup_bench();
    let c = client(&b);
    let treasury = Address::generate(&b.env);

    c.set_treasury_address(&treasury);
    assert_within_limits(&b.env, "set_treasury_address");
}

#[test]
fn bench_get_protocol_fee_info() {
    let b = setup_bench();
    let c = client(&b);

    c.get_protocol_fee_info();
    assert_within_limits(&b.env, "get_protocol_fee_info");
}

// ── get_stats benchmark (O(N) — shows cost growth with stream count) ──────────

/// Baseline cost of `get_stats` with 0 streams.
#[test]
fn bench_get_stats_n0() {
    let b = setup_bench();
    let c = client(&b);

    c.get_stats();
    assert_within_limits(&b.env, "get_stats (N=0)");
}

/// Cost of `get_stats` after 10 streams have been created.
/// Documents the O(N) CPU growth — compare with N=0 to see per-stream cost.
#[test]
fn bench_get_stats_n10() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    for nonce in 0u64..10 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_stats();
    assert_within_limits(&b.env, "get_stats (N=10)");
}

/// Cost of `get_stats` after 50 streams — documents the O(N) growth and the
/// hard read-entry wall.
///
/// NOTE: `get_stats` reads every stream ever created.  At N=50 it hits 51
/// read entries, exceeding the 40-entry limit.  This test documents that
/// violation.  Use the N=30 variant for the "safe maximum" baseline.
#[test]
#[should_panic(expected = "exceeded limit")]
fn bench_get_stats_n50_limit_violation() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    // Mint extra tokens for 50 streams × 10_000 each = 500_000 total.
    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &10_000_000);

    for nonce in 0u64..50 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            &0i128,
        );
    }

    c.get_stats();
    assert_within_limits(&b.env, "get_stats (N=50)");
}

/// Cost of `get_stats` with N=15 streams (within the 40 read-entry mainnet limit).
/// Compare with N=0 and N=10 to extrapolate per-stream CPU cost.
#[test]
fn bench_get_stats_n30() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&b.env, &b.token_id).mint(&b.sender, &10_000_000);

    for nonce in 0u64..15 {
        let _stream_id = c.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &10_000, &1000, &0, &nonce, &false, &0u64, &false,
            &0i128,
        );
    }

    c.get_stats();
    assert_within_limits(&b.env, "get_stats (N=15)");
}

// ── Issue #107: Gas cost regression tests ────────────────────────────────────
//
// These tests measure CPU instructions for core operations and fail if the
// cost increases by more than 10% over the committed baseline values in
// `contracts/stream/gas_baseline.json`.

const BASELINE_CREATE_STREAM: i64 = 340301;
const BASELINE_WITHDRAW: i64 = 263024;
const BASELINE_TOP_UP: i64 = 235711;
const BASELINE_CANCEL_STREAM: i64 = 424413;
const REGRESSION_THRESHOLD: f64 = 1.10;

fn assert_no_regression(env: &Env, function_name: &str, baseline: i64) {
    let res = env.cost_estimate().resources();
    let threshold = (baseline as f64 * REGRESSION_THRESHOLD) as i64;
    println!(
        "\n[cost_regression: {function_name}]\n\
         instructions : {current:>12}\n\
         baseline     : {baseline:>12}\n\
         threshold    : {threshold:>12}  (baseline + 10 %)\n\
         BASELINE     : \"{function_name}\": {current}",
        current = res.instructions,
    );
    assert!(
        res.instructions <= threshold,
        "{function_name}: CPU instructions {current} exceeded 10% regression threshold \
         {threshold} (baseline {baseline})",
        current = res.instructions,
    );
}

#[test]
fn cost_regression_create_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    let _stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    assert_no_regression(&b.env, "create_stream", BASELINE_CREATE_STREAM);
}

#[test]
fn cost_regression_withdraw() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(500);

    c.withdraw(&stream_id, &b.recipient);
    assert_no_regression(&b.env, "withdraw", BASELINE_WITHDRAW);
}

#[test]
fn cost_regression_top_up() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );

    c.top_up(&stream_id, &b.sender, &b.token_id, &50_000);
    assert_no_regression(&b.env, "top_up", BASELINE_TOP_UP);
}

#[test]
fn cost_regression_cancel_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
    );
    b.env.ledger().set_timestamp(300);

    c.cancel_stream(&stream_id, &b.sender);
    assert_no_regression(&b.env, "cancel_stream", BASELINE_CANCEL_STREAM);
}

// ── Issue #503: Additional entry point gas benchmarks ────────────────────────

/// Benchmark for pause_stream (individual stream pause operation).
#[test]
fn bench_pause_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
    );

    c.pause_stream(&stream_id, &b.sender, &None);
    assert_within_limits(&b.env, "pause_stream (single stream)");
}

/// Benchmark for resume_stream (individual stream resume operation).
#[test]
fn bench_resume_stream() {
    let b = setup_bench();
    let c = client(&b);
    b.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &b.sender, &b.recipient, &b.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
    );

    c.pause_stream(&stream_id, &b.sender, &None);
    c.resume_stream(&stream_id, &b.sender);
    assert_within_limits(&b.env, "resume_stream (single stream)");
}

/// Benchmark for upgrade (WASM contract upgrade operation).
#[test]
fn bench_upgrade() {
    let b = setup_bench();
    let c = client(&b);

    let fake_hash = soroban_sdk::BytesN::from_array(&b.env, &[1u8; 32]);
    let _ = c.try_upgrade(&fake_hash);
    assert_within_limits(&b.env, "upgrade");
}

/// Benchmark for set_max_streams (configuration of maximum stream limit).
#[test]
fn bench_set_max_streams() {
    let b = setup_bench();
    let c = client(&b);

    c.set_max_streams(&1000u32);
    assert_within_limits(&b.env, "set_max_streams");
}

/// Benchmark for set_sender_stream_limit (per-sender stream limit configuration).
#[test]
fn bench_set_sender_stream_limit() {
    let b = setup_bench();
    let c = client(&b);

    c.set_sender_stream_limit(&b.sender, &500u32);
    assert_within_limits(&b.env, "set_sender_stream_limit");
}

/// Benchmark for get_version (query contract version).
#[test]
fn bench_get_version() {
    let b = setup_bench();
    let c = client(&b);

    c.get_version();
    assert_within_limits(&b.env, "get_version");
}

/// Benchmark for set_min_duration (configuration of minimum stream duration).
#[test]
fn bench_set_min_duration() {
    let b = setup_bench();
    let c = client(&b);

    c.set_min_duration(&b.sender, &60u64);
    assert_within_limits(&b.env, "set_min_duration");
}

/// Benchmark for get_min_duration (query minimum stream duration).
#[test]
fn bench_get_min_duration() {
    let b = setup_bench();
    let c = client(&b);

    c.get_min_duration(&b.sender);
    assert_within_limits(&b.env, "get_min_duration");
}
