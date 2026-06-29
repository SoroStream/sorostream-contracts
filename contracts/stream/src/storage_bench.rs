//! Storage footprint benchmarks — tracks read/write ledger entries per instruction.
//!
//! Run with:
//! ```text
//! cargo test --package sorostream-stream -- storage_bench --nocapture
//! ```
//!
//! The `generate_storage_baseline` test writes `benches/storage_baseline.json`
//! with the current read/write entry counts for each instruction. CI compares
//! against this file to catch regressions exceeding 10 entries.

use std::println;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct StorageBenchEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> StorageBenchEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    // Disable minimum duration for tests
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

    StorageBenchEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn c(b: &StorageBenchEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&b.env, &b.contract_id)
}

struct EntryCount {
    name: &'static str,
    read_entries: u32,
    write_entries: u32,
}

fn measure(env: &Env, name: &'static str) -> EntryCount {
    let res = env.cost_estimate().resources();
    println!(
        "[storage_bench: {name}]  read_entries={}, write_entries={}",
        res.read_entries, res.write_entries
    );
    EntryCount {
        name,
        read_entries: res.read_entries,
        write_entries: res.write_entries,
    }
}

#[test]
fn generate_storage_baseline() {
    let mut results: std::vec::Vec<EntryCount> = std::vec::Vec::new();

    // --- create_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        results.push(measure(&b.env, "create_stream"));
    }

    // --- withdraw ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(500);
        cl.withdraw(&stream_id, &b.recipient);
        results.push(measure(&b.env, "withdraw"));
    }

    // --- top_up ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        cl.top_up(&stream_id, &b.sender, &b.token_id, &50_000);
        results.push(measure(&b.env, "top_up"));
    }

    // --- cancel_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(300);
        cl.cancel_stream(&stream_id, &b.sender);
        results.push(measure(&b.env, "cancel_stream"));
    }

    // --- partial_cancel_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(200);
        cl.partial_cancel_stream(&stream_id, &b.sender, &30_000);
        results.push(measure(&b.env, "partial_cancel_stream"));
    }

    // --- batch_create_stream (N=5) ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);

        let mut recipients = soroban_sdk::Vec::new(&b.env);
        let mut amounts = soroban_sdk::Vec::new(&b.env);
        for _ in 0..5 {
            recipients.push_back(Address::generate(&b.env));
            amounts.push_back(10_000i128);
        }
        let lock_untils = soroban_sdk::vec![&b.env, 0u64, 0u64, 0u64, 0u64, 0u64];
        let mut tokens = soroban_sdk::Vec::new(&b.env);
        for _ in 0..recipients.len() { tokens.push_back(b.token_id.clone()); }
        cl.batch_create_stream(
            &b.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
        );
        results.push(measure(&b.env, "batch_create_stream_n5"));
    }

    // --- batch_withdraw (N=5) ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);

        let mut stream_ids = soroban_sdk::Vec::new(&b.env);
        for nonce in 0u64..5 {
            let id = cl.create_stream(
                &b.sender, &b.recipient, &b.token_id,
                &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            );
            stream_ids.push_back(id);
        }
        b.env.ledger().set_timestamp(500);
        cl.batch_withdraw(&stream_ids, &b.recipient);
        results.push(measure(&b.env, "batch_withdraw_n5"));
    }

    // --- get_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        std::println!("stream_id: {}", _stream_id); cl.get_stream(&_stream_id);
        results.push(measure(&b.env, "get_stream"));
    }

    // --- get_claimable ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(500);
        cl.get_claimable(&_stream_id);
        results.push(measure(&b.env, "get_claimable"));
    }

    // Build JSON manually (no serde in no_std crate)
    let mut json = std::string::String::from("{\n");
    for (i, entry) in results.iter().enumerate() {
        json.push_str(&std::format!(
            "  \"{}\": {{ \"read_entries\": {}, \"write_entries\": {} }}",
            entry.name, entry.read_entries, entry.write_entries
        ));
        if i < results.len() - 1 {
            json.push(',');
        }
        json.push('\n');
    }
    json.push('}');

    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/storage_baseline.json");
    std::fs::write(&path, &json).expect("failed to write storage_baseline.json");
    println!("\nWrote baseline to {}", path.display());
}

