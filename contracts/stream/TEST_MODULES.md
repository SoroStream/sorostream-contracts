# Stream test, benchmark, and fuzz modules

Every Rust source file in `src/` following a test, benchmark, or fuzz naming convention is listed below. “Wired” means `lib.rs` has a `#[cfg(test)] mod ...;` declaration, so Cargo compiles it in the crate's test build. The inventory is checked in both directions by `python3 scripts/check_stream_test_modules.py` in CI; update this table when adding, removing, or wiring a matching file.

This inventory records the current state. The dormant-file history prompted issues [#806](https://github.com/SoroStream/sorostream-contracts/issues/806) (property tests), [#807](https://github.com/SoroStream/sorostream-contracts/issues/807) (differential fuzzing and cost benchmark), and [#809](https://github.com/SoroStream/sorostream-contracts/issues/809) (the guard against regressions).

| Source file | `lib.rs` status |
| --- | --- |
| `acl_tests.rs` | Dormant |
| `batch_cancel_tests.rs` | Dormant |
| `bonus_tests.rs` | Dormant |
| `cargo_audit_tests.rs` | Wired |
| `cascade_tests.rs` | Dormant |
| `chaos.rs` | Wired |
| `claimable_boundary_tests.rs` | Dormant |
| `cost_bench.rs` | Wired |
| `decay_vesting_tests.rs` | Dormant |
| `differential_fuzz.rs` | Wired |
| `duplicate_id_tests.rs` | Wired |
| `entry_point_tests.rs` | Dormant |
| `event_field_tests.rs` | Dormant |
| `event_versioning_tests.rs` | Wired |
| `feature_tests.rs` | Dormant |
| `grace_period_tests.rs` | Dormant |
| `integration_tests.rs` | Wired |
| `issue_37_tests.rs` | Wired |
| `issue_39_tests.rs` | Wired |
| `issue_401_tests.rs` | Dormant |
| `issue_402_tests.rs` | Wired |
| `issue_505_tests.rs` | Wired |
| `issue_506_tests.rs` | Wired |
| `issue_507_tests.rs` | Wired |
| `issue_516_tests.rs` | Dormant |
| `issue_517_tests.rs` | Dormant |
| `issue_518_tests.rs` | Dormant |
| `issue_520_tests.rs` | Wired |
| `issue_521_tests.rs` | Wired |
| `issue_522_tests.rs` | Wired |
| `issue_523_tests.rs` | Wired |
| `issue_602_tests.rs` | Dormant |
| `issue_603_tests.rs` | Dormant |
| `issue_604_tests.rs` | Dormant |
| `issue_605_tests.rs` | Dormant |
| `issue_606_tests.rs` | Dormant |
| `issue_607_tests.rs` | Dormant |
| `issue_608_tests.rs` | Dormant |
| `issue_609_tests.rs` | Dormant |
| `issue_617_tests.rs` | Wired |
| `issue_620_tests.rs` | Wired |
| `issue_622_tests.rs` | Dormant |
| `issue_623_tests.rs` | Dormant |
| `issue_624_tests.rs` | Wired |
| `issue_625_tests.rs` | Dormant |
| `issue_627_tests.rs` | Wired |
| `issue_628_tests.rs` | Dormant |
| `issue_629_tests.rs` | Dormant |
| `issue_633_tests.rs` | Dormant |
| `issue_634_tests.rs` | Dormant |
| `issue_638_tests.rs` | Dormant |
| `issue_640_tests.rs` | Dormant |
| `issue_641_tests.rs` | Dormant |
| `issue_643_tests.rs` | Dormant |
| `issue_644_tests.rs` | Dormant |
| `issue_647_tests.rs` | Wired |
| `issue_656_chaos_tests.rs` | Wired |
| `issue_661_tests.rs` | Dormant |
| `issue_6_tests.rs` | Wired |
| `issue_763_tests.rs` | Wired |
| `issue_815_admin_uninitialized_tests.rs` | Wired |
| `lifecycle_integration_tests.rs` | Dormant |
| `oracle_rate_adjustment_tests.rs` | Dormant |
| `paginated_recipient_tests.rs` | Dormant |
| `proptest_tests.rs` | Wired |
| `protocol_economics_tests.rs` | Dormant |
| `rate_limit_tests.rs` | Wired |
| `renewal_limit_tests.rs` | Wired |
| `role_cost_bench.rs` | Wired |
| `role_holders_tests.rs` | Dormant |
| `sender_whitelist_tests.rs` | Dormant |
| `storage_bench.rs` | Dormant |
| `storage_layout_snapshot_tests.rs` | Dormant |
| `stream_analytics_tests.rs` | Dormant |
| `stream_manager_tests.rs` | Dormant |
| `stream_nft_tests.rs` | Dormant |
| `stream_priority_tests.rs` | Wired |
| `stream_tags_tests.rs` | Dormant |
| `subscription_tests.rs` | Dormant |
| `test.rs` | Dormant |
| `testnet_integration_tests.rs` | Wired |
| `tranche_oracle_tests.rs` | Dormant |
| `upgrade_migration_tests.rs` | Dormant |
| `upgrade_proposal_tests.rs` | Dormant |

## Known dormant file: `test.rs`

`test.rs` was briefly wired in while repairing the dormant-module history (see
#806/#807/#809 above), which surfaced that it does not actually compile: its
254 `create_stream` call sites predate the `CreateStreamParams` struct
refactor and pass flattened individual arguments the current 7-parameter
signature (`sender, recipient, token, amount, duration_seconds, auto_renew,
params`) no longer accepts. It also has several corrupted doc comments from
past merges (a wrapped comment line missing its `///` continuation prefix,
breaking two functions' syntax outright).

Unlike every other file in this table, restoring `test.rs` is not a one-line
`mod` declaration — it needs a full pass rewriting its call sites against the
current API. Until that happens it stays intentionally `Dormant`; don't
re-add `#[cfg(test)] mod test;` without doing that rewrite first.
