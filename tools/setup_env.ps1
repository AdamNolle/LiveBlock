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

# Pick a supported interpreter: explicit override, then installed 3.12/3.11/3.10.
$PyExe = $null
if ($env:PYTHON) {
    $PyExe = $env:PYTHON
} elseif (Get-Command py -ErrorAction SilentlyContinue) {
    $PyExe = 'py'
    foreach ($candidate in @('3.12', '3.11', '3.10')) {
        & py "-$candidate" -c 'import sys' 2>$null
        if ($LASTEXITCODE -eq 0) {
            $PyArgsPrefix = @("-$candidate")
            break
        }
    }
    if (-not $PyArgsPrefix -and (Get-Command uv -ErrorAction SilentlyContinue)) {
        foreach ($candidate in @('3.12', '3.11', '3.10')) {
            $found = & uv python find $candidate 2>$null
            if ($LASTEXITCODE -eq 0 -and (Test-Path $found)) {
                $PyExe = $found
                $PyArgsPrefix = @()
                break
            }
        }
    }
    if ($PyExe -eq 'py' -and -not $PyArgsPrefix) {
        throw 'Python 3.10-3.12 is required for the training dependencies.'
    }
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
    throw "Python $ver is unsupported by the training dependencies. Set `$env:PYTHON to Python 3.10-3.12."
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
if ($LASTEXITCODE -ne 0) { throw "Training requirements installation failed." }

# PyPI's Windows PyTorch wheel is often CPU-only. Use the CUDA 12.8 wheel on
# NVIDIA hosts, including RTX 50-series, so an in-app training run can use GPU.
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
    $torchCuda = & $VenvPy -c "import torch; print(torch.version.cuda or '')"
    if (-not $torchCuda.Trim()) {
        Write-Host "-> NVIDIA GPU found; installing CUDA-enabled PyTorch ..."
        & $VenvPy -m pip install --force-reinstall 'torch==2.11.0+cu128' 'torchvision==0.26.0+cu128' `
            --index-url 'https://download.pytorch.org/whl/cu128'
        if ($LASTEXITCODE -ne 0) { throw "CUDA PyTorch installation failed." }
        & $VenvPy -m pip install 'numpy>=1.26,<2.0'
        if ($LASTEXITCODE -ne 0) { throw "NumPy compatibility installation failed." }
    }
}

Write-Host ""
Write-Host "OK Done. Verify:"
Write-Host "    tools\.venv\Scripts\python.exe -c `"import ultralytics; print('ok')`""
