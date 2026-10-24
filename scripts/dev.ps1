<#
.SYNOPSIS
  Local development: Go server on 127.0.0.1:7420 + admin panel Vite dev server
  on http://localhost:5173 (proxies /api and /healthz to the server).

.EXAMPLE
  .\scripts\dev.ps1
  .\scripts\dev.ps1 -ServerOnly
  .\scripts\dev.ps1 -AdminOnly

.NOTES
  Env: WT_ADDR (default 127.0.0.1:7420), WT_DATA (default .\wt-data),
  WT_ADMIN_USER (default admin), WT_ADMIN_PASSWORD (default "admin" for local
  dev only), WT_LOG_LEVEL (default debug).
#>
[CmdletBinding()]
param(
    [switch]$ServerOnly,
    [switch]$AdminOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$runServer = -not $AdminOnly
$runAdmin = -not $ServerOnly
$isWin = [Environment]::OSVersion.Platform -eq 'Win32NT'
$exe = if ($isWin) { '.exe' } else { '' }

if (-not $env:WT_ADDR) { $env:WT_ADDR = '127.0.0.1:7420' }
if (-not $env:WT_DATA) { $env:WT_DATA = Join-Path $Root 'wt-data' }
if (-not $env:WT_ADMIN_USER) { $env:WT_ADMIN_USER = 'admin' }
if (-not $env:WT_ADMIN_PASSWORD) {
    $env:WT_ADMIN_PASSWORD = 'admin'
    Write-Warning "WT_ADMIN_PASSWORD not set; using the local-dev default 'admin'"
}
$logLevel = if ($env:WT_LOG_LEVEL) { $env:WT_LOG_LEVEL } else { 'debug' }

function Stop-Tree([System.Diagnostics.Process]$Proc) {
    if ($null -eq $Proc -or $Proc.HasExited) { return }
    if ($isWin) {
        & taskkill.exe /F /T /PID $Proc.Id 2>&1 | Out-Null
    } else {
        & kill -TERM $Proc.Id 2>&1 | Out-Null
    }
}

$procs = New-Object System.Collections.Generic.List[System.Diagnostics.Process]
function Add-Child([System.Diagnostics.Process]$Proc) {
    $null = $Proc.Handle   # cache the handle so ExitCode is available after exit
    $procs.Add($Proc)
}

try {
    if ($runServer) {
        $bin = Join-Path $Root "target\go\worktree-server$exe"
        Write-Host "==> building worktree-server -> $bin"
        Push-Location (Join-Path $Root 'services\server-go')
        try {
            & go build -o $bin .
            if ($LASTEXITCODE -ne 0) { throw "go build failed ($LASTEXITCODE)" }
        } finally { Pop-Location }

        New-Item -ItemType Directory -Force $env:WT_DATA | Out-Null
        $serverArgs = @('--addr', $env:WT_ADDR, '--data', "`"$env:WT_DATA`"", '--log-level', $logLevel)
        $adminDir = Join-Path $Root 'apps\admin\dist'
        if (Test-Path (Join-Path $adminDir 'index.html')) { $serverArgs += @('--admin-dir', "`"$adminDir`"") }
        Write-Host "==> server on http://$($env:WT_ADDR) (data: $($env:WT_DATA))"
        Add-Child (Start-Process -FilePath $bin -ArgumentList $serverArgs -NoNewWindow -PassThru)
    }

    if ($runAdmin) {
        if (-not (Test-Path (Join-Path $Root 'apps\admin\package.json'))) {
            Write-Warning 'apps/admin not found; running server only'
        } else {
            if (-not (Test-Path (Join-Path $Root 'node_modules'))) {
                Push-Location $Root
                try {
                    & npm ci --no-audit --no-fund
                    if ($LASTEXITCODE -ne 0) { throw "npm ci failed ($LASTEXITCODE)" }
                } finally { Pop-Location }
            }
            Write-Host '==> admin dev server on http://localhost:5173'
            $npm = if ($isWin) { 'npm.cmd' } else { 'npm' }
            Add-Child (Start-Process -FilePath $npm -ArgumentList @('run', 'dev', '--workspace=apps/admin') `
                -WorkingDirectory $Root -NoNewWindow -PassThru)
        }
    }

    if ($procs.Count -eq 0) { throw 'nothing to run' }
    Write-Host '==> running; press Ctrl+C to stop'

    # Exit as soon as any process stops; Ctrl+C triggers the finally block.
    while ($true) {
        foreach ($p in $procs) {
            if ($p.HasExited) {
                Write-Warning "process $($p.Id) exited with status $($p.ExitCode); shutting down"
                exit $p.ExitCode
            }
        }
        Start-Sleep -Milliseconds 500
    }
} finally {
    foreach ($p in $procs) { Stop-Tree $p }
}