#[test]
fn check_storage_baseline_regression() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/storage_baseline.json");
    let baseline = std::fs::read_to_string(&path)
        .expect("benches/storage_baseline.json not found — run generate_storage_baseline first");

    let mut current: std::vec::Vec<(&str, EntryCount)> = std::vec::Vec::new();

    // --- create_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        current.push(("create_stream", measure(&b.env, "create_stream")));
    }

    // --- withdraw ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(500);
        cl.withdraw(&stream_id, &b.recipient);
        current.push(("withdraw", measure(&b.env, "withdraw")));
    }

    // --- top_up ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        cl.top_up(&stream_id, &b.sender, &b.token_id, &50_000);
        current.push(("top_up", measure(&b.env, "top_up")));
    }

    // --- cancel_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(300);
        cl.cancel_stream(&stream_id, &b.sender);
        current.push(("cancel_stream", measure(&b.env, "cancel_stream")));
    }

    // --- partial_cancel_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(200);
        cl.partial_cancel_stream(&stream_id, &b.sender, &30_000);
        current.push(("partial_cancel_stream", measure(&b.env, "partial_cancel_stream")));
    }

    // --- batch_create_stream_n5 ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let mut recipients = soroban_sdk::Vec::new(&b.env);
        let mut amounts = soroban_sdk::Vec::new(&b.env);
        for _ in 0..5 {
            recipients.push_back(Address::generate(&b.env));
            amounts.push_back(10_000i128);
        }
        let lock_untils = soroban_sdk::vec![&b.env, 0u64, 0u64, 0u64, 0u64, 0u64];
        let mut tokens = soroban_sdk::Vec::new(&b.env);
        for _ in 0..recipients.len() { tokens.push_back(b.token_id.clone()); }
        cl.batch_create_stream(
            &b.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
        );
        current.push(("batch_create_stream_n5", measure(&b.env, "batch_create_stream_n5")));
    }

    // --- batch_withdraw_n5 ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let mut stream_ids = soroban_sdk::Vec::new(&b.env);
        for nonce in 0u64..5 {
            let id = cl.create_stream(
                &b.sender, &b.recipient, &b.token_id,
                &10_000, &1000, &0, &nonce, &false, &0u64,
        &false,
            );
            stream_ids.push_back(id);
        }
        b.env.ledger().set_timestamp(500);
        cl.batch_withdraw(&stream_ids, &b.recipient);
        current.push(("batch_withdraw_n5", measure(&b.env, "batch_withdraw_n5")));
    }

    // --- get_stream ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        std::println!("stream_id: {}", _stream_id); cl.get_stream(&_stream_id);
        current.push(("get_stream", measure(&b.env, "get_stream")));
    }

    // --- get_claimable ---
    {
        let b = setup();
        let cl = c(&b);
        b.env.ledger().set_timestamp(0);
        let _stream_id = cl.create_stream(
            &b.sender, &b.recipient, &b.token_id,
            &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&b.env),
        );
        b.env.ledger().set_timestamp(500);
        cl.get_claimable(&_stream_id);
        current.push(("get_claimable", measure(&b.env, "get_claimable")));
    }

    const MAX_REGRESSION: u32 = 10;
    let mut failures: std::vec::Vec<std::string::String> = std::vec::Vec::new();

    for (name, counts) in &current {
        // Parse baseline value for this instruction from JSON
        if let Some(start) = baseline.find(&std::format!("\"{}\"", name)) {
            let rest = &baseline[start..];
            let read_base = parse_json_u32(rest, "read_entries");
            let write_base = parse_json_u32(rest, "write_entries");

            if counts.read_entries > read_base + MAX_REGRESSION {
                failures.push(std::format!(
                    "{}: read_entries {} exceeds baseline {} by more than {}",
                    name, counts.read_entries, read_base, MAX_REGRESSION
                ));
            }
            if counts.write_entries > write_base + MAX_REGRESSION {
                failures.push(std::format!(
                    "{}: write_entries {} exceeds baseline {} by more than {}",
                    name, counts.write_entries, write_base, MAX_REGRESSION
                ));
            }
        } else {
            println!("WARNING: no baseline found for {name}, skipping regression check");
        }
    }

    if !failures.is_empty() {
        panic!(
            "Storage baseline regression detected:\n{}",
            failures.join("\n")
        );
    }

    println!("\nAll instructions within 10-entry tolerance of baseline.");
}

fn parse_json_u32(s: &str, key: &str) -> u32 {
    let pattern = std::format!("\"{}\": ", key);
    if let Some(pos) = s.find(&pattern) {
        let rest = &s[pos + pattern.len()..];
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        rest[..end].parse().unwrap_or(0)
    } else {
        0
    }
}
