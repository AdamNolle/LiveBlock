[CmdletBinding()]
param(
    [ValidateSet("DryRun", "BuildOnly", "Execute")]
    [string]$Mode = "DryRun",
    [string]$OutputDir = "",
    [switch]$AllowDirty
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing required command: $Name"
    }
}

function Wait-RegularFileStable([string]$Path) {
    $previous = $null
    $stableSamples = 0
    for ($attempt = 0; $attempt -lt 15; $attempt++) {
        $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or $item.LinkType) { throw "Package output must be a regular non-symlink file: $Path" }
        $fingerprint = "$($item.Length):$($item.LastWriteTimeUtc.Ticks)"
        if ($fingerprint -eq $previous) {
            $stableSamples++
            if ($stableSamples -ge 2) { return }
        } else {
            $stableSamples = 0
            $previous = $fingerprint
        }
        Start-Sleep -Seconds 1
    }
    throw "Package output did not become stable within 15 seconds: $Path"
}

function Find-SignTool {
    if ($env:LB_WINDOWS_SIGNTOOL) {
        $candidate = Get-Item -LiteralPath $env:LB_WINDOWS_SIGNTOOL -ErrorAction Stop
        if ($candidate.PSIsContainer) { throw "LB_WINDOWS_SIGNTOOL must name signtool.exe" }
        return $candidate.FullName
    }
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $candidate = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $candidate) { throw "signtool.exe was not found in PATH or the Windows 10 SDK" }
    return $candidate.FullName
}

if (-not $IsWindows) { throw "Windows release packaging must run on Windows" }
foreach ($tool in @("git", "cargo", "node", "python")) { Require-Command $tool }

$Tauri = Join-Path $Root "platform\_shared-frontend\node_modules\.bin\tauri.cmd"
if (-not (Test-Path -LiteralPath $Tauri -PathType Leaf)) {
    throw "Missing pinned Tauri CLI; run npm ci in platform/_shared-frontend"
}
$Resources = Join-Path $Root "platform\windows\src-tauri\resources"
$DevelopmentKeyring = Join-Path $Resources "trusted-model-keys.json"
$ArtifactStaging = Join-Path $Resources "liveblock-detector.onnx"
$ManifestStaging = Join-Path $Resources "liveblock-detector.manifest.json"
$RuntimeStaging = Join-Path $Resources "onnxruntime.dll"
$RuntimeNoticesStaging = Join-Path $Resources "onnxruntime-THIRD-PARTY-NOTICES.txt"
$RuntimeLicenseStaging = Join-Path $Resources "onnxruntime-LICENSE.txt"

$stamp = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssZ")
if (-not $OutputDir) { $OutputDir = Join-Path $Root "tools\runs\release-windows\$stamp" }
$OutputDir = [IO.Path]::GetFullPath($OutputDir)
$PackageDir = Join-Path $OutputDir "packages"
$Inventory = Join-Path $OutputDir "package-inventory.json"
$MsiExtracted = Join-Path $OutputDir "msi-extracted"
$MsiPayloadInventory = Join-Path $OutputDir "msi-payload-inventory.json"
$MsiExtractLog = Join-Path $OutputDir "msi-administrative-extract.log"
$Checksum = Join-Path $OutputDir "package.sha256"

if ($Mode -eq "Execute") {
    if ($env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING -eq "1") {
        throw "The empty model-keyring override is forbidden for release execution"
    }
    foreach ($name in @("LB_WINDOWS_MODEL_BUNDLE_DIR", "LB_WINDOWS_CERTIFICATE_SHA1", "LB_WINDOWS_TIMESTAMP_URL")) {
        if (-not [Environment]::GetEnvironmentVariable($name)) { throw "Set $name for Execute mode" }
    }
    if (-not $env:LB_WINDOWS_TIMESTAMP_URL.StartsWith("https://", [StringComparison]::OrdinalIgnoreCase)) {
        throw "LB_WINDOWS_TIMESTAMP_URL must use HTTPS"
    }
    if (-not $AllowDirty -and (git status --porcelain --untracked-files=all)) {
        throw "Refusing release from a dirty tree, including untracked files (use -AllowDirty only for non-release testing)"
    }
}

python tools/validate_model_keyring.py --keyring $DevelopmentKeyring --allow-empty
if ($LASTEXITCODE -ne 0) { throw "Development keyring validation failed" }

$buildCommand = "& '$Tauri' build --bundles msi,nsis --ci"
if ($Mode -eq "DryRun") {
    $signToolDisplay = "<Windows SDK signtool.exe>"
    Write-Host "Windows release dry-run passed local preflight."
    Write-Host "No package, signing, timestamp, or release artifact was produced."
    Write-Host "Planned output: $OutputDir"
    Write-Host "Planned build: $buildCommand"
    Write-Host "Planned signing: $signToolDisplay sign /fd SHA256 /sha1 <certificate thumbprint> /tr <HTTPS timestamp URL> /td SHA256 <MSI and NSIS packages>"
    Write-Host "Planned verification: $signToolDisplay verify /pa /all /v <each package>"
    exit 0
}

