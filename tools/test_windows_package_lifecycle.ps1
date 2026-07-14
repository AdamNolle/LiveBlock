[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDir,
    [int]$LaunchSeconds = 12
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $IsWindows) { throw "Windows package lifecycle evidence must run on Windows" }
if ($LaunchSeconds -lt 5 -or $LaunchSeconds -gt 60) {
    throw "LaunchSeconds must be between 5 and 60"
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root
$EvidenceDir = (Resolve-Path -LiteralPath $EvidenceDir).Path
$PackagesDir = Join-Path $EvidenceDir "packages"
$ManifestPath = Join-Path $EvidenceDir "release-manifest.json"
$MsiExtracted = Join-Path $EvidenceDir "msi-extracted"
$LifecycleDir = Join-Path $EvidenceDir "lifecycle"
$ProgressLog = Join-Path $LifecycleDir "progress.log"
$SummaryPath = Join-Path $LifecycleDir "summary.json"

if (Test-Path -LiteralPath $LifecycleDir) {
    throw "Lifecycle evidence directory already exists; evidence is create-new"
}
New-Item -ItemType Directory -Path $LifecycleDir | Out-Null
New-Item -ItemType File -Path $ProgressLog | Out-Null

function Add-Progress([string]$Message) {
    $line = "{0} {1}" -f [DateTime]::UtcNow.ToString("o"), $Message
    Add-Content -LiteralPath $ProgressLog -Value $line -Encoding utf8NoBOM
    Write-Host $line
}

function Write-JsonCreateNew([string]$Path, [object]$Value) {
    $json = $Value | ConvertTo-Json -Depth 8
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes("$json`n")
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Invoke-MsiExec([string[]]$Arguments, [string]$Description) {
    $process = Start-Process -FilePath "msiexec.exe" -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "$Description failed with exit code $($process.ExitCode)"
    }
}

function Get-PropertyValue([object]$Object, [string]$Name) {
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Get-LiveBlockUninstallEntries {
    $roots = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
    )
    $entries = foreach ($rootPath in $roots) {
        Get-ItemProperty -Path $rootPath -ErrorAction SilentlyContinue |
            Where-Object {
                $displayName = Get-PropertyValue $_ "DisplayName"
                $uninstall = Get-PropertyValue $_ "UninstallString"
                $displayName -eq "LiveBlock" -or
                ($uninstall -and $uninstall -match "(?i)LiveBlock")
            }
    }
    return @($entries)
}

function Find-InstalledApplication([string]$ExpectedName, [object[]]$Entries) {
    $roots = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $Entries) {
        $installLocation = Get-PropertyValue $entry "InstallLocation"
        if ($installLocation) {
            [void]$roots.Add([IO.Path]::GetFullPath([string]$installLocation))
        }
    }
    foreach ($candidate in @(
        (Join-Path $env:ProgramFiles "LiveBlock"),
        (Join-Path ${env:ProgramFiles(x86)} "LiveBlock"),
        (Join-Path $env:LOCALAPPDATA "LiveBlock")
    )) {
        if ($candidate) { [void]$roots.Add([IO.Path]::GetFullPath($candidate)) }
    }

    $matches = foreach ($installRoot in $roots) {
        if (Test-Path -LiteralPath $installRoot -PathType Container) {
            Get-ChildItem -LiteralPath $installRoot -Filter $ExpectedName -File -Recurse -ErrorAction Stop
        }
    }
    $unique = @($matches | Sort-Object FullName -Unique)
    if ($unique.Count -ne 1) {
        $found = @($unique | ForEach-Object FullName) -join "; "
        throw "Expected exactly one installed $ExpectedName; found: $found"
    }
    if ($unique[0].LinkType) { throw "Installed application executable must not be a link" }
    return $unique[0]
}

