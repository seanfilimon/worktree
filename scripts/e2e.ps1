# Build every component and run the W0rkTree end-to-end suite
# (crates/worktree-e2e). Extra arguments are passed to the test binary,
# e.g. `.\scripts\e2e.ps1 local_workflow` or `.\scripts\e2e.ps1 -Threads 1 --nocapture`.
#
# Env: failing scenarios keep their temp dir (WT_E2E_KEEP=1 keeps all,
# WT_E2E_KEEP=0 none); CARGO_TARGET_DIR is honored.
param(
    [int]$Threads = 4,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TestArgs = @()
)

# Not 'Stop': under Windows PowerShell 5.1, redirected native stderr (cargo's
# progress output) would be turned into terminating errors. Failures are
# detected via $LASTEXITCODE instead.
$ErrorActionPreference = 'Continue'
$Root = Split-Path -Parent $PSScriptRoot
$onWindows = ($env:OS -eq 'Windows_NT')
$exe = if ($onWindows) { '.exe' } else { '' }

$targetDir = if ($env:CARGO_TARGET_DIR) {
    if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { $env:CARGO_TARGET_DIR }
    else { Join-Path $Root $env:CARGO_TARGET_DIR }
} else { Join-Path $Root 'target' }
$binDir = Join-Path $targetDir 'debug'
$serverBin = Join-Path $binDir "worktree-server$exe"

$saved = @{}
foreach ($name in 'WT_E2E', 'WT_BIN', 'WT_BG_BIN', 'WT_SERVER_BIN', 'CGO_ENABLED') {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

$code = 1
Push-Location $Root
try {
    Write-Host '==> cargo build -p worktree-cli -p worktree-bg'
    cargo build -p worktree-cli -p worktree-bg
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }

    Write-Host '==> go build services/server-go'
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    $env:CGO_ENABLED = '0'
    Push-Location (Join-Path $Root 'services/server-go')
    try {
        go build -o $serverBin .
        if ($LASTEXITCODE -ne 0) { throw "go build failed ($LASTEXITCODE)" }
    } finally { Pop-Location }

    $env:WT_BIN = Join-Path $binDir "wt$exe"
    $env:WT_BG_BIN = Join-Path $binDir "worktree-bg$exe"
    $env:WT_SERVER_BIN = $serverBin
    $env:WT_E2E = '1'

    Write-Host "==> WT_E2E=1 cargo test -p worktree-e2e -- --test-threads=$Threads $TestArgs"
    cargo test -p worktree-e2e -- "--test-threads=$Threads" @TestArgs
    $code = $LASTEXITCODE
} catch {
    Write-Error $_ -ErrorAction Continue
    $code = 1
} finally {
    Pop-Location
    foreach ($name in $saved.Keys) {
        [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
    }
}
exit $code
