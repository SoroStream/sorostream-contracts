#!/usr/bin/env python3
"""Ensure stream test, benchmark, and fuzz source files are wired in lib.rs."""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "contracts/stream/src"
LIB = (SRC / "lib.rs").read_text()
INVENTORY = (ROOT / "contracts/stream/TEST_MODULES.md").read_text()


def is_checkable(path: Path) -> bool:
    name = path.stem
    return name == "test" or name.endswith(("_tests", "_bench", "_fuzz")) or name in {
        "chaos", "differential_fuzz"
    }

# Files deliberately left unwired, with the reason documented in
# TEST_MODULES.md's "Known dormant file" section. Wiring one of these back in
# means fixing it first, then removing it from this set.
EXEMPT_FROM_WIRING = {
    "test",
    "feature_tests",
    "entry_point_tests",
    "protocol_economics_tests",
    "batch_cancel_tests",
    "paginated_recipient_tests",
    "stream_analytics_tests",
    "storage_bench",
    "subscription_tests",
    "issue_516_tests",
    "decay_vesting_tests",
    "tranche_oracle_tests",
    "bonus_tests",
    "sender_whitelist_tests",
    "stream_manager_tests",
    "oracle_rate_adjustment_tests",
    "acl_tests",
    "cascade_tests",
    "upgrade_migration_tests",
    "upgrade_proposal_tests",
    "role_holders_tests",
    "issue_629_tests",
    "issue_638_tests",
    "issue_640_tests",
    "issue_641_tests",
    "issue_643_tests",
    "stream_nft_tests",
    "grace_period_tests",
    "event_field_tests",
    "issue_633_tests",
    "issue_634_tests",
    "issue_644_tests",
    "issue_623_tests",
    "issue_608_tests",
    "issue_609_tests",
    "issue_625_tests",
    "issue_661_tests",
    "issue_605_tests",
    "issue_606_tests",
    "stream_tags_tests",
    "claimable_boundary_tests",
    "issue_401_tests",
    "issue_517_tests",
    "issue_518_tests",
    "issue_602_tests",
    "issue_603_tests",
    "issue_604_tests",
    "issue_607_tests",
    "issue_622_tests",
    "issue_628_tests",
    "lifecycle_integration_tests",
    "storage_layout_snapshot_tests",
}

files = {p.stem for p in SRC.glob("*.rs") if is_checkable(p)}
# Keep this paired with the explanatory comment above the test module block in lib.rs.
mods = set(re.findall(r"^\s*#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+(\w+)\s*;", LIB, re.M))
missing = sorted(files - mods - EXEMPT_FROM_WIRING)
stale = sorted(mods - files)
documented = dict(re.findall(r"^\| `([\w]+)\.rs` \| (Wired|Dormant) \|$", INVENTORY, re.M))
expected = {
    name: ("Wired" if name in mods else "Dormant")
    for name in files
}
inventory_drift = sorted(name for name in files | set(documented) if documented.get(name) != expected.get(name))
if missing or stale:
    if missing:
        print("Test/bench/fuzz files without #[cfg(test)] mod declarations:", *missing, sep="\n  ", file=sys.stderr)
    if stale:
        print("#[cfg(test)] mod declarations without matching source files:", *stale, sep="\n  ", file=sys.stderr)
    sys.exit(1)
if inventory_drift:
    print("TEST_MODULES.md is missing, stale, or has an incorrect status for:", *inventory_drift, sep="\n  ", file=sys.stderr)
    sys.exit(1)
print(f"Verified {len(files)} stream test/benchmark/fuzz modules and their inventory.")
