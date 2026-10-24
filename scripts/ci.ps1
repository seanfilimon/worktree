<#
.SYNOPSIS
  Local equivalent of .github/workflows/ci.yml for Windows PowerShell 5.1+ / PowerShell 7.

.EXAMPLE
  .\scripts\ci.ps1                    # rust, go, web, admin (fail fast)
  .\scripts\ci.ps1 -Rust -Go          # only the selected parts
  .\scripts\ci.ps1 -NoWeb             # everything except web
  .\scripts\ci.ps1 -All -KeepGoing    # include e2e, report all failures at the end
#>
[CmdletBinding()]
param(
    [switch]$Rust,
    [switch]$Go,
    [switch]$Web,
    [switch]$Admin,
    [switch]$E2E,
    [switch]$All,
    [switch]$NoRust,
    [switch]$NoGo,
    [switch]$NoWeb,
    [switch]$NoAdmin,
    [switch]$NoE2E,
    [Alias('k')][switch]$KeepGoing
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$ServerDir = Join-Path $Root 'services\server-go'
Set-Location $Root

$anySelected = $Rust -or $Go -or $Web -or $Admin -or $E2E -or $All
$parts = @{
    rust  = ($All -or $Rust  -or -not $anySelected) -and -not $NoRust
    go    = ($All -or $Go    -or -not $anySelected) -and -not $NoGo
    web   = ($All -or $Web   -or -not $anySelected) -and -not $NoWeb
    admin = ($All -or $Admin -or -not $anySelected) -and -not $NoAdmin
    e2e   = ($All -or $E2E) -and -not $NoE2E
}

$script:Passed = New-Object System.Collections.Generic.List[string]
$script:Failed = New-Object System.Collections.Generic.List[string]
$script:Skipped = New-Object System.Collections.Generic.List[string]

function Show-Summary {
    Write-Host ''
    Write-Host '==> Summary' -ForegroundColor White
    foreach ($s in $script:Passed)  { Write-Host "  [ok]   $s" -ForegroundColor Green }
    foreach ($s in $script:Skipped) { Write-Host "  [skip] $s" -ForegroundColor Yellow }
    foreach ($s in $script:Failed)  { Write-Host "  [FAIL] $s" -ForegroundColor Red }
}

# Invoke-Step NAME { script }: the block fails if it throws or a native command
# leaves a non-zero $LASTEXITCODE.
function Invoke-Step {
    param([string]$Name, [scriptblock]$Block, [switch]$Advisory)
    Write-Host ''
    Write-Host "==> $Name" -ForegroundColor White
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $ok = $true
    # Native tools (cargo, go, npm) write progress to stderr; under Windows
    # PowerShell 5.1 with redirected output that would become a terminating
    # error with 'Stop'. Exit codes decide success instead.
    $ErrorActionPreference = 'Continue'
    try {
        $global:LASTEXITCODE = 0
        & $Block
        if ($LASTEXITCODE -ne 0) { $ok = $false }
    } catch {
        Write-Host $_ -ForegroundColor Red
        $ok = $false
    }
    $secs = [int]$sw.Elapsed.TotalSeconds
    if ($ok) {
        Write-Host "[ok] $Name (${secs}s)" -ForegroundColor Green
        $script:Passed.Add($Name)
    } elseif ($Advisory) {
        Write-Host "[warn] $Name failed (advisory; not counted)" -ForegroundColor Yellow
        $script:Skipped.Add("$Name (advisory failure)")
    } else {
        Write-Host "[FAIL] $Name (${secs}s)" -ForegroundColor Red
        $script:Failed.Add($Name)
        if (-not $KeepGoing) { Show-Summary; exit 1 }
    }
}

function Skip-Step([string]$Name, [string]$Reason) {
    Write-Host ''
    Write-Host "[skip] ${Name}: $Reason" -ForegroundColor Yellow
    $script:Skipped.Add("$Name ($Reason)")
}

function Test-Command([string]$Name) { [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Invoke-InDir([string]$Dir, [scriptblock]$Block) {
    Push-Location $Dir
    try { & $Block } finally { Pop-Location }
}

function Test-ProtocPlugins {
    $gobin = Join-Path (& go env GOPATH) 'bin'
    foreach ($p in 'protoc-gen-go', 'protoc-gen-go-grpc') {
        if (-not (Test-Command $p) -and -not (Test-Path (Join-Path $gobin "$p.exe"))) { return $false }
    }
    return $true
}

function ConvertTo-PosixPath([string]$Path) { $Path -replace '\\', '/' }

# Git Bash, not the WSL launcher (System32 / WindowsApps bash.exe) that often
# comes first on PATH.
function Find-Bash {
    if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
        $b = Get-Command bash -ErrorAction SilentlyContinue
        if ($b) { return $b.Source } else { return $null }
    }
    $candidates = @()
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd) { $candidates += Join-Path (Split-Path (Split-Path $gitCmd.Source)) 'bin\bash.exe' }
    $candidates += (Join-Path $env:ProgramFiles 'Git\bin\bash.exe')
    if ($env:LOCALAPPDATA) { $candidates += (Join-Path $env:LOCALAPPDATA 'Programs\Git\bin\bash.exe') }
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    $b = Get-Command bash -All -ErrorAction SilentlyContinue |
        Where-Object { $_.Source -notmatch '\\(System32|WindowsApps)\\' } | Select-Object -First 1
    if ($b) { return $b.Source }
    return $null
}
$script:Bash = Find-Bash

# "relative-path SHA256" lines for every file under $Dir.
function Get-TreeHashes([string]$Dir) {
    Push-Location $Dir
    try {
        Get-ChildItem -Recurse -File | ForEach-Object {
            '{0} {1}' -f (Resolve-Path -Relative $_.FullName), (Get-FileHash -Algorithm SHA256 $_.FullName).Hash
        }
    } finally { Pop-Location }
}

# Regenerate Go bindings (via scripts/gen-proto.sh in Git Bash), compare with the
# working tree, then restore the original files so the check has no side effects.
function Test-ProtoDrift {
    $dir = Join-Path $ServerDir 'internal\proto'
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("wt-proto-" + [guid]::NewGuid())
    Copy-Item -Recurse $dir $tmp
    try {
        & $script:Bash (ConvertTo-PosixPath (Join-Path $Root 'scripts/gen-proto.sh'))
        if ($LASTEXITCODE -ne 0) { throw 'gen-proto.sh failed' }
        $diff = Compare-Object @(Get-TreeHashes $tmp) @(Get-TreeHashes $dir)
        if ($diff) {
            $diff | Format-Table -AutoSize | Out-String | Write-Host
            throw "generated code differs from services/server-go/internal/proto; run 'bash scripts/gen-proto.sh' and commit"
        }
    } finally {
        Remove-Item -Recurse -Force $dir
        Copy-Item -Recurse $tmp $dir
        Remove-Item -Recurse -Force $tmp
    }
}

function Test-Workspace([string]$Name) { Test-Path (Join-Path $Root "apps\$Name\package.json") }

# ------------------------------------------------------------------ rust
if ($parts.rust) {
    if (Test-Command cargo) {
        Invoke-Step 'rust: fmt'    { cargo fmt --all -- --check }
        Invoke-Step 'rust: clippy' { cargo clippy --workspace --all-targets -- -D warnings }
        Invoke-Step 'rust: test'   { cargo test --workspace }
    } else { Skip-Step 'rust' 'cargo not found' }
}

# ------------------------------------------------------------------ go
if ($parts.go) {
    if (Test-Command go) {
        Invoke-Step 'go: gofmt' {
            $out = Invoke-InDir $ServerDir { & gofmt -l . }
            if ($LASTEXITCODE -ne 0) { throw 'gofmt failed' }
            if ($out) { Write-Host "gofmt needed on:`n$($out -join "`n")"; throw 'gofmt check failed' }
        }
        Invoke-Step 'go: vet'   { Invoke-InDir $ServerDir { go vet ./... } }
        Invoke-Step 'go: build' { Invoke-InDir $ServerDir { go build ./... } }
        Invoke-Step 'go: test'  { Invoke-InDir $ServerDir { go test -count=1 ./... } }
        if ($script:Bash -and (Test-ProtocPlugins)) {
            Invoke-Step 'go: proto drift' { Test-ProtoDrift }
        } else { Skip-Step 'go: proto drift' 'needs Git Bash + protoc-gen-go / protoc-gen-go-grpc' }
        if (Test-Command golangci-lint) {
            Invoke-Step 'go: golangci-lint' -Advisory { Invoke-InDir $ServerDir { golangci-lint run ./... } }
        } else { Skip-Step 'go: golangci-lint' 'not installed' }
    } else { Skip-Step 'go' 'go not found' }
}

# ------------------------------------------------------------------ web / admin
if (($parts.web -or $parts.admin) -and (Test-Command npm) -and -not (Test-Path (Join-Path $Root 'node_modules'))) {
    Invoke-Step 'npm ci' { npm ci --no-audit --no-fund }
}

if ($parts.web) {
    if (-not (Test-Command npm)) { Skip-Step 'web' 'npm not found' }
    elseif (-not (Test-Workspace 'web')) { Skip-Step 'web' 'apps/web missing' }
    else {
        Invoke-Step 'web: build' { npm run build --workspace=apps/web }
        # Advisory until apps/web replaces `next lint` (removed in Next.js 16).
        Invoke-Step 'web: lint' -Advisory { npm run lint --workspace=apps/web }
    }
}

if ($parts.admin) {
    if (-not (Test-Command npm)) { Skip-Step 'admin' 'npm not found' }
    elseif (-not (Test-Workspace 'admin')) { Skip-Step 'admin' 'apps/admin missing' }
    else {
        Invoke-Step 'admin: lint'      { npm run lint --workspace=apps/admin --if-present }
        Invoke-Step 'admin: typecheck' { npm run typecheck --workspace=apps/admin --if-present }
        Invoke-Step 'admin: test'      { npm run test --workspace=apps/admin --if-present }
        Invoke-Step 'admin: build'     { npm run build --workspace=apps/admin }
    }
}

# ------------------------------------------------------------------ e2e
if ($parts.e2e) {
    $e2e = Join-Path $Root 'scripts\e2e.ps1'
    if (-not (Test-Path $e2e)) { $e2e = Join-Path $Root 'scripts\e2e.sh' }
    if (-not (Test-Path $e2e)) { Skip-Step 'e2e' 'scripts/e2e.* missing' }
    else {
        Invoke-Step 'e2e: build rust' { cargo build --workspace --bins }
        Invoke-Step 'e2e: build go' {
            $out = Join-Path $Root 'target\go\worktree-server.exe'
            Invoke-InDir $ServerDir { go build -trimpath -o $out . }
        }
        if ($e2e.EndsWith('.ps1')) { Invoke-Step 'e2e: run' { & $e2e } }
        else { Invoke-Step 'e2e: run' { & $script:Bash (ConvertTo-PosixPath $e2e) } }
    }
}

Show-Summary
if ($script:Failed.Count -gt 0) { exit 1 }
exit 0
