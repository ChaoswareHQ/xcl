# Install the profiling/investigation tools used by the XCL benchmarks.
# Safe to re-run: already-installed tools are skipped.
$ErrorActionPreference = "Stop"

$tools = @(
    @{ Command = "samply";     Crate = "samply" },
    @{ Command = "critcmp";    Crate = "critcmp" },
    @{ Command = "cargo-asm";  Crate = "cargo-show-asm" },
    @{ Command = "cargo-bloat"; Crate = "cargo-bloat" }
)

foreach ($tool in $tools) {
    if (Get-Command $tool.Command -ErrorAction SilentlyContinue) {
        Write-Host "[ok] $($tool.Command) already installed"
    } else {
        Write-Host "[install] $($tool.Crate)"
        cargo install --locked $tool.Crate
    }
}

Write-Host "[rustup] ensuring toolchain components"
rustup component add rustfmt clippy llvm-tools-preview rust-src
Write-Host "Done. For deterministic microbenchmarks, use iai-callgrind under WSL2 (see docs/benchmarking.md)."
