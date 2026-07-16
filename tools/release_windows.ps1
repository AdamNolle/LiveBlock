[CmdletBinding()]
param(
    [ValidateSet("DryRun", "BuildOnly", "Execute")]
    [string]$Mode = "DryRun",
    [string]$OutputDir = "",
    [string]$BuildOnlyFixtureVersion = "",
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
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or $item.LinkType) { throw "Package output must be a regular non-symlink file: $Path" }
        $contentHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
        $fingerprint = "$($item.Length):$($item.LastWriteTimeUtc.Ticks):$contentHash"
        if ($fingerprint -eq $previous) {
            $stableSamples++
            if ($stableSamples -ge 8) { return }
        } else {
            $stableSamples = 0
            $previous = $fingerprint
        }
        Start-Sleep -Seconds 1
    }
    throw "Package output did not become stable within 30 seconds: $Path"
}

function Get-CertificateSha256([Security.Cryptography.X509Certificates.X509Certificate2]$Certificate) {
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return ([Convert]::ToHexString($sha256.ComputeHash($Certificate.RawData))).ToLowerInvariant()
    } finally {
        $sha256.Dispose()
    }
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

$TauriConfig = Join-Path $Root "platform\windows\src-tauri\tauri.conf.json"
$currentPackageVersion = [Version]((Get-Content -LiteralPath $TauriConfig -Raw | ConvertFrom-Json).version)
$fixtureVersion = $null
if ($BuildOnlyFixtureVersion) {
    if ($Mode -ne "BuildOnly") {
        throw "BuildOnlyFixtureVersion is allowed only in BuildOnly mode"
    }
    try { $fixtureVersion = [Version]$BuildOnlyFixtureVersion } catch {
        throw "BuildOnlyFixtureVersion must be a numeric dotted version"
    }
    if ($fixtureVersion -ge $currentPackageVersion) {
        throw "BuildOnlyFixtureVersion must be lower than current package version $currentPackageVersion"
    }
}

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
$DependencyEvidenceDir = Join-Path $OutputDir "dependency-evidence"
$UpdateBundleDir = Join-Path $OutputDir "windows-application-update"
$UpdateBundleInventory = Join-Path $OutputDir "windows-application-update-inventory.json"

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
if ($fixtureVersion) { $buildCommand += " --config '{`"version`":`"$fixtureVersion`"}'" }
if ($Mode -eq "DryRun") {
    $signToolDisplay = "<Windows SDK signtool.exe>"
    Write-Host "Windows release dry-run passed local preflight."
    Write-Host "No package, signing, timestamp, or release artifact was produced."
    Write-Host "Planned output: $OutputDir"
    Write-Host "Planned build: $buildCommand"
    Write-Host "Planned signing: $signToolDisplay sign /fd SHA256 /sha1 <certificate thumbprint> /tr <HTTPS timestamp URL> /td SHA256 <MSI and NSIS packages>"
    Write-Host "Planned verification: $signToolDisplay verify /pa /all /v <each package>"
    Write-Host "Planned stable channel: staged NSIS GitHub Release bundle with offline Authenticode, byte, version, SBOM, and dependency-obligation verification"
    exit 0
}

if (Test-Path -LiteralPath $OutputDir) {
    throw "Output directory already exists; Windows package evidence is create-new"
}

