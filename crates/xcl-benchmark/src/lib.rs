//! Benchmark corpus and harnesses for XCL.
//!
//! This crate owns everything a benchmark needs that is not itself a benchmark
//! target: the cross-language programs, the harnesses that run them as separate
//! processes, and the standalone Luau / LuaJIT runners.
//!
//! The criterion benchmarks stay in the crates they measure (`xcl-toolchains/benches`,
//! `xcl-rt/benches`) because they need those crates' dev-dependencies and
//! `[[bench]]` entries. This crate is the other half: the part that compares XCL
//! against other runtimes.
//!
//! # Layout
//!
//! | Path | Holds |
//! |---|---|
//! | `crosslang/run.py` | timing harness: one process per language, start-up subtracted |
//! | `crosslang/usage.py` | CPU cycles, wall time and peak working set per process |
//! | `crosslang/{xcl,lua,python}/` | the same programs in each language |
//! | `runners/luau-runner/` | standalone mlua-based Luau interpreter (own workspace) |
//! | `runners/luajit-runner/` | standalone mlua-based LuaJIT interpreter (own workspace) |
//! | `scripts/score.py` | XCL-vs-Luau scoreboard from criterion's saved baselines |
//! | `scripts/install-dev-tools.*` | profiling tools (callgrind, hyperfine, dhat, …) |
//!
//! # Running
//!
//! ```sh
//! cargo build --release -p xcl-cli
//! cargo build --release --manifest-path crates/xcl-benchmark/runners/luau-runner/Cargo.toml
//! cargo build --release --manifest-path crates/xcl-benchmark/runners/luajit-runner/Cargo.toml
//! python crates/xcl-benchmark/crosslang/run.py               # all workloads
//! python crates/xcl-benchmark/crosslang/run.py native_call   # one workload
//! ```
//!
//! # Adding a workload
//!
//! Add `crosslang/xcl/<name>.xcl`, `crosslang/lua/<name>.lua`, and
//! `crosslang/python/<name>.py`, then add the name to [`WORKLOADS`] and to the
//! `WORKLOADS` list in `crosslang/run.py`. Every program prints the same
//! checksum; the harness fails the row when the languages disagree.

use std::path::{Path, PathBuf};

/// Languages the cross-language harness compares.
pub const LANGUAGES: &[&str] = &["xcl", "luau", "luajit", "python"];

/// Workloads in `crosslang/`, in scoreboard order.
///
/// Keep in sync with the `WORKLOADS` list in `crosslang/run.py` and
/// `crosslang/usage.py`; each must have a program in every language directory.
pub const WORKLOADS: &[&str] = &[
    "int_loop",
    "lcg",
    "bitwise",
    "fib",
    "hanoi",
    "map_read",
    "array_sum",
    "entity_oop",
    "native_call",
];

/// Root of this crate.
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Directory holding `run.py`, `usage.py` and the per-language programs.
pub fn crosslang_dir() -> PathBuf {
    root().join("crosslang")
}

/// The runner binaries live in their own workspaces, so their target directory
/// is per runner rather than the repository's.
pub fn runner_exe(runner: &str) -> PathBuf {
    let exe = if cfg!(windows) {
        format!("{runner}.exe")
    } else {
        runner.to_string()
    };
    root()
        .join("runners")
        .join(runner)
        .join("target")
        .join("release")
        .join(exe)
}

/// Every `(language, program)` pair for one workload, as it exists on disk.
///
/// Lua is the one language that can need more than one file: `bitwise` has
/// `bitwise_luau.lua` and `bitwise_luajit.lua` because the two runtimes expose
/// different bit libraries, and the harness picks per runner. This returns the
/// Luau variant, which is what the shared `lua/` directory means.
///
/// Returns `None` when a language is missing a program, which is what a new
/// workload looks like before all three files are added.
pub fn workload_files(name: &str) -> Option<Vec<(String, PathBuf)>> {
    let mut out = Vec::with_capacity(3);
    for (lang, ext) in [("xcl", "xcl"), ("lua", "lua"), ("python", "py")] {
        let dir = crosslang_dir().join(lang);
        let candidates = [
            dir.join(format!("{name}.{ext}")),
            dir.join(format!("{name}_luau.{ext}")),
        ];
        let path = candidates.into_iter().find(|p| p.is_file())?;
        out.push((lang.to_string(), path));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_workload_has_a_program_in_every_language() {
        for name in WORKLOADS {
            let files = workload_files(name).unwrap_or_else(|| panic!("{name} is incomplete"));
            assert_eq!(files.len(), 3, "{name}");
            for (lang, path) in files {
                assert!(
                    path.metadata().unwrap().len() > 0,
                    "{name} has an empty {lang} program"
                );
            }
        }
    }

    #[test]
    fn the_harnesses_are_where_the_docs_say() {
        assert!(crosslang_dir().join("run.py").is_file());
        assert!(crosslang_dir().join("usage.py").is_file());
        assert!(root().join("scripts/score.py").is_file());
    }

    #[test]
    fn runners_live_in_their_own_workspaces() {
        for runner in ["luau-runner", "luajit-runner"] {
            let manifest = root().join("runners").join(runner).join("Cargo.toml");
            let text = std::fs::read_to_string(&manifest).expect("runner manifest");
            assert!(
                text.contains("[workspace]"),
                "{runner} must stay out of the xcl workspace: mlua cannot enable its luau and luajit backends in one dependency graph"
            );
        }
    }
}
