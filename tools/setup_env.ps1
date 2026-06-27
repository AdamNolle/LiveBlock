<#
.SYNOPSIS
  Create a Python venv at tools\.venv and install training/export deps (Windows).
  Idempotent — safe to re-run. Mirrors tools/setup_env.sh.

.USAGE
  pwsh -File tools/setup_env.ps1
  $env:PYTHON = 'C:\Python311\python.exe'; pwsh -File tools/setup_env.ps1   # pin interpreter
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$ToolsDir = Split-Path -Parent $PSCommandPath
Set-Location $ToolsDir

# Pick an interpreter: $env:PYTHON override, else `py -3`, else `python`.
$PyExe = $null
if ($env:PYTHON) {
    $PyExe = $env:PYTHON
} elseif (Get-Command py -ErrorAction SilentlyContinue) {
    $PyExe = 'py'
    $PyArgsPrefix = @('-3')
} elseif (Get-Command python -ErrorAction SilentlyContinue) {
    $PyExe = 'python'
} else {
    Write-Error "Python 3 not found. Install Python 3.10-3.12 from python.org (check 'Add to PATH')."
    exit 1
}
if (-not $PyArgsPrefix) { $PyArgsPrefix = @() }

# Verify version is in the supported range (3.10-3.12 for coremltools/ultralytics).
$verRaw = & $PyExe @PyArgsPrefix -c "import sys; print(f'{sys.version_info[0]}.{sys.version_info[1]}')"
$ver = $verRaw.Trim()
if ($ver -notin @('3.10', '3.11', '3.12')) {
    Write-Warning "Python $ver detected. ultralytics wants 3.10-3.12 (coremltools is mac-only and skipped on Windows). Override with `$env:PYTHON`."
}

$VenvDir = Join-Path $ToolsDir '.venv'
if (-not (Test-Path $VenvDir)) {
    Write-Host "-> Creating venv at tools\.venv (using $PyExe $($PyArgsPrefix -join ' ')) ..."
    & $PyExe @PyArgsPrefix -m venv $VenvDir
}

$VenvPy = Join-Path $VenvDir 'Scripts\python.exe'
if (-not (Test-Path $VenvPy)) {
    Write-Error "venv python not found at $VenvPy after creation."
    exit 1
}

Write-Host "-> Upgrading pip ..."
& $VenvPy -m pip install --upgrade pip wheel | Out-Null

# coremltools is macOS-only; on Windows install everything EXCEPT coremltools so
# pip doesn't choke. Use the Windows-specific requirements file.
$reqWin = Join-Path $ToolsDir 'requirements-win.txt'
$req = if (Test-Path $reqWin) { $reqWin } else { Join-Path $ToolsDir 'requirements.txt' }
Write-Host "-> Installing requirements from $([System.IO.Path]::GetFileName($req)) (a couple of minutes) ..."
& $VenvPy -m pip install -r $req

Write-Host ""
Write-Host "OK Done. Verify:"
Write-Host "    tools\.venv\Scripts\python.exe -c `"import ultralytics; print('ok')`""
