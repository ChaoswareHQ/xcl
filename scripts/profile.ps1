# Profile script for XCL interpreter optimization baseline
#
# Usage:
#   .\scripts\profile.ps1              # Run benchmarks with opcode stats, save baseline
#   .\scripts\profile.ps1 -Flamegraph  # Generate flamegraph (requires cargo-flamegraph)
#   .\scripts\profile.ps1 -BenchOnly   # Run benchmarks only, no opcode stats
#
# Prerequisites:
#   - cargo-flamegraph (optional, for flamegraph generation)
#     Install with: cargo install flamegraph

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
    $benchOutput = cargo bench -p xcl-vm --bench dispatch -- --noplot 2>&1 | Where-Object { $_ -notmatch 'CategoryInfo|FullyQualifiedErrorId|RemoteException' } | Out-String
} else {
    $benchOutput = cargo bench -p xcl-vm --features opcode-stats --bench dispatch -- --noplot 2>&1 | Where-Object { $_ -notmatch 'CategoryInfo|FullyQualifiedErrorId|RemoteException' } | Out-String
}

Write-Host $benchOutput

# Step 2: Generate flamegraph (optional)
$flamegraphPath = $null
if ($Flamegraph) {
    Write-Host ""
    Write-Host "[2/3] Generating flamegraph..." -ForegroundColor Yellow
    Write-Host "Note: This requires cargo-flamegraph installed." -ForegroundColor Gray
    Write-Host "Install with: cargo install flamegraph" -ForegroundColor Gray
    Write-Host ""

    if (Get-Command cargo-flamegraph -ErrorAction SilentlyContinue) {
        $flamegraphPath = Join-Path $StateDir "flamegraph.svg"
        Push-Location $ProjectRoot
        cargo flamegraph --root xcl-vm --bench dispatch --freq 999 --min-width 0.05
        Pop-Location

        # Move flamegraph to state dir if it exists
        if (Test-Path "flamegraph.svg") {
            Move-Item -Force "flamegraph.svg" $flamegraphPath
            Write-Host "Flamegraph saved to: $flamegraphPath" -ForegroundColor Green
        } else {
            Write-Host "Flamegraph generation did not produce output." -ForegroundColor Red
        }
    } else {
        Write-Host "cargo-flamegraph not found. Skipping flamegraph generation." -ForegroundColor Red
        Write-Host "Install with: cargo install flamegraph" -ForegroundColor Gray
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

## Build Profiles

- **bench-flamegraph**: Debug info + opt-level 1 for flamegraph readability
- **release-lto-debug**: LTO release with debuginfo for BOLT re-compilation

## How to Use

1. Run benchmarks: `cargo bench -p xcl-vm`
2. Run with opcode stats: `cargo bench -p xcl-vm --features opcode-stats`
3. Generate flamegraph: `cargo flamegraph --bench dispatch`
4. Compare after optimization: re-run and diff results
"@

[System.IO.File]::WriteAllText($BaselineFile, $baselineContent)
Write-Host "Baseline saved to: $BaselineFile" -ForegroundColor Green

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Profiling complete!" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
