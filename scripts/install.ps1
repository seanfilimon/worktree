<#
.SYNOPSIS
  Install W0rkTree (wt + worktree-bg, optionally worktree-server) on Windows.

.DESCRIPTION
  Downloads the release archive for this machine from GitHub, verifies its
  SHA-256 checksum, and copies the binaries into -InstallDir. With
  -FromSource, builds with cargo (and go for the server) instead.

.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.ps1))) -WithServer
.EXAMPLE
  .\scripts\install.ps1 -Version v0.2.0 -WithServer -AddToPath
.EXAMPLE
  .\scripts\install.ps1 -FromSource
#>
[CmdletBinding()]
param(
    [string]$Version = $(if ($env:WT_VERSION) { $env:WT_VERSION } else { 'latest' }),
    [string]$InstallDir = $(if ($env:WT_INSTALL_DIR) { $env:WT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\worktree\bin' }),
    [string]$Repo = $(if ($env:WT_REPO) { $env:WT_REPO } else { 'seanfilimon/worktree' }),
    [switch]$WithServer,
    [switch]$FromSource,
    # Persistently prepend -InstallDir to the *user* PATH (off by default).
    [switch]$AddToPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is far faster without the progress bar
# Windows PowerShell 5.1 defaults to TLS 1.0/1.1, which GitHub rejects.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Get-Arch {
    $a = $null
    try { $a = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { }
    if (-not $a) {
        $a = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    }
    switch -Regex ($a) {
        '^(X64|AMD64)$' { return 'x86_64' }
        '^(Arm64|ARM64)$' { return 'aarch64' }
        default { throw "unsupported architecture: $a" }
    }
}

function Install-Binary([string]$Source, [string]$Name) {
    New-Item -ItemType Directory -Force $InstallDir | Out-Null
    $dest = Join-Path $InstallDir $Name
    try {
        Copy-Item -Force $Source $dest
    } catch {
        throw "could not write $dest (is it running? stop it with 'wt daemon stop' and retry): $_"
    }
    Write-Host "  installed $dest"
}

$bins = @('wt', 'worktree-bg')
if ($WithServer) { $bins += 'worktree-server' }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("worktree-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

try {
    if (-not $FromSource) {
        $target = "$(Get-Arch)-pc-windows-msvc"
        if ($Version -eq 'latest') {
            try {
                $rel = Invoke-RestMethod -UseBasicParsing -Headers @{ 'User-Agent' = 'worktree-installer' } `
                    -Uri "https://api.github.com/repos/$Repo/releases/latest"
            } catch {
                throw "could not resolve the latest release of $Repo (no published release?): $($_.Exception.Message)"
            }
            $Version = $rel.tag_name
            if (-not $Version) { throw "could not resolve the latest release of $Repo" }
        }
        if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
        $name = "worktree-$($Version.Substring(1))-$target"
        $archive = "$name.zip"
        $base = "https://github.com/$Repo/releases/download/$Version"

        Write-Host "Installing W0rkTree $Version ($target) into $InstallDir"
        $zip = Join-Path $tmp $archive
        $sumFile = "$zip.sha256"
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $zip
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive.sha256" -OutFile $sumFile

        $expected = ((Get-Content -Raw $sumFile).Trim() -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
        if (-not $expected -or $expected -ne $actual) {
            throw "checksum mismatch for $archive (expected $expected, got $actual)"
        }
        Write-Host "  checksum ok ($actual)"

        $extract = Join-Path $tmp 'x'
        Expand-Archive -Path $zip -DestinationPath $extract -Force
        foreach ($b in $bins) {
            $src = Join-Path $extract "$name\$b.exe"
            if (-not (Test-Path $src)) { throw "$b.exe missing from $archive" }
            Install-Binary $src "$b.exe"
        }
    } else {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo not found (install Rust from https://rustup.rs)' }
        $src = $null
        if ($PSScriptRoot) {
            $here = Split-Path -Parent $PSScriptRoot
            if ((Test-Path (Join-Path $here 'Cargo.toml')) -and (Test-Path (Join-Path $here 'crates\worktree-cli'))) { $src = $here }
        }
        if (-not $src) {
            if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git not found (needed to fetch the source)' }
            $src = Join-Path $tmp 'src'
            $cloneArgs = @('clone', '--depth', '1')
            if ($Version -ne 'latest') { $cloneArgs += @('--branch', $Version) }
            Write-Host "Cloning https://github.com/$Repo"
            & git @cloneArgs "https://github.com/$Repo.git" $src
            if ($LASTEXITCODE -ne 0) { throw 'git clone failed' }
        }
        Write-Host "Building W0rkTree from source ($src) into $InstallDir"
        $cargoRoot = Join-Path $tmp 'cargo'
        $locked = @()
        if (Test-Path (Join-Path $src 'Cargo.lock')) { $locked = @('--locked') }
        foreach ($crate in 'worktree-cli', 'worktree-bg') {
            & cargo install @locked --force --root $cargoRoot --path (Join-Path $src "crates\$crate")
            if ($LASTEXITCODE -ne 0) { throw "cargo install $crate failed" }
        }
        Install-Binary (Join-Path $cargoRoot 'bin\wt.exe') 'wt.exe'
        Install-Binary (Join-Path $cargoRoot 'bin\worktree-bg.exe') 'worktree-bg.exe'
        if ($WithServer) {
            if (-not (Get-Command go -ErrorAction SilentlyContinue)) { throw 'go not found (needed for -WithServer -FromSource)' }
            $out = Join-Path $tmp 'worktree-server.exe'
            Push-Location (Join-Path $src 'services\server-go')
            try {
                $env:CGO_ENABLED = '0'
                & go build -trimpath -o $out .
                if ($LASTEXITCODE -ne 0) { throw 'go build failed' }
            } finally { Pop-Location; Remove-Item Env:\CGO_ENABLED -ErrorAction SilentlyContinue }
            Install-Binary $out 'worktree-server.exe'
        }
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------------ PATH
$full = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = @(($env:Path -split ';') + ($userPath -split ';')) |
    Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') } | Where-Object { $_ -ieq $full }
if (-not $onPath) {
    if ($AddToPath) {
        $newPath = if ($userPath) { "$full;$userPath" } else { $full }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$full;$env:Path"
        Write-Host ''
        Write-Host "Added $full to your user PATH (open a new terminal to pick it up)."
    } else {
        Write-Host ''
        Write-Host "$full is not on your PATH. Add it for your user with:"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"$full;`" + [Environment]::GetEnvironmentVariable('Path','User'), 'User')"
        Write-Host '(or re-run this installer with -AddToPath), then open a new terminal.'
    }
}
Write-Host ''
Write-Host 'Done. Try: wt --help    (start the background daemon with: wt daemon start)' -ForegroundColor Green
