#!/usr/bin/env python3
"""CPU and peak-memory comparison: XCL vs Luau vs LuaJIT vs CPython.

Runs the same programs as `run.py`, one process per sample, and records for
each process:

  * wall time (perf_counter around the whole process);
  * CPU cycles (`QueryProcessCycleTime`) — see the note below;
  * peak working set / RSS (`GetProcessMemoryInfo`).

CPU time comes from the per-process cycle counter converted with a frequency
calibrated at start-up (see the note below); wall time is the median of the
same samples. Both include process start-up, which is reported separately so
the compute-only part remains visible.

Why cycles instead of `GetProcessTimes`: Windows updates per-process CPU
accounting once per scheduler tick, i.e. in 15.625 ms steps. The XCL JIT needs
3-10 ms for `fib`/`hanoi`, so that counter cannot resolve the work at all (it
reports 0 or 1 tick). The per-process cycle counter is updated continuously and
was stable to a few percent across runs. It is converted to an approximate CPU
time with a frequency calibrated at start-up by a ~1 s CPU-bound process;
ratios between languages are unaffected by the calibration.

    cargo build --release -p xcl-cli --features jit
    cp target/release/xcl.exe target/release/xcl-jit.exe
    cargo build --release -p xcl-cli
    python crates/xcl-benchmark/crosslang/usage.py

`CROSSLANG_REPS` controls the measured repetitions (default 5; one extra run
per language/workload is used as warm-up), exactly like `run.py`.

Requires Windows for the process counters (ctypes, no third-party modules).
"""

import ctypes
import os
import statistics
import subprocess
import sys
import time
from ctypes import wintypes
from pathlib import Path

# Repository root: crosslang -> xcl-benchmark -> crates -> <repo>.
ROOT = Path(__file__).resolve().parents[3]
RUNNERS = Path(__file__).resolve().parents[1] / "runners"
HERE = Path(__file__).resolve().parent
REPS = int(os.environ.get("CROSSLANG_REPS", "5"))
TIMEOUT = 600


def exe(path: Path) -> Path:
    return path.with_suffix(".exe") if os.name == "nt" else path


XCL = exe(ROOT / "target/release/xcl")
XCL_JIT = exe(ROOT / "target/release/xcl-jit")
LUAU = exe(RUNNERS / "luau-runner/target/release/luau-runner")
LUAJIT = exe(RUNNERS / "luajit-runner/target/release/luajit-runner")

LANGS = [("xcl", [str(XCL)], "xcl", ".xcl")]
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
    "native_call",
]

# ── Windows process counters ──────────────────────────────────────────────


class FILETIME(ctypes.Structure):
    _fields_ = [("lo", wintypes.DWORD), ("hi", wintypes.DWORD)]


class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
    _fields_ = [
        ("cb", wintypes.DWORD),
        ("page_faults", wintypes.DWORD),
        ("peak_working_set", ctypes.c_size_t),
        ("working_set", ctypes.c_size_t),
        ("quota_peak_paged", ctypes.c_size_t),
        ("quota_paged", ctypes.c_size_t),
        ("quota_peak_nonpaged", ctypes.c_size_t),
        ("quota_nonpaged", ctypes.c_size_t),
        ("pagefile", ctypes.c_size_t),
        ("peak_pagefile", ctypes.c_size_t),
    ]


if os.name == "nt":
    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _psapi = ctypes.WinDLL("psapi", use_last_error=True)
    _kernel32.GetProcessTimes.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(FILETIME),
        ctypes.POINTER(FILETIME),
        ctypes.POINTER(FILETIME),
        ctypes.POINTER(FILETIME),
    ]
    _kernel32.QueryProcessCycleTime.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(ctypes.c_ulonglong),
    ]
    _psapi.GetProcessMemoryInfo.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(PROCESS_MEMORY_COUNTERS),
        wintypes.DWORD,
    ]


def process_usage(handle) -> tuple[int, float, int]:
    """Return (cpu cycles, CPU seconds from tick accounting, peak RSS bytes)."""
    cycles = ctypes.c_ulonglong()
    ok = _kernel32.QueryProcessCycleTime(handle, ctypes.byref(cycles))

    created, exited, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
    if not _kernel32.GetProcessTimes(
        handle,
        ctypes.byref(created),
        ctypes.byref(exited),
        ctypes.byref(kernel),
        ctypes.byref(user),
    ):
        raise ctypes.WinError(ctypes.get_last_error())
    ticks = ((kernel.hi << 32) | kernel.lo) + ((user.hi << 32) | user.lo)

    counters = PROCESS_MEMORY_COUNTERS()
    counters.cb = ctypes.sizeof(PROCESS_MEMORY_COUNTERS)
    if not _psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
        raise ctypes.WinError(ctypes.get_last_error())

    return (cycles.value if ok else 0), ticks / 1e7, counters.peak_working_set


