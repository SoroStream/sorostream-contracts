//! Resource cost comparison for `roles::has_any_role`.
//!
//! Run with:
//! ```text
//! cargo test --package sorostream-stream -- role_cost_bench -- --nocapture
//! ```

use std::println;

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[contract]
struct RoleCheckBench;

#[contractimpl]
impl RoleCheckBench {
    pub fn has_any_role(env: Env, caller: Address, admin: Address) -> bool {
        super::roles::has_any_role(&env, &caller, &admin)
    }

    pub fn single_role(env: Env, caller: Address) -> bool {
        super::roles::get_fee_manager(&env).as_ref() == Some(&caller)
    }
}

#[derive(Clone, Copy)]
struct Cost {
    read_entries: u32,
    read_bytes: u32,
    instructions: i64,
    mem_bytes: i64,
}

fn measure_role_check(single_role: bool) -> Cost {
    let env = Env::default();
    let contract_id = env.register(RoleCheckBench, ());
    let client = RoleCheckBenchClient::new(&env, &contract_id);
    let caller = Address::generate(&env);
    let admin = Address::generate(&env);

    // Store populated, non-matching roles so the all-role check reads every
    // role slot. The top-level invocation below reports its resources alone.
    env.as_contract(&contract_id, || {
        super::roles::set_fee_manager(&env, &Address::generate(&env));
        super::roles::set_emergency_pause_role(&env, &Address::generate(&env));
        super::roles::set_token_admin(&env, &Address::generate(&env));
        super::roles::set_analytics_role(&env, &Address::generate(&env));
    });

    let result = if single_role {
        client.single_role(&caller)
    } else {
        client.has_any_role(&caller, &admin)
    };
    assert!(!result);
    let resources = env.cost_estimate().resources();

    Cost {
        read_entries: resources.read_entries,
        read_bytes: resources.read_bytes,
        instructions: resources.instructions,
        mem_bytes: resources.mem_bytes,
    }
}

/// Compares the full role scan with a hypothetical single fee-manager lookup.
#[test]
fn bench_has_any_role_vs_single_role_check() {
    let all_roles = measure_role_check(false);
    let single_role = measure_role_check(true);

    println!(
        "\n[role_cost_bench: has_any_role vs single-role check]\n\
         measurement                 read_entries  read_bytes  instructions  mem_bytes\n\
         has_any_role                {:>12}  {:>10}  {:>12}  {:>9}\n\
         hypothetical single-role   {:>12}  {:>10}  {:>12}  {:>9}\n\
         potential savings           {:>12}  {:>10}  {:>12}  {:>9}",
        all_roles.read_entries,
        all_roles.read_bytes,
        all_roles.instructions,
        all_roles.mem_bytes,
        single_role.read_entries,
        single_role.read_bytes,
        single_role.instructions,
        single_role.mem_bytes,
        all_roles
            .read_entries
            .saturating_sub(single_role.read_entries),
        all_roles.read_bytes.saturating_sub(single_role.read_bytes),
        all_roles
            .instructions
            .saturating_sub(single_role.instructions),
        all_roles.mem_bytes.saturating_sub(single_role.mem_bytes),
    );
}
