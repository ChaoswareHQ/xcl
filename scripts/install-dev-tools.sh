#!/usr/bin/env bash
# Install the profiling/investigation tools used by the XCL benchmarks.
# Safe to re-run: already-installed tools are skipped.
set -euo pipefail

install_if_missing() {
    local command="$1"
    local crate="$2"
    if command -v "$command" >/dev/null 2>&1; then
        echo "[ok] $command already installed"
    else
        echo "[install] $crate"
        cargo install --locked "$crate"
    fi
}

install_if_missing samply samply
install_if_missing critcmp critcmp
install_if_missing cargo-asm cargo-show-asm
install_if_missing cargo-bloat cargo-bloat

rustup component add rustfmt clippy llvm-tools-preview rust-src
echo "Done. For deterministic microbenchmarks, use iai-callgrind under WSL2 (see docs/benchmarking.md)."
