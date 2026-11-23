# Stream test and harness modules

The `#[cfg(test)]` declarations in `contracts/stream/src/lib.rs` are the authoritative
list of stream tests, benchmarks, and fuzz harnesses compiled by `cargo test`. Files
matching the test/bench/fuzz naming convention must have a declaration there, and
that declaration must point to an existing file. CI enforces both directions with
`scripts/check_stream_test_modules.py` (see [#805](https://github.com/SoroStream/sorostream-contracts/issues/805)
and [#809](https://github.com/SoroStream/sorostream-contracts/issues/809)); see also
[#810](https://github.com/SoroStream/sorostream-contracts/issues/810) for the inventory
requirement that prompted this list.

The current wired files are:
- `acl_tests.rs`
- `batch_cancel_tests.rs`
- `bonus_tests.rs`
- `cargo_audit_tests.rs`
- `cascade_tests.rs`
- `chaos.rs`
- `claimable_boundary_tests.rs`
- `cost_bench.rs`
- `decay_vesting_tests.rs`
- `differential_fuzz.rs`
- `duplicate_id_tests.rs`
- `entry_point_tests.rs`
- `event_field_tests.rs`
- `event_versioning_tests.rs`
- `feature_tests.rs`
- `grace_period_tests.rs`
- `integration_tests.rs`
- `issue_37_tests.rs`
- `issue_39_tests.rs`
- `issue_401_tests.rs`
- `issue_402_tests.rs`
- `issue_505_tests.rs`
- `issue_506_tests.rs`
- `issue_507_tests.rs`
- `issue_516_tests.rs`
- `issue_517_tests.rs`
- `issue_518_tests.rs`
- `issue_520_tests.rs`
- `issue_521_tests.rs`
- `issue_522_tests.rs`
- `issue_523_tests.rs`
- `issue_602_tests.rs`
- `issue_603_tests.rs`
- `issue_604_tests.rs`
- `issue_605_tests.rs`
- `issue_606_tests.rs`
- `issue_607_tests.rs`
- `issue_608_tests.rs`
- `issue_609_tests.rs`
- `issue_617_tests.rs`
- `issue_620_tests.rs`
- `issue_622_tests.rs`
- `issue_623_tests.rs`
- `issue_624_tests.rs`
- `issue_625_tests.rs`
- `issue_627_tests.rs`
- `issue_628_tests.rs`
- `issue_629_tests.rs`
- `issue_633_tests.rs`
- `issue_634_tests.rs`
- `issue_638_tests.rs`
- `issue_640_tests.rs`
- `issue_641_tests.rs`
- `issue_643_tests.rs`
- `issue_644_tests.rs`
- `issue_647_tests.rs`
- `issue_656_chaos_tests.rs`
- `issue_661_tests.rs`
- `issue_6_tests.rs`
- `issue_763_tests.rs`
- `issue_815_admin_uninitialized_tests.rs`
- `lifecycle_integration_tests.rs`
- `oracle_rate_adjustment_tests.rs`
- `paginated_recipient_tests.rs`
- `proptest_tests.rs`
- `protocol_economics_tests.rs`
- `rate_limit_tests.rs`
- `renewal_limit_tests.rs`
- `role_cost_bench.rs`
- `role_holders_tests.rs`
- `sender_whitelist_tests.rs`
- `storage_bench.rs`
- `storage_layout_snapshot_tests.rs`
- `stream_analytics_tests.rs`
- `stream_manager_tests.rs`
- `stream_nft_tests.rs`
- `stream_priority_tests.rs`
- `stream_tags_tests.rs`
- `subscription_tests.rs`
- `test.rs`
- `testnet_integration_tests.rs`
- `tranche_oracle_tests.rs`
- `upgrade_migration_tests.rs`
- `upgrade_proposal_tests.rs`

`proptest_tests.rs` contains the property checks originally associated with
[#97](https://github.com/SoroStream/sorostream-contracts/issues/97),
[#258](https://github.com/SoroStream/sorostream-contracts/issues/258),
[#578](https://github.com/SoroStream/sorostream-contracts/issues/578), and
[#647](https://github.com/SoroStream/sorostream-contracts/issues/647); these are
now part of the compiled suite after the omission was identified in
[#806](https://github.com/SoroStream/sorostream-contracts/issues/806).

`differential_fuzz.rs` and `cost_bench.rs` are the shared differential-fuzz and
resource-cost harnesses. New harness extensions such as the roles benchmark in
[#843](https://github.com/SoroStream/sorostream-contracts/issues/843) and new
vesting differential tests such as [#740](https://github.com/SoroStream/sorostream-contracts/issues/740)
should extend these wired modules or add a module declaration before relying on a
new test/benchmark target. The wiring gap was tracked in [#807](https://github.com/SoroStream/sorostream-contracts/issues/807).
