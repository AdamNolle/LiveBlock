<#
.SYNOPSIS
  Hands-off LiveBlock training on Windows: backgrounded, auto-export, auto-install,
  Tauri-friendly progress on stdout, toast notification on completion.

  Mirrors tools/auto.sh (macOS). The native Windows app
  (platform/windows/src-tauri/src/training.rs) shells out to this script with
  `--epochs/--batch/--imgsz` and NO data.yaml; in that mode we run
  export_labels.py first and train on the freshest export. You can also pass an
  explicit data.yaml as the first positional argument.

.USAGE
  pwsh -File tools/auto.ps1                              # auto-export labels then train
  pwsh -File tools/auto.ps1 path\to\data.yaml            # train a specific dataset
  pwsh -File tools/auto.ps1 --epochs 30 --batch 8        # forwarded to train_logos.py
  pwsh -File tools/auto.ps1 status                       # check the running job
  pwsh -File tools/auto.ps1 log                          # tail the log
  pwsh -File tools/auto.ps1 stop                         # cancel the running job

  What it does after you start it:
    1. Sets up tools/.venv if missing (via setup_env.ps1)
    2. Exports labels -> YOLO dataset if no data.yaml was given
    3. Trains (GPU auto: CUDA if available, else CPU)
    4. Exports CoreML + ONNX and installs the ONNX into the Windows runtime
    5. Sends a Windows toast notification on success/failure

  NOTE: When invoked by the app, stdout is parsed for ultralytics epoch lines, so
  this script runs the training pipeline in the FOREGROUND of the spawned process
  (it does not self-detach) unless you use the `status/log/stop` subcommands.
  When run by a human with a data.yaml/empty args, it ALSO runs in the foreground
  and streams to the log; close the window only if you started it detached.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)]
    [string[]] $Argv
)

$ErrorActionPreference = 'Stop'

# --- Locate repo + set up well-known paths -------------------------------- #
$ToolsDir = Split-Path -Parent $PSCommandPath
$Repo     = Split-Path -Parent $ToolsDir
$RunsDir  = Join-Path $ToolsDir 'runs'
$LogFile  = Join-Path $RunsDir 'auto.log'
$PidFile  = Join-Path $RunsDir 'auto.pid'

New-Item -ItemType Directory -Force -Path $RunsDir | Out-Null

# venv python (created by setup_env.ps1). On Windows the venv layout is Scripts\.
$VenvPy = Join-Path $ToolsDir '.venv\Scripts\python.exe'

function Write-Note([string]$Msg) {
    $stamp = (Get-Date).ToString('HH:mm:ss')
    Write-Host "[$stamp] $Msg"
}

function Send-Toast([string]$Title, [string]$Body) {
    # Best-effort Windows toast via BurntToast if present; else a balloon tip;
    # else just log. Never fail the run because notification plumbing is missing.
    try {
        if (Get-Module -ListAvailable -Name BurntToast) {
            Import-Module BurntToast -ErrorAction Stop
            New-BurntToastNotification -Text $Title, $Body | Out-Null
            return
        }
    } catch { }
    try {
        Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
        $ni = New-Object System.Windows.Forms.NotifyIcon
        $ni.Icon = [System.Drawing.SystemIcons]::Information
        $ni.BalloonTipTitle = $Title
        $ni.BalloonTipText = $Body
        $ni.Visible = $true
        $ni.ShowBalloonTip(8000)
        Start-Sleep -Milliseconds 200
        $ni.Dispose()
    } catch {
        Write-Note "NOTIFY: $Title - $Body"
    }
}

# --- Subcommands ---------------------------------------------------------- #
$first = if ($Argv -and $Argv.Count -ge 1) { $Argv[0] } else { '' }