$originalKeyring = [IO.File]::ReadAllBytes($DevelopmentKeyring)
$stagedProduction = $false
$stagedRuntime = $false
$fixtureConfigOverride = $null
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
    $TargetRoot = Join-Path $Root "platform\windows\target"
    if (Test-Path -LiteralPath $TargetRoot -PathType Container) {
        Get-ChildItem -LiteralPath $TargetRoot -Directory -Recurse |
            Where-Object { $_.Name -in @("msi", "nsis") -and $_.Parent.Name -eq "bundle" } |
            Remove-Item -Recurse -Force
    }
    $tauriArguments = @("build", "--bundles", "msi,nsis", "--ci")
    if ($fixtureVersion) {
        $fixtureConfigOverride = Join-Path ([IO.Path]::GetTempPath()) "liveblock-windows-fixture-$PID.json"
        $fixtureJson = @{ version = $fixtureVersion.ToString() } | ConvertTo-Json -Compress
        $fixtureBytes = [Text.UTF8Encoding]::new($false).GetBytes($fixtureJson)
        $fixtureStream = [IO.File]::Open($fixtureConfigOverride, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $fixtureStream.Write($fixtureBytes, 0, $fixtureBytes.Length)
            $fixtureStream.Flush($true)
        } finally { $fixtureStream.Dispose() }
        $tauriArguments += @("--config", $fixtureConfigOverride)
    }
    Push-Location (Join-Path $Root "platform\windows\src-tauri")
    try { & $Tauri @tauriArguments } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "Tauri Windows bundle build failed" }

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
    $artifactType = if ($Mode -eq "Execute") {
        "windows-installers-signed"
    } elseif ($fixtureVersion) {
        "windows-installers-version-fixture-build-only"
    } else {
        "windows-installers-build-only"
    }
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
        applicationUpdateChannel = "stable-staged-nsis"
        applicationUpdateBundleProduced = ($Mode -eq "Execute")
        packageFixtureVersion = if ($fixtureVersion) { $fixtureVersion.ToString() } else { $null }
        packages = @($copied | Sort-Object Name | ForEach-Object { $_.Name })
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDir "release-manifest.json") -Encoding utf8NoBOM

    if ($Mode -eq "Execute") {
        New-Item -ItemType Directory -Path $DependencyEvidenceDir | Out-Null
        $coreMetadata = Join-Path $DependencyEvidenceDir "core-metadata.json"
        $windowsMetadata = Join-Path $DependencyEvidenceDir "windows-metadata.json"
        $linuxMetadata = Join-Path $DependencyEvidenceDir "linux-metadata.json"
        cargo metadata --manifest-path core/Cargo.toml --locked --format-version 1 |
            Set-Content -LiteralPath $coreMetadata -Encoding utf8NoBOM
        if ($LASTEXITCODE -ne 0) { throw "Core dependency metadata generation failed" }
        cargo metadata --manifest-path platform/windows/src-tauri/Cargo.toml --locked --format-version 1 |
            Set-Content -LiteralPath $windowsMetadata -Encoding utf8NoBOM
        if ($LASTEXITCODE -ne 0) { throw "Windows dependency metadata generation failed" }
        cargo metadata --manifest-path platform/linux/src-tauri/Cargo.toml --locked --format-version 1 |
            Set-Content -LiteralPath $linuxMetadata -Encoding utf8NoBOM
        if ($LASTEXITCODE -ne 0) { throw "Linux dependency metadata generation failed" }

        $sbom = Join-Path $DependencyEvidenceDir "liveblock.cdx.json"
        $licenseReport = Join-Path $DependencyEvidenceDir "dependency-licenses.json"
        $obligationReport = Join-Path $DependencyEvidenceDir "dependency-obligations.json"
        python tools/release_evidence.py sbom `
            --cargo-metadata $coreMetadata `
            --cargo-metadata $windowsMetadata `
            --cargo-metadata $linuxMetadata `
            --npm-lock platform/_shared-frontend/package-lock.json `
            --license-decisions licenses/dependency-license-decisions.json `
            --output $sbom `
            --license-report $licenseReport `
            --commit $commit
        if ($LASTEXITCODE -ne 0) { throw "Production dependency SBOM/license generation failed" }
        python tools/verify_dependency_obligations.py `
            --license-report $licenseReport `
            --license-decisions licenses/dependency-license-decisions.json `
            --source-offer licenses/mpl-source-offer.json `
            --cargo-lock core/Cargo.lock `
            --cargo-lock platform/windows/Cargo.lock `
            --cargo-lock platform/linux/Cargo.lock `
            --output $obligationReport
        if ($LASTEXITCODE -ne 0) { throw "Production dependency-obligation verification failed" }

        New-Item -ItemType Directory -Path $UpdateBundleDir | Out-Null
        $nsis = $copied | Where-Object Extension -eq ".exe" | Select-Object -First 1
        $updateInstaller = Copy-Item -LiteralPath $nsis.FullName -Destination (Join-Path $UpdateBundleDir $nsis.Name) -PassThru
        $updateSbom = Copy-Item -LiteralPath $sbom -Destination (Join-Path $UpdateBundleDir "liveblock.cdx.json") -PassThru
        $updateLicenseReport = Copy-Item -LiteralPath $licenseReport -Destination (Join-Path $UpdateBundleDir "dependency-licenses.json") -PassThru
        $updateObligationReport = Copy-Item -LiteralPath $obligationReport -Destination (Join-Path $UpdateBundleDir "dependency-obligations.json") -PassThru
        $updateLicenseDecisions = Copy-Item -LiteralPath "licenses/dependency-license-decisions.json" -Destination (Join-Path $UpdateBundleDir "dependency-license-decisions.json") -PassThru
        $updateMplSourceOffer = Copy-Item -LiteralPath "licenses/mpl-source-offer.json" -Destination (Join-Path $UpdateBundleDir "mpl-source-offer.json") -PassThru
        $updateMplLicense = Copy-Item -LiteralPath "licenses/MPL-2.0.txt" -Destination (Join-Path $UpdateBundleDir "MPL-2.0.txt") -PassThru

        $updateSignature = Get-AuthenticodeSignature -LiteralPath $updateInstaller.FullName
        if ($updateSignature.Status -ne "Valid" -or -not $updateSignature.SignerCertificate -or -not $updateSignature.TimeStamperCertificate) {
            throw "Staged NSIS update requires a valid Authenticode signer and RFC-3161 timestamp"
        }
        $signerCertificateSha256 = Get-CertificateSha256 $updateSignature.SignerCertificate
        $timestampCertificateSha256 = Get-CertificateSha256 $updateSignature.TimeStamperCertificate
        python tools/windows_application_update.py create `
            --bundle $UpdateBundleDir `
            --version ($currentPackageVersion.ToString()) `
            --commit $commit `
            --publisher-subject ($updateSignature.SignerCertificate.Subject) `
            --certificate-sha256 $signerCertificateSha256 `
            --timestamp-certificate-sha256 $timestampCertificateSha256 `
            --installer $updateInstaller.FullName `
            --sbom $updateSbom.FullName `
            --license-report $updateLicenseReport.FullName `
            --obligation-report $updateObligationReport.FullName `
            --license-decisions $updateLicenseDecisions.FullName `
            --mpl-source-offer $updateMplSourceOffer.FullName `
            --mpl-license $updateMplLicense.FullName
        if ($LASTEXITCODE -ne 0) { throw "Staged Windows application-update descriptor creation failed" }

        $certificateThumbprint = $env:LB_WINDOWS_CERTIFICATE_SHA1.Replace(" ", "")
        $descriptorSigningCertificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$certificateThumbprint" -ErrorAction Stop
        if ((Get-CertificateSha256 $descriptorSigningCertificate) -ne $signerCertificateSha256) {
            throw "Descriptor-signing certificate does not match the Authenticode signer"
        }
        $updateDescriptorPath = Join-Path $UpdateBundleDir "windows-application-update.json"
        $updateDescriptorSignaturePath = Join-Path $UpdateBundleDir "windows-application-update.p7s"
        $contentInfo = [Security.Cryptography.Pkcs.ContentInfo]::new([IO.File]::ReadAllBytes($updateDescriptorPath))
        $detachedCms = [Security.Cryptography.Pkcs.SignedCms]::new($contentInfo, $true)
        $cmsSigner = [Security.Cryptography.Pkcs.CmsSigner]::new($descriptorSigningCertificate)
        $cmsSigner.DigestAlgorithm = [Security.Cryptography.Oid]::new("2.16.840.1.101.3.4.2.1")
        $cmsSigner.IncludeOption = [Security.Cryptography.X509Certificates.X509IncludeOption]::EndCertOnly
        $detachedCms.ComputeSignature($cmsSigner)
        $encodedCms = $detachedCms.Encode()
        $cmsStream = [IO.File]::Open($updateDescriptorSignaturePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $cmsStream.Write($encodedCms, 0, $encodedCms.Length)
            $cmsStream.Flush($true)
        } finally { $cmsStream.Dispose() }

        & (Join-Path $Root "tools\verify_windows_application_update.ps1") `
            -BundleDir $UpdateBundleDir `
            -ExpectedSignerCertificateSha256 $signerCertificateSha256
        python tools/release_evidence.py inventory `
            --root $UpdateBundleDir `
            --output $UpdateBundleInventory `
            --platform windows-x86_64 `
            --artifact-type windows-stable-staged-nsis-update `
            --commit $commit `
            --require windows-application-update.json `
            --require windows-application-update.p7s `
            --require ($updateInstaller.Name) `
            --require liveblock.cdx.json `
            --require dependency-licenses.json `
            --require dependency-obligations.json `
            --require dependency-license-decisions.json `
            --require mpl-source-offer.json `
            --require MPL-2.0.txt
        if ($LASTEXITCODE -ne 0) { throw "Staged Windows application-update inventory creation failed" }
        python tools/release_evidence.py verify --root $UpdateBundleDir --manifest $UpdateBundleInventory
        if ($LASTEXITCODE -ne 0) { throw "Staged Windows application-update inventory verification failed" }
    }

    Write-Host "Verified Windows package evidence: $OutputDir"
} finally {
    if ($fixtureConfigOverride) {
        Remove-Item -LiteralPath $fixtureConfigOverride -Force -ErrorAction SilentlyContinue
    }
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
