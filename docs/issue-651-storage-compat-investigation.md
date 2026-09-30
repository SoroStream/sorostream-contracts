# Issue #651: Storage Compatibility Investigation

Status: blocked pending confirmation of the legacy formats the contract promises to support.

The current persisted `Stream` is a Soroban contract type stored under the stream ID. Its schema includes a `sponsor` field and groups runtime state in a nested `options` field. An earlier layout (for example, the flat `Stream` in revision `44dbc11`) stored many of those values as top-level fields. The repository does not identify which historical layouts are supported or define a version-by-version migration contract.

The existing `upgrade_migration_tests.rs` tests create and read streams with the current contract implementation on both sides of a simulated upgrade. They do not install a legacy serialized value or execute old contract code, and the module is not currently registered in `lib.rs`. Those tests therefore cannot establish that a new contract can decode an old stream.

Before adding an acceptance-level compatibility test, maintainers need to specify the oldest supported storage schema and expected defaults for fields introduced later. The test can then write a fixture encoded by that schema, exercise the new contract's read path, and assert the migrated stream fields and subsequent operations. If multiple historical schemas are supported, each needs its own versioned fixture and migration assertion.

This note intentionally does not claim that backward compatibility is currently working and does not add a test that would pass without exercising legacy data.