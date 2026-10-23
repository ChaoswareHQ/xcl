# xcl-benchmark

Benchmark corpus and cross-language harnesses for XCL.

This crate is not a benchmark target. Criterion benches live next to the code
they measure (`xcl-vm/benches`, `xcl-plugins/benches`); this crate holds the
other half — the programs and harnesses that compare XCL against Luau, LuaJIT
and CPython as separate processes, plus the standalone runners they need.

## Layout

| Path | Holds |
|---|---|
| `crosslang/run.py` | timing harness, one process per language, start-up subtracted |
| `crosslang/usage.py` | CPU cycles, wall time and peak working set per process (Windows) |
| `crosslang/xcl/`, `crosslang/lua/`, `crosslang/python/` | the same program in each language |
| `runners/luau-runner/` | mlua-based Luau interpreter (standalone workspace) |
| `runners/luajit-runner/` | mlua-based LuaJIT interpreter (standalone workspace) |
| `scripts/score.py` | XCL-vs-Luau scoreboard from criterion baselines |
| `scripts/install-dev-tools.*` | profiling tools (callgrind, hyperfine, dhat, …) |

The two runners are **standalone crates** (they declare an empty `[workspace]`).
mlua cannot enable its `luau` and `luajit` backends in one dependency graph, so
they must stay out of the xcl workspace.

## Running

```sh
cargo build --release
cargo build --release --manifest-path crates/xcl-benchmark/runners/luau-runner/Cargo.toml
cargo build --release --manifest-path crates/xcl-benchmark/runners/luajit-runner/Cargo.toml

python crates/xcl-benchmark/crosslang/run.py                # every workload
python crates/xcl-benchmark/crosslang/run.py native_call    # one workload
CROSSLANG_REPS=9 python crates/xcl-benchmark/crosslang/run.py
python crates/xcl-benchmark/crosslang/usage.py              # CPU + peak memory
```

The JIT tier is measured by copying the JIT build aside first, so both binaries
exist:

```sh
cargo build --release -p xcl-runtime --features jit
mkdir -p target/release/jit
cp target/release/xcl target/release/jit/xcl
cp target/release/xcl_rt.dll target/release/jit/
cargo build --release -p xcl-runtime
```

## Adding a workload

1. Add `crosslang/xcl/<name>.xcl`, `crosslang/lua/<name>.lua`,
   `crosslang/python/<name>.py`. Each prints the same value.
2. Add the name to `WORKLOADS` in `crosslang/run.py`, `crosslang/usage.py`, and
   to `WORKLOADS` in `src/lib.rs`.
3. `cargo test -p xcl-benchmark` checks that all three programs exist and are
   non-empty.

## Scoreboard

```sh
cargo bench -p xcl-vm --bench vm_core --bench vm_data \
    --bench vm_algorithms --bench vm_games -- --save-baseline run
python crates/xcl-benchmark/scripts/score.py run
```
