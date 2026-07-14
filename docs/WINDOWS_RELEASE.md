# Windows release and installer runbook

LiveBlock targets Windows 11 x64. Windows 10 compatibility remains a low-cost,
uncertified goal. A Tauri compile, unsigned installer, or `BuildOnly` package is
not distribution evidence.

## Credential-free preflight

From PowerShell on Windows after `npm ci` in `platform/_shared-frontend`:

```powershell
pwsh -File tools/release_windows.ps1 -Mode DryRun
```

The dry run validates repository tooling, the pinned Tauri CLI, and the strict
committed development keyring. It prints the MSI/NSIS build and Authenticode
commands without building, accessing certificates, signing, or timestamping.

## CI/build-only installers

```powershell
pwsh -File tools/release_windows.ps1 -Mode BuildOnly `
  -OutputDir tools/runs/windows-package-build-only
```

This mode is explicitly development-only. It sets the empty-keyring override,
refuses leftover staged model resources, builds one MSI and one NSIS installer,
copies them into a fixed evidence directory, emits basename-rerunnable SHA-256
lines, and creates/verifies a schema-1 artifact inventory. It also performs a
non-installing MSI administrative extraction and inventories the complete payload,
requiring the application executable, packaged ONNX Runtime DLL, and trusted-keyring
resource. The release manifest
must say:

```json
{
  "artifactType": "windows-installers-build-only",
  "promotedModelEmbedded": false,
  "signed": false,
  "timestamped": false
}
```

Do not distribute these installers. They contain the empty development trust
ring and no promoted model. Compilation/package construction is not proof of
installation, launch, DirectML execution, capture, update, or uninstall behavior.

## Credential-gated execution

Import a genuine code-signing certificate into the current user's certificate
store through the organization's secret-management process. Do not commit PFX
files, passwords, private keys, certificate exports, or token credentials.

Set only non-secret selectors/locations in the shell:

```powershell
$env:LB_WINDOWS_MODEL_BUNDLE_DIR = 'D:\protected\liveblock-model'
$env:LB_WINDOWS_CERTIFICATE_SHA1 = '<certificate thumbprint>'
$env:LB_WINDOWS_TIMESTAMP_URL = 'https://<trusted-rfc3161-service>'
pwsh -File tools/release_windows.ps1 -Mode Execute
```

The protected model directory must contain exactly the production inputs used by
the runtime:

```text
liveblock-model/
├── liveblock-detector.onnx
├── liveblock-detector.manifest.json
└── trusted-model-keys.json
```

Execute mode:

1. rejects the empty-keyring override and dirty tracked/untracked worktrees;
2. cryptographically verifies the exact ONNX artifact, schema-2 manifest, and
   nonempty schema-1 public keyring before staging;
3. restores/removes all temporary source-tree resources in `finally` cleanup;
4. builds MSI and NSIS packages from the reviewed commit;
5. signs and RFC-3161 timestamps both packages with SHA-256 using Windows SDK
   `signtool.exe` and a certificate-store thumbprint;
6. verifies each package with both `signtool verify /pa /all` and
   `Get-AuthenticodeSignature`;
7. administratively extracts the signed MSI without installing it, requires the
   executable, ONNX Runtime DLL, keyring, promoted ONNX, and signed manifest, then
   inventories/verifies that complete extracted payload;
8. hashes and inventories the final post-signing package bytes; and
9. records signed/timestamped/promoted-model flags and the full Git commit.

The script never accepts a PFX password or private-key value. CI signing should
use an ephemeral certificate store or hardware-backed provider exposed to
SignTool by the credential system.

## Update and installer boundaries

Model updates are offline authenticated transactions inside the installed app:
the embedded keyring authenticates schema-2 manifests, monotonic release
sequences prevent rollback, production ORT load occurs before commit, and startup
recovers interrupted swaps. Transporting a model update does not import a trust
root and is separate from updating the signed desktop application.

Tauri 2 currently produces MSI and NSIS for this adapter. MSIX creation, package
identity, App Installer/application-update signing, and downgrade/upgrade policy
for MSIX are **not implemented**. The checklist item covering signed MSI/MSIX and
secure application updates remains open until one reviewed distribution channel
is selected and clean-install/upgrade/uninstall tests run on real Windows.

## Required final evidence

Before distribution, preserve and seal at minimum:

- signed/timestamped MSI and chosen production installer format;
- package checksums, inventory, release manifest, certificate chain, and SignTool
  verification output;
- clean install, same-version repair, upgrade, downgrade rejection, uninstall,
  and user-data retention/removal decisions;
- offline/network-denied first launch and update behavior;
- Windows 11 x64 capture, mixed-DPI, hot-plug, sleep/lock, panic, WDA capture
  exclusion, CPU fallback, and AMD/Intel/NVIDIA runs; and
- exact promoted ONNX/schema-5/parity evidence and production nonempty keyring.

Absent credentials, promoted model inputs, or Windows devices remain blockers;
they must never be represented by `BuildOnly` evidence.
