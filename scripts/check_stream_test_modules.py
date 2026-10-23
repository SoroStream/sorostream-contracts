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

files = {p.stem for p in SRC.glob("*.rs") if is_checkable(p)}
# Keep this paired with the explanatory comment above the test module block in lib.rs.
mods = set(re.findall(r"^\s*#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+(\w+)\s*;", LIB, re.M))
missing = sorted(files - mods)
stale = sorted(mods - files)
documented = dict(re.findall(r"^\| `([\w]+)\.rs` \| (Wired|Dormant) \|$", INVENTORY, re.M))
expected = {name: "Wired" if name in mods else "Dormant" for name in files}
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