if (Test-Path -LiteralPath $OutputDir) {
    throw "Output directory already exists; Windows package evidence is create-new"
}

$originalKeyring = [IO.File]::ReadAllBytes($DevelopmentKeyring)
$stagedProduction = $false
$stagedRuntime = $false
$oldOverride = $env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING
try {
    if ($Mode -eq "Execute") {
        if (Test-Path -LiteralPath $ArtifactStaging) { throw "Internal ONNX staging path already exists" }
        if (Test-Path -LiteralPath $ManifestStaging) { throw "Internal manifest staging path already exists" }
        $bundle = (Resolve-Path -LiteralPath $env:LB_WINDOWS_MODEL_BUNDLE_DIR).Path
        python tools/verify_signed_onnx_bundle.py --bundle $bundle
        if ($LASTEXITCODE -ne 0) { throw "Protected ONNX bundle verification failed" }
        $stagedProduction = $true
        Copy-Item -LiteralPath (Join-Path $bundle "liveblock-detector.onnx") -Destination $ArtifactStaging
        Copy-Item -LiteralPath (Join-Path $bundle "liveblock-detector.manifest.json") -Destination $ManifestStaging
        Copy-Item -LiteralPath (Join-Path $bundle "trusted-model-keys.json") -Destination $DevelopmentKeyring -Force
        Remove-Item Env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING -ErrorAction SilentlyContinue
    } else {
        if ((Test-Path -LiteralPath $ArtifactStaging) -or (Test-Path -LiteralPath $ManifestStaging)) {
            throw "BuildOnly mode refuses leftover production model staging"
        }
        $env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING = "1"
    }

    # Install/build before launching the Tauri CLI. On Windows the running
    # native CLI module locks itself, so a nested `npm ci` cannot safely unlink it.
    Push-Location (Join-Path $Root "platform\_shared-frontend")
    try {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "frontend build failed" }
    } finally { Pop-Location }

    foreach ($path in @($RuntimeStaging, $RuntimeNoticesStaging, $RuntimeLicenseStaging)) {
        if (Test-Path -LiteralPath $path) { throw "Internal ONNX Runtime staging path already exists: $path" }
    }
    python tools/stage_windows_onnxruntime.py
    if ($LASTEXITCODE -ne 0) { throw "Pinned ONNX Runtime DirectML staging failed" }
    $stagedRuntime = $true
    Push-Location (Join-Path $Root "platform\windows\src-tauri")
    try { & $Tauri build --bundles msi,nsis --ci } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "Tauri Windows bundle build failed" }

    $TargetRoot = Join-Path $Root "platform\windows\target"
    $packages = @(Get-ChildItem -Path $TargetRoot -Recurse -File |
        Where-Object {
            ($_.Extension -eq ".msi" -and $_.Directory.Name -eq "msi") -or
            ($_.Extension -eq ".exe" -and $_.Directory.Name -eq "nsis")
        })
    if (@($packages | Where-Object Extension -eq ".msi").Count -ne 1 -or
        @($packages | Where-Object { $_.Extension -eq ".exe" -and $_.Directory.Name -eq "nsis" }).Count -ne 1) {
        throw "Expected exactly one MSI and one NSIS installer"
    }
    New-Item -ItemType Directory -Path $PackageDir -Force | Out-Null
    $copied = foreach ($package in $packages) {
        Copy-Item -LiteralPath $package.FullName -Destination (Join-Path $PackageDir $package.Name) -PassThru
    }

    if ($Mode -eq "Execute") {
        $SignTool = Find-SignTool
        foreach ($package in $copied) {
            & $SignTool sign /fd SHA256 /sha1 $env:LB_WINDOWS_CERTIFICATE_SHA1 /tr $env:LB_WINDOWS_TIMESTAMP_URL /td SHA256 $package.FullName
            if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed for $($package.Name)" }
            & $SignTool verify /pa /all /v $package.FullName
            if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed for $($package.Name)" }
            if ((Get-AuthenticodeSignature -LiteralPath $package.FullName).Status -ne "Valid") {
                throw "PowerShell Authenticode validation failed for $($package.Name)"
            }
        }
    }

    $msi = $copied | Where-Object Extension -eq ".msi" | Select-Object -First 1
    New-Item -ItemType Directory -Path $MsiExtracted | Out-Null
    $msiArguments = @(
        "/a", "`"$($msi.FullName)`"",
        "/qn", "TARGETDIR=`"$MsiExtracted`"",
        "/L*V", "`"$MsiExtractLog`""
    )
    $msiProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList $msiArguments -Wait -PassThru
    if ($msiProcess.ExitCode -ne 0) { throw "MSI administrative extraction failed with exit code $($msiProcess.ExitCode)" }
    $payloadRequirements = @()
    $executables = @(Get-ChildItem -LiteralPath $MsiExtracted -Recurse -File -Filter "*.exe")
    if ($executables.Count -ne 1) {
        $found = @(Get-ChildItem -LiteralPath $MsiExtracted -Recurse -File | ForEach-Object FullName) -join "; "
        throw "MSI payload must contain exactly one application executable; found: $found"
    }
    $relative = [IO.Path]::GetRelativePath($MsiExtracted, $executables[0].FullName).Replace("\", "/")
    $payloadRequirements += @("--require", $relative)
    foreach ($name in @("onnxruntime.dll", "onnxruntime-THIRD-PARTY-NOTICES.txt", "onnxruntime-LICENSE.txt", "trusted-model-keys.json")) {
        $matches = @(Get-ChildItem -LiteralPath $MsiExtracted -Recurse -File -Filter $name)
        if ($matches.Count -ne 1) { throw "MSI payload must contain exactly one $name" }
        $relative = [IO.Path]::GetRelativePath($MsiExtracted, $matches[0].FullName).Replace("\", "/")
        $payloadRequirements += @("--require", $relative)
    }
    if ($Mode -eq "Execute") {
        foreach ($name in @("liveblock-detector.onnx", "liveblock-detector.manifest.json")) {
            $matches = @(Get-ChildItem -LiteralPath $MsiExtracted -Recurse -File -Filter $name)
            if ($matches.Count -ne 1) { throw "Production MSI payload must contain exactly one $name" }
            $relative = [IO.Path]::GetRelativePath($MsiExtracted, $matches[0].FullName).Replace("\", "/")
            $payloadRequirements += @("--require", $relative)
        }
    }

    foreach ($package in $copied) { Wait-RegularFileStable $package.FullName }
    $hashLines = foreach ($package in ($copied | Sort-Object Name)) {
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $package.FullName).Hash.ToLowerInvariant()
        "$hash  $($package.Name)"
    }
    [IO.File]::WriteAllLines($Checksum, $hashLines, [Text.UTF8Encoding]::new($false))
    $commit = (git rev-parse HEAD).Trim()
    $required = @()
    foreach ($package in $copied) { $required += @("--require", $package.Name) }
    $artifactType = if ($Mode -eq "Execute") { "windows-installers-signed" } else { "windows-installers-build-only" }
    python tools/release_evidence.py inventory --root $PackageDir --output $Inventory --platform windows-x86_64 --artifact-type $artifactType --commit $commit @required
    if ($LASTEXITCODE -ne 0) { throw "Package inventory creation failed" }
    python tools/release_evidence.py verify --root $PackageDir --manifest $Inventory
    if ($LASTEXITCODE -ne 0) { throw "Package inventory verification failed" }
    python tools/release_evidence.py inventory --root $MsiExtracted --output $MsiPayloadInventory --platform windows-x86_64 --artifact-type "$artifactType-msi-payload" --commit $commit @payloadRequirements
    if ($LASTEXITCODE -ne 0) { throw "MSI payload inventory creation failed" }
    python tools/release_evidence.py verify --root $MsiExtracted --manifest $MsiPayloadInventory
    if ($LASTEXITCODE -ne 0) { throw "MSI payload inventory verification failed" }

    $manifest = [ordered]@{
        schema = 1
        platform = "windows-x86_64"
        gitCommit = $commit
        artifactType = $artifactType
        signed = ($Mode -eq "Execute")
        timestamped = ($Mode -eq "Execute")
        promotedModelEmbedded = ($Mode -eq "Execute")
        packages = @($copied | Sort-Object Name | ForEach-Object { $_.Name })
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDir "release-manifest.json") -Encoding utf8NoBOM
    Write-Host "Verified Windows package evidence: $OutputDir"
} finally {
    if ($stagedRuntime) {
        Remove-Item -LiteralPath $RuntimeStaging -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $RuntimeNoticesStaging -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $RuntimeLicenseStaging -Force -ErrorAction SilentlyContinue
    }
    if ($stagedProduction) {
        Remove-Item -LiteralPath $ArtifactStaging -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $ManifestStaging -Force -ErrorAction SilentlyContinue
        [IO.File]::WriteAllBytes($DevelopmentKeyring, $originalKeyring)
    }
    if ($null -eq $oldOverride) {
        Remove-Item Env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING -ErrorAction SilentlyContinue
    } else {
        $env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING = $oldOverride
    }
}
