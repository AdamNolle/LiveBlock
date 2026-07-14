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
refuses leftover staged model resources, stages the official ONNX Runtime DirectML
1.18.1 NuGet package only after verifying archive SHA-256
`51273348e0edc53d50a68fdddf29f142cdb9eca5c688d0233334c3295ebae595`,
validates the DLL as x86_64 PE32+, and preserves its license and complete notices.
It then builds one MSI and one NSIS installer, copies them into a fixed evidence
directory, emits basename-rerunnable SHA-256 lines, and creates/verifies a schema-1
artifact inventory. It also performs a non-installing MSI administrative extraction
and inventories the complete payload, requiring the application executable,
packaged ONNX Runtime DLL/license/notices, and trusted-keyring resource. The release manifest
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
ring and no promoted model. Package construction alone is not proof of runtime
behavior. Native hosted CI separately exercises the exact BuildOnly bytes as
described below; that smoke evidence still is not production distribution or
hardware certification.

### Hosted build-only lifecycle smoke

After package and payload inventories pass, the Windows 11 hosted runner executes
`tools/test_windows_package_lifecycle.ps1` against only an unsigned,
untimestamped, no-model `windows-installers-build-only` manifest. The script:

1. requires that no LiveBlock uninstall registration exists;
2. silently installs the MSI, inventories and reverifies the installed
   executable/runtime/license/notices/keyring tree, and requires the UI process
   to remain alive for a bounded 12-second window;
3. force-stops that process, silently uninstalls the MSI, and waits for both the
   payload and uninstall registration to disappear;
4. repeats clean install, installed-payload inventory, bounded launch, and
   uninstall through the NSIS current-user installer; and
5. preserves verbose MSI logs, NSIS stdout/stderr, launch logs, a progress
   journal, both installed-payload inventories, and a machine-readable summary.

The test has `finally` cleanup and the artifact upload runs even after failure so
partial diagnostics remain available. It proves installer mechanics and bounded
UI startup only on the hosted Windows image. It does not prove signatures,
promoted-model authentication, DirectML/GPU execution, capture, anti-cheat,
mixed-DPI, lifecycle recovery, accessibility, same-version repair, prior-version
upgrade, application updates, or sustained use.

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
is selected, genuine signed packages run, prior-version upgrade/repair policy is
tested, and the real Windows device matrix passes. Hosted BuildOnly MSI/NSIS
clean-install/launch/uninstall smoke does not satisfy those production gates.

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