function Write-InstalledInventory(
    [IO.FileInfo]$Application,
    [string]$Output,
    [string]$ArtifactType,
    [string]$Commit
) {
    $installRoot = $Application.Directory.FullName
    $requirements = @(
        $Application.Name,
        "resources/onnxruntime.dll",
        "resources/onnxruntime-THIRD-PARTY-NOTICES.txt",
        "resources/onnxruntime-LICENSE.txt",
        "resources/trusted-model-keys.json"
    )
    foreach ($relative in $requirements) {
        $path = Join-Path $installRoot $relative
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or $item.LinkType) {
            throw "Installed payload requirement must be a regular non-link file: $relative"
        }
    }
    $requiredArguments = @()
    foreach ($relative in $requirements) { $requiredArguments += @("--require", $relative.Replace("\", "/")) }
    python tools/release_evidence.py inventory `
        --root $installRoot --output $Output --platform windows-x86_64 `
        --artifact-type $ArtifactType --commit $Commit @requiredArguments
    if ($LASTEXITCODE -ne 0) { throw "Installed payload inventory creation failed" }
    python tools/release_evidence.py verify --root $installRoot --manifest $Output
    if ($LASTEXITCODE -ne 0) { throw "Installed payload inventory verification failed" }
    return $installRoot
}

function Invoke-BoundedLaunch(
    [IO.FileInfo]$Application,
    [string]$Prefix,
    [int]$Seconds
) {
    $stdout = Join-Path $LifecycleDir "$Prefix-launch.stdout.log"
    $stderr = Join-Path $LifecycleDir "$Prefix-launch.stderr.log"
    $process = Start-Process -FilePath $Application.FullName -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Start-Sleep -Seconds $Seconds
    $process.Refresh()
    if ($process.HasExited) {
        throw "$Prefix application exited before the $Seconds-second launch gate with code $($process.ExitCode)"
    }
    $processName = $process.ProcessName
    Stop-Process -Id $process.Id -Force -ErrorAction Stop
    $process.WaitForExit(5000) | Out-Null
    Add-Progress "$Prefix launch remained active for $Seconds seconds and was terminated"
    return [ordered]@{
        survivedSeconds = $Seconds
        processName = $processName
        terminatedAfterGate = $true
    }
}

function Wait-Removed([string]$ApplicationPath, [string]$Description) {
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        if (-not (Test-Path -LiteralPath $ApplicationPath) -and
            (Get-LiveBlockUninstallEntries).Count -eq 0) {
            return
        }
        Start-Sleep -Seconds 1
    }
    throw "$Description did not remove the application payload and uninstall registration within 30 seconds"
}

function Resolve-NsisUninstaller([object[]]$Entries, [string]$InstallRoot) {
    $commands = @($Entries | ForEach-Object {
        $quiet = Get-PropertyValue $_ "QuietUninstallString"
        $regular = Get-PropertyValue $_ "UninstallString"
        if ($quiet) { [string]$quiet }
        elseif ($regular) { [string]$regular }
    })
    foreach ($command in $commands) {
        if ($command -match '^\s*"([^"]+\.exe)"') {
            if (Test-Path -LiteralPath $Matches[1] -PathType Leaf) { return $Matches[1] }
        } elseif ($command -match '^\s*(.+?\.exe)(?:\s|$)') {
            if (Test-Path -LiteralPath $Matches[1] -PathType Leaf) { return $Matches[1] }
        }
    }
    $fallback = @(Get-ChildItem -LiteralPath $InstallRoot -File -Filter "uninstall*.exe" -Recurse)
    if ($fallback.Count -ne 1) { throw "Expected exactly one NSIS uninstaller" }
    return $fallback[0].FullName
}

$manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
if ($manifest.schema -ne 1 -or
    $manifest.artifactType -ne "windows-installers-build-only" -or
    $manifest.signed -ne $false -or
    $manifest.timestamped -ne $false -or
    $manifest.promotedModelEmbedded -ne $false) {
    throw "Lifecycle smoke accepts only unsigned, untimestamped, no-model BuildOnly evidence"
}
$commit = [string]$manifest.gitCommit
if ($commit -notmatch '^[0-9a-f]{40}$') { throw "Release manifest Git commit is invalid" }

$msiPackages = @(Get-ChildItem -LiteralPath $PackagesDir -File -Filter "*.msi")
$nsisPackages = @(Get-ChildItem -LiteralPath $PackagesDir -File -Filter "*.exe")
if ($msiPackages.Count -ne 1 -or $nsisPackages.Count -ne 1) {
    throw "Expected exactly one MSI and one NSIS package"
}
$extractedExecutables = @(Get-ChildItem -LiteralPath $MsiExtracted -File -Filter "*.exe" -Recurse)
if ($extractedExecutables.Count -ne 1) { throw "Expected exactly one application executable in MSI extraction evidence" }
$expectedExecutableName = $extractedExecutables[0].Name
if ((Get-LiveBlockUninstallEntries).Count -ne 0) {
    throw "LiveBlock is already registered; clean-install evidence requires an absent package"
}

$msiInstalled = $false
$nsisInstalled = $false
$msiApplicationPath = $null
$nsisApplicationPath = $null
$nsisUninstaller = $null
try {
    Add-Progress "starting MSI clean-install lifecycle"
    $msiInstallLog = Join-Path $LifecycleDir "msi-install.log"
    $msiInstalled = $true
    Invoke-MsiExec @(
        "/i", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiInstallLog`""
    ) "MSI clean install"
    $msiEntries = @(Get-LiveBlockUninstallEntries)
    if ($msiEntries.Count -eq 0) { throw "MSI install did not create an uninstall registration" }
    $msiApplication = Find-InstalledApplication $expectedExecutableName $msiEntries
    $msiApplicationPath = $msiApplication.FullName
    $msiInventory = Join-Path $LifecycleDir "msi-installed-payload-inventory.json"
    [void](Write-InstalledInventory $msiApplication $msiInventory "windows-build-only-msi-installed" $commit)
    $msiLaunch = Invoke-BoundedLaunch $msiApplication "msi" $LaunchSeconds

    $msiUninstallLog = Join-Path $LifecycleDir "msi-uninstall.log"
    Invoke-MsiExec @(
        "/x", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiUninstallLog`""
    ) "MSI uninstall"
    $msiInstalled = $false
    Wait-Removed $msiApplicationPath "MSI uninstall"
    Add-Progress "MSI uninstall removed payload and registration"

    Add-Progress "starting NSIS clean-install lifecycle"
    $nsisInstallStdout = Join-Path $LifecycleDir "nsis-install.stdout.log"
    $nsisInstallStderr = Join-Path $LifecycleDir "nsis-install.stderr.log"
    $nsisInstalled = $true
    $nsisInstall = Start-Process -FilePath $nsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisInstallStdout -RedirectStandardError $nsisInstallStderr
    if ($nsisInstall.ExitCode -ne 0) { throw "NSIS clean install failed with exit code $($nsisInstall.ExitCode)" }
    $nsisEntries = @(Get-LiveBlockUninstallEntries)
    if ($nsisEntries.Count -eq 0) { throw "NSIS install did not create an uninstall registration" }
    $nsisApplication = Find-InstalledApplication $expectedExecutableName $nsisEntries
    $nsisApplicationPath = $nsisApplication.FullName
    $nsisInventory = Join-Path $LifecycleDir "nsis-installed-payload-inventory.json"
    $nsisInstallRoot = Write-InstalledInventory $nsisApplication $nsisInventory "windows-build-only-nsis-installed" $commit
    $nsisUninstaller = Resolve-NsisUninstaller $nsisEntries $nsisInstallRoot
    $nsisLaunch = Invoke-BoundedLaunch $nsisApplication "nsis" $LaunchSeconds

    $nsisUninstallStdout = Join-Path $LifecycleDir "nsis-uninstall.stdout.log"
    $nsisUninstallStderr = Join-Path $LifecycleDir "nsis-uninstall.stderr.log"
    $nsisRemove = Start-Process -FilePath $nsisUninstaller -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisUninstallStdout -RedirectStandardError $nsisUninstallStderr
    if ($nsisRemove.ExitCode -ne 0) { throw "NSIS uninstall failed with exit code $($nsisRemove.ExitCode)" }
    $nsisInstalled = $false
    Wait-Removed $nsisApplicationPath "NSIS uninstall"
    Add-Progress "NSIS uninstall removed payload and registration"

    $summary = [ordered]@{
        schemaVersion = 1
        evidenceClass = "build-only-hosted-windows"
        gitCommit = $commit
        productionModelAndTrustRoots = $false
        signedAndTimestamped = $false
        hardwareCertification = $false
        msi = [ordered]@{
            package = $msiPackages[0].Name
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $msiPackages[0].FullName).Hash.ToLowerInvariant()
            cleanInstall = $true
            installedPayloadInventory = "msi-installed-payload-inventory.json"
            launch = $msiLaunch
            uninstallRemovedPayloadAndRegistration = $true
        }
        nsis = [ordered]@{
            package = $nsisPackages[0].Name
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $nsisPackages[0].FullName).Hash.ToLowerInvariant()
            cleanInstall = $true
            installedPayloadInventory = "nsis-installed-payload-inventory.json"
            launch = $nsisLaunch
            uninstallRemovedPayloadAndRegistration = $true
        }
        notTested = @(
            "signed installer execution",
            "promoted model authentication",
            "same-version repair or prior-version upgrade",
            "capture, DirectML, GPU, display, lifecycle, accessibility, or anti-cheat behavior"
        )
    }
    Write-JsonCreateNew $SummaryPath $summary
    Add-Progress "Windows build-only installer lifecycle passed"
} finally {
    Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($expectedExecutableName)) -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
    if ($nsisInstalled) {
        if (-not $nsisUninstaller) {
            $fallbackRoot = Join-Path $env:LOCALAPPDATA "LiveBlock"
            try {
                $nsisUninstaller = Resolve-NsisUninstaller @(Get-LiveBlockUninstallEntries) $fallbackRoot
            } catch {}
        }
        if ($nsisUninstaller -and (Test-Path -LiteralPath $nsisUninstaller -PathType Leaf)) {
            try { Start-Process -FilePath $nsisUninstaller -ArgumentList "/S" -Wait | Out-Null } catch {}
        }
    }
    if ($msiInstalled) {
        try {
            Invoke-MsiExec @("/x", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart") "MSI cleanup uninstall"
        } catch {}
    }
}
