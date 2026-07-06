<#
.SYNOPSIS
  W0rkTree installer for Windows.

.DESCRIPTION
  Builds the requested components from source (release profile) and installs
  them to a user-level bin directory, adding it to the user PATH.

  Components:
    cli     -> wt.exe + worktree-bg.exe   (the CLI and its background daemon)
    server  -> worktree-server.exe        (the remote multi-tenant server)
    all     -> everything (default)

.EXAMPLE
  .\scripts\install.ps1                 # install everything
  .\scripts\install.ps1 -Component cli  # just the CLI + daemon
  .\scripts\install.ps1 -Uninstall      # remove binaries and PATH entry
#>
[CmdletBinding()]
param(
    [ValidateSet("all", "cli", "server")]
    [string]$Component = "all",

    # Where binaries are installed.
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\W0rkTree\bin",

    # Skip modifying the user PATH.
    [switch]$NoPath,

    # Remove installed binaries and the PATH entry.
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step($message) { Write-Host ">> $message" -ForegroundColor Cyan }

$cliBinaries = @("wt.exe", "worktree-bg.exe")
$serverBinaries = @("worktree-server.exe")
$binaries = @()
if ($Component -eq "all" -or $Component -eq "cli") { $binaries += $cliBinaries }
if ($Component -eq "all" -or $Component -eq "server") { $binaries += $serverBinaries }

if ($Uninstall) {
    Write-Step "Uninstalling from $InstallDir"
    foreach ($bin in $binaries) {
        $path = Join-Path $InstallDir $bin
        if (Test-Path $path) {
            Remove-Item -Force $path
            Write-Host "   removed $bin"
        }
    }
    # Drop the directory and PATH entry when nothing is left.
    if ((Test-Path $InstallDir) -and -not (Get-ChildItem $InstallDir)) {
        Remove-Item -Force $InstallDir
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = $userPath -split ";" | Where-Object { $_ -and $_ -ne $InstallDir }
        [Environment]::SetEnvironmentVariable("Path", ($entries -join ";"), "User")
        Write-Host "   removed PATH entry"
    }
    Write-Host "Uninstall complete." -ForegroundColor Green
    exit 0
}

Write-Host "W0rkTree Installer" -ForegroundColor Cyan
Write-Host "==================" -ForegroundColor Cyan

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo not found. Install Rust first: https://rustup.rs" -ForegroundColor Red
    exit 1
}

$packages = @()
if ($Component -eq "all" -or $Component -eq "cli") { $packages += @("-p", "worktree-cli", "-p", "worktree-bg") }
if ($Component -eq "all" -or $Component -eq "server") { $packages += @("-p", "worktree-server") }

Write-Step "Building release binaries ($Component)..."
Push-Location $repoRoot
try {
    cargo build --release @packages
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
finally {
    Pop-Location
}

Write-Step "Installing to $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
foreach ($bin in $binaries) {
    $source = Join-Path $repoRoot "target\release\$bin"
    if (-not (Test-Path $source)) { throw "expected binary missing: $source" }
    Copy-Item -Force $source (Join-Path $InstallDir $bin)
    Write-Host "   installed $bin"
}

if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $onPath = ($userPath -split ";") -contains $InstallDir
    if (-not $onPath) {
        Write-Step "Adding $InstallDir to user PATH"
        $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    }
    # Current session too, so `wt` works immediately.
    if (-not (($env:Path -split ";") -contains $InstallDir)) {
        $env:Path = "$env:Path;$InstallDir"
    }
}

Write-Host ""
Write-Host "Installation complete." -ForegroundColor Green
if ($binaries -contains "wt.exe") {
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  wt init             # initialize a worktree in the current directory"
    Write-Host "  wt server start     # start the background daemon (auto-snapshots)"
    Write-Host "  wt --help           # everything else"
    if (-not $NoPath) {
        Write-Host ""
        Write-Host "Open a NEW terminal for PATH changes to apply everywhere."
    }
}
if ($binaries -contains "worktree-server.exe") {
    Write-Host ""
    Write-Host "Server: run 'worktree-server' (see docs/server-architecture.md)."
}
