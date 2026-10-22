#!/usr/bin/env python3
"""Cross-language timing: XCL vs Luau vs LuaJIT vs CPython.

Each language runs the same program in a separate process. Process start-up is
measured separately and reported, and the table shows both raw and
start-up-adjusted times.

    cargo build --release -p xcl-cli
    cargo build --release --manifest-path crates/xcl-benchmark/runners/luau-runner/Cargo.toml
    cargo build --release --manifest-path crates/xcl-benchmark/runners/luajit-runner/Cargo.toml
    python crates/xcl-benchmark/crosslang/run.py

Optional: benchmark the JIT build next to the interpreter. Build the JIT first,
copy it aside, then rebuild the interpreter, so both binaries stay available:

    cargo build --release -p xcl-cli --features jit
    cp target/release/xcl.exe target/release/xcl-jit.exe
    cargo build --release -p xcl-cli
    CROSSLANG_REPS=7 python crates/xcl-benchmark/crosslang/run.py

`CROSSLANG_REPS` controls the measured repetitions (default 5; one extra run is
used as warm-up on every process).

Notes:
  * Luau has no JIT. `luau` here is the interpreter (mlua's Luau backend);
    Luau's optional native codegen is an AOT backend and is not enabled.
  * LuaJIT is the real JIT comparison.
  * CPython is the system `python` (3.14 on this machine).
"""

import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

# Repository root: crosslang -> xcl-benchmark -> crates -> <repo>.
ROOT = Path(__file__).resolve().parents[3]
RUNNERS = Path(__file__).resolve().parents[1] / "runners"
HERE = Path(__file__).resolve().parent
REPS = int(os.environ.get("CROSSLANG_REPS", "5"))
TIMEOUT = 300


def exe(path: Path) -> Path:
    return path.with_suffix(".exe") if os.name == "nt" else path


XCL = exe(ROOT / "target/release/xcl")
XCL_JIT = exe(ROOT / "target/release/xcl-jit")
LUAU = exe(RUNNERS / "luau-runner/target/release/luau-runner")
LUAJIT = exe(RUNNERS / "luajit-runner/target/release/luajit-runner")

LANGS = [("xcl", [str(XCL)], "xcl", ".xcl")]
# Optional second XCL column: an interpreter and a `--features jit` build can
# be benchmarked side by side when both binaries exist.
if XCL_JIT.exists():
    LANGS.append(("xcl-jit", [str(XCL_JIT)], "xcl", ".xcl"))
LANGS += [
    ("luau", [str(LUAU)], "lua", ".lua"),
    ("luajit", [str(LUAJIT)], "lua", ".lua"),
    ("python", [sys.executable], "python", ".py"),
]

WORKLOADS = [
    "int_loop",
    "lcg",
    "bitwise",
    "fib",
    "hanoi",
    "map_read",
    "array_sum",
    "entity_oop",
    "native_call",
]


def run_once(cmd, script):
    t0 = time.perf_counter()
    proc = subprocess.run(
        cmd + [str(script)],
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
    )
    elapsed = time.perf_counter() - t0
    if proc.returncode != 0:
        raise RuntimeError(f"{script} failed:\n{proc.stderr}")
    out = proc.stdout.strip().split()
    value = int(out[-1]) if out else None
    return elapsed, value


def measure(cmd, script, reps=REPS):
    times = []
    checksums = set()
    for _ in range(reps + 1):  # first is warm-up
        elapsed, value = run_once(cmd, script)
        if value is not None:
            checksums.add(value)
        times.append(elapsed)
    return min(times[1:]), (checksums.pop() if len(checksums) == 1 else None)


def startup():
    """Time a trivial program per language."""
    scripts = {
        "xcl": HERE / "xcl/startup.xcl",
        "xcl-jit": HERE / "xcl/startup.xcl",
        "luau": HERE / "lua/startup.lua",
        "luajit": HERE / "lua/startup.lua",
        "python": HERE / "python/startup.py",
    }
    out = {}
    for name, cmd, _, _ in LANGS:
        t, _ = measure(cmd, scripts[name], reps=7)
        out[name] = t
    return out


def main():
    for binary in (XCL, LUAU, LUAJIT):
        if not binary.exists():
            print(f"missing binary: {binary}", file=sys.stderr)
            return 1

    selected = sys.argv[1:] or WORKLOADS
    unknown = [w for w in selected if w not in WORKLOADS]
    if unknown:
        print(f"unknown workload(s): {', '.join(unknown)}", file=sys.stderr)
        print(f"known: {', '.join(WORKLOADS)}", file=sys.stderr)
        return 1

    print(f"xcl     : {XCL}")
    if XCL_JIT.exists():
        print(f"xcl-jit : {XCL_JIT}")
    print()

    base = startup()
    print("start-up (median of 7, one process):")
    for name, _, _, _ in LANGS:
        print(f"  {name:8s} {base[name] * 1000:7.2f} ms")
    print()

    results = {}
    checks = {}
    for workload in selected:
        row = {}
        checksums = {}
        for name, cmd, folder, ext in LANGS:
            name_override = {
                ("luau", "bitwise"): HERE / "lua/bitwise_luau.lua",
                ("luajit", "bitwise"): HERE / "lua/bitwise_luajit.lua",
            }.get((name, workload))
            script = name_override or (HERE / folder / (workload + ext))
            if not script.exists():
                raise SystemExit(f"missing script: {script}")
            raw, value = measure(cmd, script)
            row[name] = max(0.0, raw - base[name])
            checksums[name] = value
        results[workload] = row
        checks[workload] = checksums
        if len(set(checksums.values())) != 1:
            print(f"  WARNING {workload}: checksums differ: {checksums}")

    names = [name for name, _, _, _ in LANGS]
    header = f"{'workload':14s}" + "".join(f"{n:>10s}" for n in names) + "   xcl/py  xcl/lj xcl/luau"
    if "xcl-jit" in names:
        header += " xcl/jit"
    print(header)
    print("-" * len(header))
    for workload in selected:
        row = results[workload]
        line = f"{workload:14s}" + "".join(f"{row[n] * 1000:10.2f}" for n in names)

        def ratio(num, den):
            return num / den if den > 0 else float("inf")

        line += (
            f"   {ratio(row['xcl'], row['python']):6.2f}"
            f"  {ratio(row['xcl'], row['luajit']):6.2f}"
            f"  {ratio(row['xcl'], row['luau']):7.2f}"
        )
        if "xcl-jit" in names:
            line += f"  {ratio(row['xcl'], row['xcl-jit']):7.2f}"
        print(line)

    print()
    print("times are ms, minimum of", REPS, "runs, process start-up subtracted")
    print("checksums:", {w: checks[w]["xcl"] for w in selected})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
