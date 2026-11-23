//! Storage-footprint benchmark for the factory's deployed-contracts registry.
//!
//! Measures `get_deployed_contracts` read/write ledger entries as the number of
//! registered child contracts grows, to establish a safe deployment-volume
//! ceiling against Soroban's resource limits (issue #770).
//!
//! Run with:
//! ```text
//! cargo test --package sorostream-factory -- storage_bench --nocapture
//! ```
//!
//! The `generate_factory_storage_baseline` test writes
//! `benches/factory_storage_baseline.json` with the current read/write entry
//! counts for each registry size.

use std::println;

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, BytesN, Env,
};

struct StorageBenchEnv {
    env: Env,
    contract_id: Address,
    admin: Address,
    wasm_hash: BytesN<32>,
}

fn setup() -> StorageBenchEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(StreamFactory, ());
    let admin = Address::generate(&env);
    let c = StreamFactoryClient::new(&env, &contract_id);
    c.initialize(&admin);

    // Upload a valid Soroban WASM fixture for child deployments.
    const CHILD_WASM: &[u8] = soroban_sdk::contractfile!(
        file = "test_fixtures/child_contract.wasm",
        sha256 = "fd41d2f77920ca07b723e05f732a82db4c2f6459eb2be6b40c4f225434569550"
    );
    let wasm_hash = env.deployer().upload_contract_wasm(CHILD_WASM);

    StorageBenchEnv {
        env,
        contract_id,
        admin,
        wasm_hash,
    }
}

fn c(b: &StorageBenchEnv) -> StreamFactoryClient<'_> {
    StreamFactoryClient::new(&b.env, &b.contract_id)
}

fn fake_salt(env: &Env, n: u32) -> BytesN<32> {
    let mut salt = [0u8; 32];
    salt[..4].copy_from_slice(&n.to_be_bytes());
    BytesN::from_array(env, &salt)
}

/// Deploy `n` child contracts through the factory and return the client.
fn deploy_n(b: &mut StorageBenchEnv, n: u32) {
    let cl = c(b);
    for i in 0..n {
        cl.deploy_stream_contract(
            &b.admin,
            &b.wasm_hash,
            &fake_salt(&b.env, i + 1),
            &None,
        );
    }
}

struct EntryCount {
    name: &'static str,
    registry_size: u32,
    read_entries: u32,
    write_entries: u32,
}

fn measure(env: &Env, name: &'static str, registry_size: u32) -> EntryCount {
    let res = env.cost_estimate().resources();
    println!(
        "[factory_storage_bench: {name} (registry_size={registry_size})]  read_entries={}, write_entries={}",
        res.read_entries, res.write_entries
    );
    EntryCount {
        name,
        registry_size,
        read_entries: res.read_entries,
        write_entries: res.write_entries,
    }
}

#[test]
fn generate_factory_storage_baseline() {
    let mut results: std::vec::Vec<EntryCount> = std::vec::Vec::new();

    let registry_sizes = [0u32, 10, 50, 100, 250, 500, 1000];

    for &target_size in &registry_sizes {
        let mut b = setup();

        // Build up to target_size (cumulative across iterations is not possible
        // since each setup is fresh; deploy target_size fresh contracts).
        deploy_n(&mut b, target_size);

        let cl = c(&b);
        cl.get_deployed_contracts(&0, &20);
        results.push(measure(
            &b.env,
            "get_deployed_contracts",
            target_size,
        ));
    }

    // Build JSON manually (no serde in no_std crate)
    let mut json = std::string::String::from("{\n");
    for (i, entry) in results.iter().enumerate() {
        json.push_str(&std::format!(
            "  \"{}_n{}\": {{ \"read_entries\": {}, \"write_entries\": {}, \"registry_size\": {} }}",
            entry.name, entry.registry_size, entry.read_entries, entry.write_entries, entry.registry_size
        ));
        if i < results.len() - 1 {
            json.push(',');
        }
        json.push('\n');
    }
    json.push('}');

    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/factory_storage_baseline.json");
    std::fs::write(&path, &json).expect("failed to write factory_storage_baseline.json");
    println!("\nWrote baseline to {}", path.display());
}

#[test]
fn check_factory_storage_baseline_regression() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/factory_storage_baseline.json");
    let baseline = std::fs::read_to_string(&path)
        .expect("benches/factory_storage_baseline.json not found — run generate_factory_storage_baseline first");

    let registry_sizes = [0u32, 10, 50, 100, 250, 500, 1000];
    let mut failures: std::vec::Vec<std::string::String> = std::vec::Vec::new();
    const MAX_REGRESSION: u32 = 10;

    for &target_size in &registry_sizes {
        let mut b = setup();
        deploy_n(&mut b, target_size);
        let cl = c(&b);
        cl.get_deployed_contracts(&0, &20);
        let res = b.env.cost_estimate().resources();

        let key = std::format!("get_deployed_contracts_n{}", target_size);
        if let Some(start) = baseline.find(&std::format!("\"{}\"", key)) {
            let rest = &baseline[start..];
            let read_base = parse_json_u32(rest, "read_entries");
            let write_base = parse_json_u32(rest, "write_entries");

            if res.read_entries > read_base + MAX_REGRESSION {
                failures.push(std::format!(
                    "n{}: read_entries {} exceeds baseline {} by more than {}",
                    target_size, res.read_entries, read_base, MAX_REGRESSION
                ));
            }
            if res.write_entries > write_base + MAX_REGRESSION {
                failures.push(std::format!(
                    "n{}: write_entries {} exceeds baseline {} by more than {}",
                    target_size, res.write_entries, write_base, MAX_REGRESSION
                ));
            }
        } else {
            println!("WARNING: no baseline found for {key}, skipping regression check");
        }
    }

    if !failures.is_empty() {
        panic!(
            "Factory storage baseline regression detected:\n{}",
            failures.join("\n")
        );
    }

    println!("\nAll registry sizes within 10-entry tolerance of baseline.");
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
