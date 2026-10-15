# Profile script for XCL interpreter optimization baseline
#
# Usage:
#   .\scripts\profile.ps1              # Run benchmarks with opcode stats, save baseline
#   .\scripts\profile.ps1 -Flamegraph  # Generate flamegraph via samply (opens Firefox Profiler)
#   .\scripts\profile.ps1 -BenchOnly   # Run benchmarks only, no opcode stats
#
# Prerequisites:
#   - samply (for flamegraph generation)
#     Install with: cargo install --locked samply
#
# Note: On Windows, cargo-flamegraph uses the blondie backend which produces
# raw memory addresses instead of function names. Samply + Firefox Profiler
# resolves symbols properly and provides an interactive flamegraph view.

param(
    [switch]$Flamegraph,
    [switch]$BenchOnly
)

$ErrorActionPreference = "Continue"
$ProjectRoot = Split-Path (Split-Path $MyInvocation.MyCommand.Path -Parent) -Parent
$StateDir = Join-Path $ProjectRoot "state"
$BaselineFile = Join-Path $StateDir "baseline.md"

# Ensure state directory exists
if (-not (Test-Path $StateDir)) {
    New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
}

Set-Location $ProjectRoot

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  XCL Optimization Profiling" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Step 1: Run benchmarks with opcode stats
Write-Host "[1/3] Running benchmarks with opcode stats..." -ForegroundColor Yellow
$benchOutput = @()

if ($BenchOnly) {
    $benchOutput = cargo bench -p xcl-vm --bench dispatch -- --noplot 2>&1 | Where-Object { $_ -notmatch 'CategoryInfo|FullyQualifiedErrorId|RemoteException|Finished.*target' } | Out-String
} else {
    $benchOutput = cargo bench -p xcl-vm --features opcode-stats --bench dispatch -- --noplot 2>&1 | Where-Object { $_ -notmatch 'CategoryInfo|FullyQualifiedErrorId|RemoteException|Finished.*target' } | Out-String
}

Write-Host $benchOutput

# Step 2: Generate flamegraph via samply (optional)
$flamegraphPath = $null
if ($Flamegraph) {
    Write-Host ""
    Write-Host "[2/3] Generating flamegraph via samply..." -ForegroundColor Yellow
    Write-Host "Note: This requires samply installed." -ForegroundColor Gray
    Write-Host "Install with: cargo install --locked samply" -ForegroundColor Gray
    Write-Host "Firefox Profiler will open with the interactive flamegraph." -ForegroundColor Gray
    Write-Host ""

    if (Get-Command samply -ErrorAction SilentlyContinue) {
        $profilePath = Join-Path $StateDir "profile.json.gz"

        # Build the profiling example with debug info
        Write-Host "Building profiling workload..." -ForegroundColor Gray
        cargo build -p xcl-vm --example profile_workload --profile bench-flamegraph 2>&1 | Out-Null

        $binaryDir = Join-Path $ProjectRoot "target\bench-flamegraph\examples"
        $exampleBinary = Get-ChildItem $binaryDir -Filter "profile_workload.exe" -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName

        if ($exampleBinary) {
            Write-Host "Profiling with samply (requires admin privileges)..." -ForegroundColor Gray
            $samplyOutput = samply record --no-open --no-server --symbol-dir $binaryDir -o $profilePath -- $exampleBinary 2>&1

            if (Test-Path $profilePath) {
                $flamegraphPath = $profilePath
                Write-Host "Profile saved to: $flamegraphPath" -ForegroundColor Green
                Write-Host ""
                Write-Host "To view the flamegraph:" -ForegroundColor Cyan
                Write-Host "  1. Open https://profiler.firefox.com/" -ForegroundColor Cyan
                Write-Host "  2. Click 'Load' -> 'From file' and select $profilePath" -ForegroundColor Cyan
                Write-Host "  3. Or run: samply replay $profilePath" -ForegroundColor Cyan
            } else {
                Write-Host "Samply failed to produce a profile." -ForegroundColor Red
                Write-Host "Common causes:" -ForegroundColor Yellow
                Write-Host "  - Run as Administrator (xperf requires elevated privileges)" -ForegroundColor Yellow
                Write-Host "  - xperf not in PATH (install Windows SDK WPT)" -ForegroundColor Yellow
                if ($samplyOutput) {
                    Write-Host "Error output:" -ForegroundColor Red
                    Write-Host $samplyOutput -ForegroundColor Red
                }
            }
        } else {
            Write-Host "Profiling example binary not found." -ForegroundColor Red
        }
    } else {
        Write-Host "samply not found. Skipping flamegraph generation." -ForegroundColor Red
        Write-Host "Install with: cargo install --locked samply" -ForegroundColor Gray
    }
}

# Step 3: Save baseline results
Write-Host ""
Write-Host "[3/3] Saving baseline results..." -ForegroundColor Yellow

$timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

$commit = git rev-parse --short HEAD 2>$null
$flamegraphLine = if ($flamegraphPath) { "Path: $flamegraphPath" } else { "Not generated. Run: .\scripts\profile.ps1 -Flamegraph" }

$baselineContent = @"
# XCL Baseline Profile

Generated: $timestamp
Commit: $commit

## Benchmark Results

$benchOutput

## Opcode Histogram

Run with: `cargo bench -p xcl-vm --features opcode-stats -- --noplot`

## Flamegraph

$flamegraphLine

On Windows, cargo-flamegraph produces unreadable flamegraphs (raw memory addresses).
Use **samply** instead for proper symbol resolution:

1. Install: `cargo install --locked samply`
2. Profile: `.\scripts\profile.ps1 -Flamegraph`
3. View: Open the saved `profile.json.gz` in https://profiler.firefox.com/
   Or run: `samply replay state/profile.json.gz`

Firefox Profiler shows an interactive flamegraph with resolved function names,
source code links, and call tree analysis.

## Build Profiles

- **bench-flamegraph**: Debug info + opt-level 1 for flamegraph readability
- **release-lto-debug**: LTO release with debuginfo for BOLT re-compilation

## How to Use

1. Run benchmarks: `cargo bench -p xcl-vm`
2. Run with opcode stats: `cargo bench -p xcl-vm --features opcode-stats`
3. Generate flamegraph: `.\scripts\profile.ps1 -Flamegraph` (uses samply)
4. Compare after optimization: re-run and diff results
"@

[System.IO.File]::WriteAllText($BaselineFile, $baselineContent)
Write-Host "Baseline saved to: $BaselineFile" -ForegroundColor Green

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Profiling complete!" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