def run_once(cmd, script) -> tuple[float, int, float, int, int | None]:
    """Wall seconds, cycles, CPU seconds, peak RSS bytes, printed checksum."""
    t0 = time.perf_counter()
    proc = subprocess.Popen(
        cmd + [str(script)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    out, err = proc.communicate(timeout=TIMEOUT)
    wall = time.perf_counter() - t0
    if proc.returncode != 0:
        raise RuntimeError(f"{script} failed:\n{err}")
    cycles, cpu, peak = process_usage(proc._handle)
    tokens = out.strip().split()
    value = int(tokens[-1]) if tokens else None
    return wall, cycles, cpu, peak, value


def measure(cmd, script) -> dict:
    walls, cycles, cpus, peaks, checksums = [], [], [], [], set()
    for i in range(REPS + 1):  # first sample is warm-up
        wall, cyc, cpu, peak, value = run_once(cmd, script)
        if value is not None:
            checksums.add(value)
        if i == 0:
            continue
        walls.append(wall)
        cycles.append(cyc)
        cpus.append(cpu)
        peaks.append(peak)
    return {
        # Median, not minimum: the baseline and the workload are separate
        # processes, and at 3-20 ms the spread is large enough that a
        # min-vs-min subtraction can go negative. The median is stable for
        # both the counter and the wall clock.
        "wall": statistics.median(walls),
        "cycles": statistics.median(cycles),
        "cpu_ticks": statistics.median(cpus),
        # Peak memory is stable; median avoids a single unlucky allocation.
        "peak": statistics.median(peaks),
        "check": checksums.pop() if len(checksums) == 1 else None,
    }


def calibrate_frequency() -> float:
    """Cycles per second of a 100%-busy single-threaded process."""
    src = "x = 0\nfor i in range(10_000_000):\n    x = (x + i) & 0xFFFFFF"
    proc = subprocess.Popen(
        [sys.executable, "-c", src],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    t0 = time.perf_counter()
    proc.communicate(timeout=TIMEOUT)
    wall = time.perf_counter() - t0
    cycles, _, _ = process_usage(proc._handle)
    if cycles == 0 or wall <= 0:
        return 2.0e9
    return cycles / wall


def script_for(lang: str, workload: str) -> Path:
    override = {
        ("luau", "bitwise"): HERE / "lua/bitwise_luau.lua",
        ("luajit", "bitwise"): HERE / "lua/bitwise_luajit.lua",
    }.get((lang, workload))
    if override:
        return override
    for name, _, folder, ext in LANGS:
        if name == lang:
            return HERE / folder / (workload + ext)
    raise KeyError(lang)


def main() -> int:
    if os.name != "nt":
        print("this script uses the Windows process counters", file=sys.stderr)
        return 1
    for binary in (XCL, LUAU, LUAJIT):
        if not binary.exists():
            print(f"missing binary: {binary}", file=sys.stderr)
            return 1

    print(f"xcl     : {XCL}")
    if XCL_JIT.exists():
        print(f"xcl-jit : {XCL_JIT}")

    freq = calibrate_frequency()
    print(f"calibration: {freq / 1e9:.2f} GHz (cycles per second, 100% busy)")
    print()

    names = [name for name, _, _, _ in LANGS]
    base = {}
    startup_scripts = {
        "xcl": HERE / "xcl/startup.xcl",
        "xcl-jit": HERE / "xcl/startup.xcl",
        "luau": HERE / "lua/startup.lua",
        "luajit": HERE / "lua/startup.lua",
        "python": HERE / "python/startup.py",
    }
    for name, cmd, _, _ in LANGS:
        base[name] = measure(cmd, startup_scripts[name])

    def cpu_ms(sample: dict) -> float:
        if sample["cycles"] > 0:
            return sample["cycles"] / freq * 1000.0
        return sample["cpu_ticks"] * 1000.0

    print(f"baseline (start-up only, median of {REPS}):")
    print(f"{'':10s}" + "".join(f"{n:>12s}" for n in names))
    print(f"{'wall ms':10s}" + "".join(f"{base[n]['wall'] * 1000:12.2f}" for n in names))
    print(f"{'cpu ms':10s}" + "".join(f"{cpu_ms(base[n]):12.2f}" for n in names))
    print(
        f"{'peak MB':10s}"
        + "".join(f"{base[n]['peak'] / 2**20:12.2f}" for n in names)
    )
    print()

    results = {}
    bad_checks = {}
    for workload in WORKLOADS:
        row = {}
        checks = {}
        for name, cmd, _, _ in LANGS:
            sample = measure(cmd, script_for(name, workload))
            wall = max(0.0, sample["wall"] - base[name]["wall"])
            cpu_total = cpu_ms(sample)
            cpu = max(0.0, cpu_total - cpu_ms(base[name]))
            row[name] = {
                "wall": wall,
                "cpu": cpu,
                # Whole-process values: start-up included is the only figure
                # that is stable when the work itself is a few milliseconds.
                "cpu_total": cpu_total,
                "wall_total": sample["wall"],
                "peak": sample["peak"] / 2**20,
                "check": sample["check"],
            }
            checks[name] = sample["check"]
        results[workload] = row
        if len(set(checks.values())) != 1:
            bad_checks[workload] = checks

    def table(title, unit, key, fmt="{:.2f}"):
        header = f"{'workload':12s}" + "".join(f"{n:>10s}" for n in names)
        if "xcl-jit" in names:
            header += "   jit/lj  jit/luau  jit/py"
        print(title)
        print(header)
        print("-" * len(header))
        for workload in WORKLOADS:
            row = results[workload]
            line = f"{workload:12s}" + "".join(
                f"{fmt.format(row[n][key]):>10s}" for n in names
            )

            def ratio(num, den):
                return num / den if den > 0 else float("inf")

            if "xcl-jit" in names:
                jit = row["xcl-jit"][key]
                line += (
                    f"  {ratio(jit, row['luajit'][key]):6.2f}"
                    f"  {ratio(jit, row['luau'][key]):8.2f}"
                    f"  {ratio(jit, row['python'][key]):6.2f}"
                )
            print(line)
        print(f"  ({unit})")
        print()

    table(
        "CPU time, whole process incl. start-up (ms, median):",
        "ms, lower is better; the baseline row above is part of every number",
        "cpu_total",
    )
    table(
        "CPU work above start-up (ms, median):",
        "ms; short workloads (< 5 ms) are within noise of the baseline",
        "cpu",
    )

    print("CPU utilisation, whole process (cpu / wall, %):")
    header = f"{'workload':12s}" + "".join(f"{n:>10s}" for n in names)
    print(header)
    print("-" * len(header))
    for workload in WORKLOADS:
        row = results[workload]
        line = f"{workload:12s}"
        for name in names:
            wall = row[name]["wall_total"] * 1000.0
            pct = 100.0 * row[name]["cpu_total"] / wall if wall > 0 else 0.0
            line += f"{pct:10.1f}"
        print(line)
    print("  (100% = one core; short bursts can read slightly >100% because")
    print("   the calibration frequency is a sustained, not a boosted, clock)")
    print()

    table("Peak working set / RSS, raw (median MB):", "MB, lower is better", "peak")

    if XCL_JIT.exists():
        print("XCL JIT vs the others, per workload (lower is better):")
        print(
            f"{'workload':12s}{'cpu vs lj':>10s}{'cpu vs luau':>13s}"
            f"{'cpu vs py':>11s}{'mem vs lj':>11s}{'mem vs luau':>13s}{'mem vs py':>11s}"
        )
        print("-" * 81)
        for workload in WORKLOADS:
            row = results[workload]

            def ratio(num, den):
                return num / den if den > 0 else float("inf")

            jit = row["xcl-jit"]
            print(
                f"{workload:12s}"
                f"{ratio(jit['cpu_total'], row['luajit']['cpu_total']):10.2f}"
                f"{ratio(jit['cpu_total'], row['luau']['cpu_total']):13.2f}"
                f"{ratio(jit['cpu_total'], row['python']['cpu_total']):11.2f}"
                f"{ratio(jit['peak'], row['luajit']['peak']):11.2f}"
                f"{ratio(jit['peak'], row['luau']['peak']):13.2f}"
                f"{ratio(jit['peak'], row['python']['peak']):11.2f}"
            )
        print()

    if bad_checks:
        print("WARNING: checksums differ:", bad_checks, file=sys.stderr)
        return 1
    print("checksums:", {w: results[w]["xcl"]["check"] for w in WORKLOADS})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
