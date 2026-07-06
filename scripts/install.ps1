<#
.SYNOPSIS
  W0rkTree installer for Windows.

.DESCRIPTION
  Installs W0rkTree binaries to a user-level bin directory and adds it to
  the user PATH. Two modes:

    build (default)  — compile from source with cargo (requires Rust)
    -FromRelease     — download prebuilt binaries from GitHub Releases
                       (SHA-256 verified; -ReleaseTag pins a version,
                       otherwise the latest release is used)

  Components:
    cli     -> wt.exe + worktree-bg.exe   (the CLI and its background daemon)
    server  -> worktree-server.exe        (the remote multi-tenant server)
    all     -> everything (default)

.EXAMPLE
  .\scripts\install.ps1                        # build + install everything
  .\scripts\install.ps1 -FromRelease           # prebuilt binaries, no Rust needed
  .\scripts\install.ps1 -FromRelease -ReleaseTag v0.1.0-alpha.1
  .\scripts\install.ps1 -Component cli         # just the CLI + daemon
  .\scripts\install.ps1 -Uninstall             # remove binaries and PATH entry
#>
[CmdletBinding()]
param(
    [ValidateSet("all", "cli", "server")]
    [string]$Component = "all",

    # Where binaries are installed.
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\W0rkTree\bin",

    # Download prebuilt binaries from GitHub Releases instead of building.
    [switch]$FromRelease,

    # Release tag to download (default: latest release).
    [string]$ReleaseTag = "",

    # Skip modifying the user PATH.
    [switch]$NoPath,

    # Remove installed binaries and the PATH entry.
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$repo = "seanfilimon/worktree"
$releaseTarget = "x86_64-pc-windows-msvc"

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
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

if ($FromRelease) {
    $asset = "w0rktree-$releaseTarget.zip"
    if ($ReleaseTag) {
        $url = "https://github.com/$repo/releases/download/$ReleaseTag/$asset"
    } else {
        $url = "https://github.com/$repo/releases/latest/download/$asset"
    }

    $tmp = Join-Path $env:TEMP "w0rktree-install-$(Get-Random)"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        Write-Step "Downloading $url"
        Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset)
        Invoke-WebRequest -Uri "$url.sha256" -OutFile (Join-Path $tmp "$asset.sha256")

        Write-Step "Verifying checksum"
        $expected = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw) -split "\s+")[0].ToLower()
        $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) {
            throw "checksum mismatch for ${asset}: expected $expected, got $actual"
        }

        Write-Step "Installing to $InstallDir"
        Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
        foreach ($bin in $binaries) {
            $source = Join-Path $tmp $bin
            if (-not (Test-Path $source)) { throw "release archive is missing $bin" }
            Copy-Item -Force $source (Join-Path $InstallDir $bin)
            Write-Host "   installed $bin"
        }
    }
    finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
} else {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "cargo not found. Install Rust (https://rustup.rs) or use -FromRelease." -ForegroundColor Red
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
    foreach ($bin in $binaries) {
        $source = Join-Path $repoRoot "target\release\$bin"
        if (-not (Test-Path $source)) { throw "expected binary missing: $source" }
        Copy-Item -Force $source (Join-Path $InstallDir $bin)
        Write-Host "   installed $bin"
    }
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
