<#
.SYNOPSIS
  Install W0rkTree (wt + worktree-bg, optionally worktree-server).

.DESCRIPTION
  Downloads the release archive for this machine (Windows, or Linux/macOS
  under PowerShell 7) from a release base URL, verifies its SHA-256 against
  SHA256SUMS (or the archive's .sha256 file for older releases), and copies
  the binaries into -InstallDir. Nothing is installed when verification
  fails. With -FromSource, builds with cargo (and go for the server) instead.

  -BaseUrl accepts https:// URLs, file:/// URLs and local directories (for
  example the dist\ directory written by scripts/release.sh).

.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.ps1))) -WithServer
.EXAMPLE
  .\scripts\install.ps1 -Version v0.7.0 -WithServer -AddToPath
.EXAMPLE
  .\scripts\install.ps1 -BaseUrl .\dist -InstallDir C:\tools\worktree
.EXAMPLE
  .\scripts\install.ps1 -FromSource
#>
[CmdletBinding()]
param(
    [string]$Version = $(if ($env:WT_VERSION) { $env:WT_VERSION } else { 'latest' }),
    [string]$InstallDir = $(if ($env:WT_INSTALL_DIR) { $env:WT_INSTALL_DIR } else { '' }),
    [string]$BaseUrl = $(if ($env:WT_BASE_URL) { $env:WT_BASE_URL } else { '' }),
    [string]$Target = $(if ($env:WT_TARGET) { $env:WT_TARGET } else { '' }),
    [string]$Repo = $(if ($env:WT_REPO) { $env:WT_REPO } else { 'seanfilimon/worktree' }),
    [switch]$WithServer,
    [switch]$FromSource,
    # Persistently prepend -InstallDir to the *user* PATH (Windows only, off by default).
    [switch]$AddToPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is far faster without the progress bar
# Windows PowerShell 5.1 defaults to TLS 1.0/1.1, which GitHub rejects.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

# $IsWindows only exists in PowerShell 6+; 5.1 always runs on Windows.
$onWindows = $true
if (Get-Variable -Name IsWindows -ErrorAction SilentlyContinue) { $onWindows = [bool]$IsWindows }
$onMac = $false
if (Get-Variable -Name IsMacOS -ErrorAction SilentlyContinue) { $onMac = [bool]$IsMacOS }

if (-not $InstallDir) {
    if ($onWindows) {
        $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\worktree\bin'
    } else {
        $InstallDir = Join-Path $HOME '.local/bin'
    }
}

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

function Get-DefaultTarget {
    $arch = Get-Arch
    if ($onWindows) { return "$arch-pc-windows-msvc" }
    if ($onMac) { return "$arch-apple-darwin" }
    return "$arch-unknown-linux-gnu"
}

# Returns the local filesystem path for file:// URLs and plain paths, or
# $null for http(s) URLs.
function Get-LocalPath([string]$Url) {
    if ($Url -match '^https?://') { return $null }
    if ($Url -match '^file://') {
        try { return ([Uri]$Url).LocalPath } catch { throw "invalid file URL: $Url" }
    }
    return $Url
}

function Join-Url([string]$Base, [string]$Name) {
    if (Get-LocalPath $Base) { return (Join-Path (Get-LocalPath $Base) $Name) }
    return "$($Base.TrimEnd('/'))/$Name"
}

# Downloads (or copies) $Url to $Dest; returns $false when it does not exist.
function Get-File([string]$Url, [string]$Dest) {
    $local = Get-LocalPath $Url
    if ($local) {
        if (-not (Test-Path -LiteralPath $local -PathType Leaf)) { return $false }
        Copy-Item -LiteralPath $local -Destination $Dest -Force
        return $true
    }
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Dest -Headers @{ 'User-Agent' = 'worktree-installer' }
        return $true
    } catch {
        Write-Verbose "download failed: $Url ($($_.Exception.Message))"
        return $false
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
    if (-not $onWindows) { & chmod 0755 $dest }
    Write-Host "  installed $dest"
}

$bins = @('wt', 'worktree-bg')
if ($WithServer) { $bins += 'worktree-server' }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("worktree-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

try {
    if (-not $FromSource) {
        if (-not $Target) { $Target = Get-DefaultTarget }
        $archiveExt = if ($Target -like '*-windows-*') { 'zip' } else { 'tar.gz' }
        $binExt = if ($Target -like '*-windows-*') { '.exe' } else { '' }

        if ($BaseUrl) {
            $base = $BaseUrl
        } elseif ($Version -eq 'latest') {
            $base = "https://github.com/$Repo/releases/latest/download"
        } else {
            if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
            $base = "https://github.com/$Repo/releases/download/$Version"
        }
        if ($base -match '^http://') {
            Write-Warning "$base is plain HTTP; the checksum only detects corruption, not tampering"
        }
        $wantVersion = $Version.TrimStart('v')

        # Pick the archive from SHA256SUMS (works for "latest" and for mirrors).
        $archive = $null
        $expected = $null
        $sums = Join-Path $tmp 'SHA256SUMS'
        if (Get-File (Join-Url $base 'SHA256SUMS') $sums) {
            foreach ($line in Get-Content -LiteralPath $sums) {
                $parts = $line.Trim() -split '\s+'
                if ($parts.Count -lt 2) { continue }
                $name = $parts[-1].TrimStart('*')
                if (-not $name.EndsWith("-$Target.$archiveExt") -or -not $name.StartsWith('worktree-')) { continue }
                if ($Version -ne 'latest' -and $name -ne "worktree-$wantVersion-$Target.$archiveExt") { continue }
                $archive = $name
                $expected = $parts[0].ToLowerInvariant()
            }
            if (-not $archive) { throw "no $Target archive listed in $(Join-Url $base 'SHA256SUMS')" }
        } else {
            # Older releases publish only per-archive .sha256 files.
            if ($Version -eq 'latest') { throw "could not fetch $(Join-Url $base 'SHA256SUMS') (pass -Version to use per-archive checksums)" }
            $archive = "worktree-$wantVersion-$Target.$archiveExt"
            $sumFile = Join-Path $tmp "$archive.sha256"
            if (-not (Get-File (Join-Url $base "$archive.sha256") $sumFile)) {
                throw "could not fetch $(Join-Url $base 'SHA256SUMS') or $(Join-Url $base "$archive.sha256")"
            }
            $expected = ((Get-Content -Raw -LiteralPath $sumFile).Trim() -split '\s+')[0].ToLowerInvariant()
        }

        $name = $archive.Substring(0, $archive.Length - $archiveExt.Length - 1)
        $shown = $name.Substring('worktree-'.Length)
        $shown = $shown.Substring(0, $shown.Length - $Target.Length - 1)
        Write-Host "Installing W0rkTree $shown ($Target) into $InstallDir"

        $file = Join-Path $tmp $archive
        if (-not (Get-File (Join-Url $base $archive) $file)) { throw "download failed: $(Join-Url $base $archive)" }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
        if (-not $expected -or $expected -ne $actual) {
            throw "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
        }
        Write-Host "  checksum ok ($actual)"

        $extract = Join-Path $tmp 'x'
        New-Item -ItemType Directory -Force $extract | Out-Null
        if ($archiveExt -eq 'zip') {
            Expand-Archive -LiteralPath $file -DestinationPath $extract -Force
        } else {
            & tar -xzf $file -C $extract
            if ($LASTEXITCODE -ne 0) { throw "could not extract $archive" }
        }
        foreach ($b in $bins) {
            $src = Join-Path (Join-Path $extract $name) "$b$binExt"
            if (-not (Test-Path -LiteralPath $src)) { throw "$b$binExt missing from $archive" }
        }
        foreach ($b in $bins) {
            Install-Binary (Join-Path (Join-Path $extract $name) "$b$binExt") "$b$binExt"
        }
    } else {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo not found (install Rust from https://rustup.rs)' }
        $exe = if ($onWindows) { '.exe' } else { '' }
        $src = $null
        if ($PSScriptRoot) {
            $here = Split-Path -Parent $PSScriptRoot
            if ((Test-Path (Join-Path $here 'Cargo.toml')) -and (Test-Path (Join-Path $here 'crates/worktree-cli'))) { $src = $here }
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
            & cargo install @locked --force --root $cargoRoot --path (Join-Path (Join-Path $src 'crates') $crate)
            if ($LASTEXITCODE -ne 0) { throw "cargo install $crate failed" }
        }
        Install-Binary (Join-Path (Join-Path $cargoRoot 'bin') "wt$exe") "wt$exe"
        Install-Binary (Join-Path (Join-Path $cargoRoot 'bin') "worktree-bg$exe") "worktree-bg$exe"
        if ($WithServer) {
            if (-not (Get-Command go -ErrorAction SilentlyContinue)) { throw 'go not found (needed for -WithServer -FromSource)' }
            $out = Join-Path $tmp "worktree-server$exe"
            Push-Location (Join-Path (Join-Path $src 'services') 'server-go')
            try {
                $env:CGO_ENABLED = '0'
                & go build -trimpath -o $out .
                if ($LASTEXITCODE -ne 0) { throw 'go build failed' }
            } finally { Pop-Location; Remove-Item Env:\CGO_ENABLED -ErrorAction SilentlyContinue }
            Install-Binary $out "worktree-server$exe"
        }
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------------ PATH
$sep = [IO.Path]::PathSeparator
$full = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\', '/')
$userPath = if ($onWindows) { [Environment]::GetEnvironmentVariable('Path', 'User') } else { '' }
$onPath = @(($env:PATH -split [regex]::Escape($sep)) + ($userPath -split [regex]::Escape($sep))) |
    Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\', '/') } | Where-Object { $_ -ieq $full }
if (-not $onPath) {
    Write-Host ''
    if ($onWindows -and $AddToPath) {
        $newPath = if ($userPath) { "$full;$userPath" } else { $full }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$full;$env:Path"
        Write-Host "Added $full to your user PATH (open a new terminal to pick it up)."
    } elseif ($onWindows) {
        Write-Host "$full is not on your PATH. Add it for your user with:"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"$full;`" + [Environment]::GetEnvironmentVariable('Path','User'), 'User')"
        Write-Host '(or re-run this installer with -AddToPath), then open a new terminal.'
    } else {
        Write-Host "$full is not on your PATH. Add it to your shell profile:"
        Write-Host "  export PATH=`"$full`:`$PATH`""
    }
}
Write-Host ''
Write-Host 'Done. Try: wt --help    (start the background daemon with: wt daemon start)' -ForegroundColor Green
