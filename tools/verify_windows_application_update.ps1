[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{64}$')]
    [string]$ExpectedSignerCertificateSha256,
    [string]$CurrentVersion = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Windows application-update verification requires Windows" }

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BundleDir = (Resolve-Path -LiteralPath $BundleDir).Path
$descriptorPath = Join-Path $BundleDir "windows-application-update.json"
$descriptorItem = Get-Item -LiteralPath $descriptorPath -Force -ErrorAction Stop
if ($descriptorItem.PSIsContainer -or $descriptorItem.LinkType -or ($descriptorItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Update descriptor must be a regular non-reparse file"
}

function Get-CertificateSha256([Security.Cryptography.X509Certificates.X509Certificate2]$Certificate) {
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return ([Convert]::ToHexString($sha256.ComputeHash($Certificate.RawData))).ToLowerInvariant()
    } finally {
        $sha256.Dispose()
    }
}

$expectedCertificate = $ExpectedSignerCertificateSha256.ToLowerInvariant()
python (Join-Path $Root "tools\windows_application_update.py") verify `
    --bundle $BundleDir `
    --expected-certificate-sha256 $expectedCertificate
if ($LASTEXITCODE -ne 0) { throw "Update byte/schema verification failed" }

$detachedSignaturePath = Join-Path $BundleDir "windows-application-update.p7s"
$detachedSignatureItem = Get-Item -LiteralPath $detachedSignaturePath -Force -ErrorAction Stop
if ($detachedSignatureItem.PSIsContainer -or $detachedSignatureItem.LinkType -or ($detachedSignatureItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Detached update signature must be a regular non-reparse file"
}
$contentInfo = [Security.Cryptography.Pkcs.ContentInfo]::new([IO.File]::ReadAllBytes($descriptorPath))
$detachedCms = [Security.Cryptography.Pkcs.SignedCms]::new($contentInfo, $true)
try {
    $detachedCms.Decode([IO.File]::ReadAllBytes($detachedSignaturePath))
    $detachedCms.CheckSignature($true)
} catch {
    throw "Detached CMS descriptor signature verification failed: $($_.Exception.Message)"
}
if ($detachedCms.SignerInfos.Count -ne 1 -or -not $detachedCms.SignerInfos[0].Certificate) {
    throw "Detached descriptor must have exactly one embedded signer certificate"
}
$cmsSignerCertificate = $detachedCms.SignerInfos[0].Certificate
$cmsSignerSha256 = Get-CertificateSha256 $cmsSignerCertificate
if ($cmsSignerSha256 -ne $expectedCertificate) { throw "Detached descriptor signer does not match the trusted expected certificate" }

$descriptor = Get-Content -LiteralPath $descriptorPath -Raw -Encoding utf8 | ConvertFrom-Json
if ($cmsSignerSha256 -ne $descriptor.authenticode.certificateSha256) { throw "Detached descriptor signer does not match descriptor identity" }
if ($cmsSignerCertificate.Subject -cne $descriptor.authenticode.publisherSubject) { throw "Detached descriptor publisher subject mismatch" }
$installerPath = Join-Path $BundleDir $descriptor.artifacts.installer.fileName
$installer = Get-Item -LiteralPath $installerPath -Force -ErrorAction Stop
if ($installer.PSIsContainer -or $installer.LinkType -or ($installer.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Update installer must be a regular non-reparse file"
}

$signature = Get-AuthenticodeSignature -LiteralPath $installerPath
if ($signature.Status -ne "Valid") { throw "Authenticode status must be Valid; found $($signature.Status)" }
if (-not $signature.SignerCertificate) { throw "Authenticode signer certificate is missing" }
if (-not $signature.TimeStamperCertificate) { throw "RFC-3161 timestamp certificate is missing" }

$signerSha256 = Get-CertificateSha256 $signature.SignerCertificate
$timestampSha256 = Get-CertificateSha256 $signature.TimeStamperCertificate
if ($signerSha256 -ne $expectedCertificate) { throw "Authenticode signer does not match the trusted expected certificate" }
if ($signerSha256 -ne $descriptor.authenticode.certificateSha256) { throw "Authenticode signer does not match the descriptor" }
if ($timestampSha256 -ne $descriptor.authenticode.timestampCertificateSha256) { throw "Timestamp signer does not match the descriptor" }
if ($signature.SignerCertificate.Subject -cne $descriptor.authenticode.publisherSubject) { throw "Publisher subject does not match the descriptor" }

try {
    $candidate = [Version]$descriptor.version
    $signedFileVersion = [Version]([Diagnostics.FileVersionInfo]::GetVersionInfo($installerPath).FileVersion)
} catch {
    throw "Descriptor and signed installer file versions must be numeric dotted versions"
}
if ($signedFileVersion.Major -ne $candidate.Major -or
    $signedFileVersion.Minor -ne $candidate.Minor -or
    $signedFileVersion.Build -ne $candidate.Build) {
    throw "Descriptor version does not match the Authenticode-covered installer file version"
}

$codeSigningOid = "1.3.6.1.5.5.7.3.3"
$hasCodeSigningEku = $false
foreach ($extension in $signature.SignerCertificate.Extensions) {
    if ($extension -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]) {
        foreach ($oid in $extension.EnhancedKeyUsages) {
            if ($oid.Value -eq $codeSigningOid) { $hasCodeSigningEku = $true }
        }
    }
}
if (-not $hasCodeSigningEku) { throw "Signer certificate does not declare the code-signing EKU" }

if ($CurrentVersion) {
    try {
        $current = [Version]$CurrentVersion
    } catch {
        throw "Current and candidate versions must be numeric dotted versions"
    }
    if ($candidate -le $current) {
        throw "Staged update must be newer than current version $current; found $candidate"
    }
}

Write-Host "Verified staged Windows NSIS update $($descriptor.version) from $($descriptor.source.tag)"
Write-Host "No network request, install, or trust-root import was performed."