switch ($first) {
    'status' {
        if ((Test-Path $PidFile)) {
            $jobPid = (Get-Content $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
            $proc = Get-Process -Id $jobPid -ErrorAction SilentlyContinue
            if ($proc) {
                Write-Host "OK Running (PID $jobPid). Log: $LogFile"
                if (Test-Path $LogFile) {
                    Write-Host "  Last lines:"
                    Get-Content $LogFile -Tail 5 | ForEach-Object { "    $_" }
                }
                exit 0
            }
        }
        Write-Host "x No active training job."
        if (Test-Path $LogFile) { Write-Host ("  Last log: " + (Get-Content $LogFile -Tail 1)) }
        exit 0
    }
    'log' {
        if (-not (Test-Path $LogFile)) { Write-Host "No log yet at $LogFile"; exit 1 }
        Get-Content $LogFile -Wait -Tail 40
        exit 0
    }
    'stop' {
        if (Test-Path $PidFile) {
            $jobPid = (Get-Content $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
            $proc = Get-Process -Id $jobPid -ErrorAction SilentlyContinue
            if ($proc) {
                # Kill the process tree (python child of pwsh).
                try { Stop-Process -Id $jobPid -Force -ErrorAction Stop; Write-Host "OK Stopped PID $jobPid" }
                catch { Write-Host "Failed to stop PID ${jobPid}: $_" }
            } else {
                Write-Host "Nothing to stop (stale pid)."
            }
            Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
        } else {
            Write-Host "Nothing to stop."
        }
        exit 0
    }
    { $_ -in @('-h', '--help', 'help') } {
        Get-Help $PSCommandPath -Detailed
        exit 0
    }
}

# --- Guard against concurrent runs ---------------------------------------- #
if (Test-Path $PidFile) {
    $jobPid = (Get-Content $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
    if (Get-Process -Id $jobPid -ErrorAction SilentlyContinue) {
        Write-Host "Another job is already running (PID $jobPid)."
        Write-Host "Run 'pwsh -File tools/auto.ps1 stop' first, or 'status' to check."
        exit 3
    }
}

# Record our own PID so status/stop can find us. The app parses our stdout, so we
# run the pipeline in THIS process (foreground) rather than detaching.
$PID | Out-File -FilePath $PidFile -Encoding ascii -Force

# Ensure everything we emit also lands in the log file (tee).
Start-Transcript -Path $LogFile -Append -ErrorAction SilentlyContinue | Out-Null

try {
    # --- One-time venv setup ---------------------------------------------- #
    if (-not (Test-Path $VenvPy)) {
        Write-Note "One-time: setting up tools/.venv (Python deps, ~3 min) ..."
        $setup = Join-Path $ToolsDir 'setup_env.ps1'
        if (Test-Path $setup) {
            & pwsh -NoProfile -ExecutionPolicy Bypass -File $setup
        } else {
            throw "tools/.venv is missing and tools/setup_env.ps1 was not found. Create the venv first."
        }
    }
    if (-not (Test-Path $VenvPy)) {
        throw "Python venv still missing at $VenvPy after setup."
    }

    # --- Determine the data.yaml ------------------------------------------ #
    # If the first remaining arg is an existing .yaml, use it; otherwise export.
    $dataYaml = $null
    $trainArgs = @()
    if ($Argv) { $trainArgs = @($Argv) }

    if ($trainArgs.Count -ge 1 -and ($trainArgs[0] -like '*.yaml') -and (Test-Path $trainArgs[0])) {
        $dataYaml = (Resolve-Path $trainArgs[0]).Path
        # Drop the positional; guard the [1..0] wrap-around when it's the only arg.
        $trainArgs = if ($trainArgs.Count -gt 1) { $trainArgs[1..($trainArgs.Count - 1)] } else { @() }
        Write-Note "Using provided dataset: $dataYaml"
    } else {
        Write-Note "No data.yaml provided; running export_labels.py ..."
        & $VenvPy (Join-Path $ToolsDir 'export_labels.py')
        if ($LASTEXITCODE -ne 0) {
            Send-Toast 'LiveBlock - export failed' 'export_labels.py failed. See tools/runs/auto.log.'
            throw "export_labels.py failed (exit $LASTEXITCODE)."
        }
        # Find the newest exports/<name>/data.yaml under the per-user training dir.
        $exportsRoot = & $VenvPy -c "import sys, pathlib; sys.path.insert(0, r'$ToolsDir'); import lb_paths; print(lb_paths.exports_dir())"
        $exportsRoot = $exportsRoot.Trim()
        if (-not (Test-Path $exportsRoot)) { throw "Exports dir not found: $exportsRoot" }
        $dataYaml = Get-ChildItem -Path $exportsRoot -Recurse -Filter 'data.yaml' -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
        if (-not $dataYaml) { throw "Export succeeded but no data.yaml found under $exportsRoot." }
        Write-Note "Freshest dataset: $dataYaml"
    }

    # --- Train (+ export CoreML/ONNX + install) --------------------------- #
    Write-Note "Running train_logos.py --install"
    $allTrainArgs = @('--data', $dataYaml, '--install') + $trainArgs
    & $VenvPy (Join-Path $ToolsDir 'train_logos.py') @allTrainArgs
    if ($LASTEXITCODE -ne 0) {
        Send-Toast 'LiveBlock - training failed' 'See tools/runs/auto.log.'
        throw "training failed (exit $LASTEXITCODE)."
    }

    Write-Note "All steps OK. Trained model installed for the Windows app."
    Send-Toast 'LiveBlock - model ready' 'Trained model installed. Relaunch the app to use it.'
    $exitCode = 0
}
catch {
    Write-Note "FAILED: $_"
    $exitCode = 1
}
finally {
    Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
    Stop-Transcript -ErrorAction SilentlyContinue | Out-Null
}

exit $exitCode
