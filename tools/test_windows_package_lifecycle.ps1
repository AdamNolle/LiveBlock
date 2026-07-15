[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDir,
    [Parameter(Mandatory = $true)]
    [string]$PreviousEvidenceDir,
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
$PreviousEvidenceDir = (Resolve-Path -LiteralPath $PreviousEvidenceDir).Path
$PackagesDir = Join-Path $EvidenceDir "packages"
$PreviousPackagesDir = Join-Path $PreviousEvidenceDir "packages"
$PreviousManifestPath = Join-Path $PreviousEvidenceDir "release-manifest.json"
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

function Assert-UserDataSentinel([string]$Description) {
    $actual = Get-Content -LiteralPath $UserDataSentinel -Raw -ErrorAction Stop
    if ($actual -ne $UserDataSentinelValue) {
        throw "$Description modified the application-data sentinel"
    }
}

function Assert-SingleRegisteredVersion([string]$ExpectedVersion, [string]$Description) {
    $entries = @(Get-LiveBlockUninstallEntries)
    if ($entries.Count -ne 1) {
        throw "$Description expected exactly one LiveBlock registration; found $($entries.Count)"
    }
    $actualVersion = [string](Get-PropertyValue $entries[0] "DisplayVersion")
    if ($actualVersion -ne $ExpectedVersion) {
        throw "$Description expected registered version $ExpectedVersion; found $actualVersion"
    }
    return $entries
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

function Test-BuildOnlyRuntimePayload([string]$InstallRoot, [string]$Prefix) {
    $keyringPath = Join-Path $InstallRoot "resources/trusted-model-keys.json"
    $keyring = Get-Content -LiteralPath $keyringPath -Raw -ErrorAction Stop | ConvertFrom-Json
    if ($keyring.schemaVersion -ne 1 -or @($keyring.keys).Count -ne 0) {
        throw "$Prefix BuildOnly package must contain the schema-1 empty development keyring"
    }

    $runtimePath = Join-Path $InstallRoot "resources/onnxruntime.dll"
    $runtimeHandle = [IntPtr]::Zero
    try {
        $runtimeHandle = [Runtime.InteropServices.NativeLibrary]::Load($runtimePath)
        if ($runtimeHandle -eq [IntPtr]::Zero) {
            throw "$Prefix packaged ONNX Runtime returned a null native-library handle"
        }
    }
    finally {
        if ($runtimeHandle -ne [IntPtr]::Zero) {
            [Runtime.InteropServices.NativeLibrary]::Free($runtimeHandle)
        }
    }
    Add-Progress "$Prefix payload has an empty development keyring and a loadable packaged ONNX Runtime"
    return [ordered]@{
        emptyDevelopmentKeyringVerified = $true
        packagedOnnxRuntimeLoadable = $true
    }
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
    if (-not $process.WaitForExit(5000)) {
        throw "$Prefix application did not exit within five seconds after bounded-launch termination"
    }
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
            @(Get-LiveBlockUninstallEntries).Count -eq 0) {
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
$currentVersion = [Version]((Get-Content -LiteralPath (Join-Path $Root "platform/windows/src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json).version)
$previousManifest = Get-Content -LiteralPath $PreviousManifestPath -Raw | ConvertFrom-Json
if ($previousManifest.schema -ne 1 -or
    $previousManifest.artifactType -ne "windows-installers-version-fixture-build-only" -or
    $previousManifest.signed -ne $false -or
    $previousManifest.timestamped -ne $false -or
    $previousManifest.promotedModelEmbedded -ne $false) {
    throw "Prior-version lifecycle accepts only unsigned, untimestamped, no-model fixture evidence"
}
$previousVersion = [Version]([string]$previousManifest.packageFixtureVersion)
if ($previousVersion -ge $currentVersion) {
    throw "Prior-version fixture must be older than current version $currentVersion"
}
$currentVersionString = $currentVersion.ToString()
$previousVersionString = $previousVersion.ToString()

$msiPackages = @(Get-ChildItem -LiteralPath $PackagesDir -File -Filter "*.msi")
$nsisPackages = @(Get-ChildItem -LiteralPath $PackagesDir -File -Filter "*.exe")
$previousMsiPackages = @(Get-ChildItem -LiteralPath $PreviousPackagesDir -File -Filter "*.msi")
$previousNsisPackages = @(Get-ChildItem -LiteralPath $PreviousPackagesDir -File -Filter "*.exe")
if ($msiPackages.Count -ne 1 -or $nsisPackages.Count -ne 1) {
    throw "Expected exactly one current MSI and one current NSIS package"
}
if ($previousMsiPackages.Count -ne 1 -or $previousNsisPackages.Count -ne 1) {
    throw "Expected exactly one prior-version MSI and one prior-version NSIS fixture"
}
$extractedExecutables = @(Get-ChildItem -LiteralPath $MsiExtracted -File -Filter "*.exe" -Recurse)
if ($extractedExecutables.Count -ne 1) { throw "Expected exactly one application executable in MSI extraction evidence" }
$expectedExecutableName = $extractedExecutables[0].Name
if (@(Get-LiveBlockUninstallEntries).Count -ne 0) {
    throw "LiveBlock is already registered; clean-install evidence requires an absent package"
}
$UserDataRoot = Join-Path $env:APPDATA "LiveBlock"
$UserDataSentinel = Join-Path $UserDataRoot "package-transition-sentinel.txt"
$UserDataSentinelValue = "liveblock-package-transition-sentinel-v1"
if (Test-Path -LiteralPath $UserDataRoot) {
    throw "Application-data root unexpectedly exists before transition test: $UserDataRoot"
}
New-Item -ItemType Directory -Path $UserDataRoot | Out-Null
$sentinelStream = [IO.File]::Open($UserDataSentinel, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
try {
    $sentinelBytes = [Text.UTF8Encoding]::new($false).GetBytes($UserDataSentinelValue)
    $sentinelStream.Write($sentinelBytes, 0, $sentinelBytes.Length)
    $sentinelStream.Flush($true)
} finally { $sentinelStream.Dispose() }

$msiInstalled = $false
$nsisInstalled = $false
$msiApplicationPath = $null
$nsisApplicationPath = $null
$nsisUninstaller = $null
try {
    Add-Progress "starting MSI prior-version install and upgrade lifecycle"
    $msiPreviousInstallLog = Join-Path $LifecycleDir "msi-previous-install.log"
    $msiUpgradeLog = Join-Path $LifecycleDir "msi-upgrade.log"
    $msiRepairLog = Join-Path $LifecycleDir "msi-repair.log"
    $msiDowngradeLog = Join-Path $LifecycleDir "msi-downgrade.log"
    $msiInstalled = $true
    Invoke-MsiExec @(
        "/i", "`"$($previousMsiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiPreviousInstallLog`""
    ) "MSI prior-version install"
    [void](Assert-SingleRegisteredVersion $previousVersionString "MSI prior-version install")
    Invoke-MsiExec @(
        "/i", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiUpgradeLog`""
    ) "MSI major upgrade"
    $msiEntries = Assert-SingleRegisteredVersion $currentVersionString "MSI major upgrade"
    Invoke-MsiExec @(
        "/fa", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiRepairLog`""
    ) "MSI same-version repair"
    [void](Assert-SingleRegisteredVersion $currentVersionString "MSI same-version repair")
    $downgradeProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @(
        "/i", "`"$($previousMsiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiDowngradeLog`""
    ) -Wait -PassThru
    if ($downgradeProcess.ExitCode -eq 0) {
        throw "MSI downgrade fixture was accepted over current version"
    }
    [void](Assert-SingleRegisteredVersion $currentVersionString "MSI downgrade rejection")
    Assert-UserDataSentinel "MSI upgrade/repair/downgrade"
    Add-Progress "MSI upgraded $previousVersion to $currentVersion, repaired current, and rejected downgrade with exit code $($downgradeProcess.ExitCode)"
    $msiApplication = Find-InstalledApplication $expectedExecutableName $msiEntries
    $msiApplicationPath = $msiApplication.FullName
    $msiInventory = Join-Path $LifecycleDir "msi-installed-payload-inventory.json"
    $msiInstallRoot = Write-InstalledInventory $msiApplication $msiInventory "windows-build-only-msi-installed" $commit
    $msiPayloadCheck = Test-BuildOnlyRuntimePayload $msiInstallRoot "msi"
    $msiLaunch = Invoke-BoundedLaunch $msiApplication "msi" $LaunchSeconds

    $msiUninstallLog = Join-Path $LifecycleDir "msi-uninstall.log"
    Invoke-MsiExec @(
        "/x", "`"$($msiPackages[0].FullName)`"", "/qn", "/norestart", "/L*V", "`"$msiUninstallLog`""
    ) "MSI uninstall"
    $msiInstalled = $false
    Wait-Removed $msiApplicationPath "MSI uninstall"
    Assert-UserDataSentinel "MSI uninstall"
    Add-Progress "MSI uninstall removed payload and registration while preserving user data"

    Add-Progress "starting NSIS prior-version install and transition lifecycle"
    $nsisPreviousInstallStdout = Join-Path $LifecycleDir "nsis-previous-install.stdout.log"
    $nsisPreviousInstallStderr = Join-Path $LifecycleDir "nsis-previous-install.stderr.log"
    $nsisUpgradeStdout = Join-Path $LifecycleDir "nsis-upgrade.stdout.log"
    $nsisUpgradeStderr = Join-Path $LifecycleDir "nsis-upgrade.stderr.log"
    $nsisRepairStdout = Join-Path $LifecycleDir "nsis-repair.stdout.log"
    $nsisRepairStderr = Join-Path $LifecycleDir "nsis-repair.stderr.log"
    $nsisDowngradeStdout = Join-Path $LifecycleDir "nsis-downgrade.stdout.log"
    $nsisDowngradeStderr = Join-Path $LifecycleDir "nsis-downgrade.stderr.log"
    $nsisInstalled = $true
    $nsisPreviousInstall = Start-Process -FilePath $previousNsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisPreviousInstallStdout -RedirectStandardError $nsisPreviousInstallStderr
    if ($nsisPreviousInstall.ExitCode -ne 0) { throw "NSIS prior-version install failed with exit code $($nsisPreviousInstall.ExitCode)" }
    [void](Assert-SingleRegisteredVersion $previousVersionString "NSIS prior-version install")
    $nsisUpgrade = Start-Process -FilePath $nsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisUpgradeStdout -RedirectStandardError $nsisUpgradeStderr
    if ($nsisUpgrade.ExitCode -ne 0) { throw "NSIS upgrade failed with exit code $($nsisUpgrade.ExitCode)" }
    [void](Assert-SingleRegisteredVersion $currentVersionString "NSIS upgrade")
    $nsisRepair = Start-Process -FilePath $nsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisRepairStdout -RedirectStandardError $nsisRepairStderr
    if ($nsisRepair.ExitCode -ne 0) { throw "NSIS same-version reinstall failed with exit code $($nsisRepair.ExitCode)" }
    [void](Assert-SingleRegisteredVersion $currentVersionString "NSIS same-version reinstall")
    $nsisDowngrade = Start-Process -FilePath $previousNsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisDowngradeStdout -RedirectStandardError $nsisDowngradeStderr
    $afterDowngradeEntries = @(Get-LiveBlockUninstallEntries)
    if ($afterDowngradeEntries.Count -ne 1) {
        throw "NSIS downgrade attempt expected one registration; found $($afterDowngradeEntries.Count)"
    }
    $afterDowngradeVersion = [string](Get-PropertyValue $afterDowngradeEntries[0] "DisplayVersion")
    if ($afterDowngradeVersion -notin @($currentVersionString, $previousVersionString)) {
        throw "NSIS downgrade attempt produced unexpected version $afterDowngradeVersion"
    }
    $nsisDowngradeAccepted = $afterDowngradeVersion -eq $previousVersionString
    if ($nsisDowngradeAccepted) {
        $nsisRestoreStdout = Join-Path $LifecycleDir "nsis-restore-current.stdout.log"
        $nsisRestoreStderr = Join-Path $LifecycleDir "nsis-restore-current.stderr.log"
        $nsisRestore = Start-Process -FilePath $nsisPackages[0].FullName -ArgumentList "/S" -Wait -PassThru `
            -RedirectStandardOutput $nsisRestoreStdout -RedirectStandardError $nsisRestoreStderr
        if ($nsisRestore.ExitCode -ne 0) { throw "NSIS current-version restore failed with exit code $($nsisRestore.ExitCode)" }
        [void](Assert-SingleRegisteredVersion $currentVersionString "NSIS current-version restore")
    }
    Assert-UserDataSentinel "NSIS upgrade/reinstall/downgrade/restore"
    Add-Progress "NSIS upgraded $previousVersion to $currentVersion, reinstalled current, observed downgradeAccepted=$nsisDowngradeAccepted, and restored current"
    $nsisEntries = @(Get-LiveBlockUninstallEntries)
    $nsisApplication = Find-InstalledApplication $expectedExecutableName $nsisEntries
    $nsisApplicationPath = $nsisApplication.FullName
    $nsisInventory = Join-Path $LifecycleDir "nsis-installed-payload-inventory.json"
    $nsisInstallRoot = Write-InstalledInventory $nsisApplication $nsisInventory "windows-build-only-nsis-installed" $commit
    $nsisPayloadCheck = Test-BuildOnlyRuntimePayload $nsisInstallRoot "nsis"
    $nsisUninstaller = Resolve-NsisUninstaller $nsisEntries $nsisInstallRoot
    $nsisLaunch = Invoke-BoundedLaunch $nsisApplication "nsis" $LaunchSeconds

    $nsisUninstallStdout = Join-Path $LifecycleDir "nsis-uninstall.stdout.log"
    $nsisUninstallStderr = Join-Path $LifecycleDir "nsis-uninstall.stderr.log"
    $nsisRemove = Start-Process -FilePath $nsisUninstaller -ArgumentList "/S" -Wait -PassThru `
        -RedirectStandardOutput $nsisUninstallStdout -RedirectStandardError $nsisUninstallStderr
    if ($nsisRemove.ExitCode -ne 0) { throw "NSIS uninstall failed with exit code $($nsisRemove.ExitCode)" }
    $nsisInstalled = $false
    Wait-Removed $nsisApplicationPath "NSIS uninstall"
    Assert-UserDataSentinel "NSIS uninstall"
    Add-Progress "NSIS uninstall removed payload and registration while preserving user data"

    $summary = [ordered]@{
        schemaVersion = 1
        evidenceClass = "build-only-hosted-windows"
        gitCommit = $commit
        productionModelAndTrustRoots = $false
        signedAndTimestamped = $false
        hardwareCertification = $false
        userDataSentinelPreservedAcrossTransitionsAndUninstall = $true
        msi = [ordered]@{
            package = $msiPackages[0].Name
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $msiPackages[0].FullName).Hash.ToLowerInvariant()
            priorVersionFixture = $previousVersionString
            priorVersionInstall = $true
            upgradeToCurrent = $true
            sameVersionRepair = $true
            downgradeRejected = $true
            downgradeExitCode = $downgradeProcess.ExitCode
            installedPayloadInventory = "msi-installed-payload-inventory.json"
            payloadCheck = $msiPayloadCheck
            launch = $msiLaunch
            uninstallRemovedPayloadAndRegistration = $true
        }
        nsis = [ordered]@{
            package = $nsisPackages[0].Name
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $nsisPackages[0].FullName).Hash.ToLowerInvariant()
            priorVersionFixture = $previousVersionString
            priorVersionInstall = $true
            upgradeToCurrent = $true
            sameVersionReinstall = $true
            silentDowngradeAccepted = $nsisDowngradeAccepted
            downgradeProtectionPassed = (-not $nsisDowngradeAccepted)
            downgradeExitCode = $nsisDowngrade.ExitCode
            currentRestoredAfterDowngradeProbe = $true
            installedPayloadInventory = "nsis-installed-payload-inventory.json"
            payloadCheck = $nsisPayloadCheck
            launch = $nsisLaunch
            uninstallRemovedPayloadAndRegistration = $true
        }
        notTested = @(
            "signed installer execution",
            "promoted model authentication",
            "signed NSIS transition execution",
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
        foreach ($cleanupMsi in @($msiPackages[0], $previousMsiPackages[0])) {
            try {
                Invoke-MsiExec @("/x", "`"$($cleanupMsi.FullName)`"", "/qn", "/norestart") "MSI cleanup uninstall"
            } catch {}
        }
    }
    if (Test-Path -LiteralPath $UserDataRoot -PathType Container) {
        Remove-Item -LiteralPath $UserDataRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
