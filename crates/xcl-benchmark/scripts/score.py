#!/usr/bin/env python3
"""Print the XCL-vs-Luau scoreboard from criterion's saved baselines.

Usage:
    python crates/xcl-benchmark/scripts/score.py [baseline]

`baseline` defaults to criterion's "new" baseline. Run the suites first, e.g.

    cargo bench -p xcl-vm --bench vm_core --bench vm_data \
        --bench vm_algorithms --bench vm_games -- --save-baseline full-after
    python crates/xcl-benchmark/scripts/score.py full-after
"""

import glob
import json
import os
import sys

BASELINE = sys.argv[1] if len(sys.argv) > 1 else "new"
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "..", "..", "target", "criterion")


def mean_ns(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)["mean"]["point_estimate"]


def main():
    rows = []
    pattern = os.path.join(ROOT, "*", "*_xcl", BASELINE, "estimates.json")
    for xcl_path in glob.glob(pattern):
        bench_dir = os.path.dirname(os.path.dirname(xcl_path))
        group = os.path.basename(os.path.dirname(bench_dir))
        name = os.path.basename(bench_dir)[: -len("_xcl")]
        luau_path = os.path.join(ROOT, group, name + "_luau", BASELINE, "estimates.json")
        if not os.path.exists(luau_path):
            continue
        xcl = mean_ns(xcl_path)
        luau = mean_ns(luau_path)
        rows.append((xcl / luau, group, name, xcl, luau))

    if not rows:
        print(f"no results for baseline {BASELINE!r} under {ROOT}/")
        return 1

    rows.sort()
    print(f"{'ratio':>7}  {'xcl':>10}  {'luau':>10}  benchmark")
    for ratio, group, name, xcl, luau in rows:
        print(f"{ratio:7.3f}  {xcl / 1e3:9.1f}us  {luau / 1e3:9.1f}us  {group}/{name}")

    wins = sum(1 for row in rows if row[0] < 1.0)
    print()
    print(f"total={len(rows)}  faster={wins}  slower={len(rows) - wins}")
    losses = [row for row in rows if row[0] >= 1.0]
    if losses:
        print("losses:")
        for ratio, group, name, _xcl, _luau in losses:
            print(f"  {ratio:.3f}  {group}/{name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
