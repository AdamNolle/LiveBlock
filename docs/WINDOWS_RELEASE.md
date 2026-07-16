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

1. requires that no LiveBlock uninstall registration exists and validates a
   separately inventoried unsigned `0.0.9` package-mechanics fixture;
2. silently installs the prior MSI, upgrades it to current `0.1.0`, verifies a
   single current registration, runs same-version `/fa` repair, requires the
   prior MSI downgrade attempt to fail while current remains registered, then
   inventories and reverifies the installed executable/runtime/license/notices/
   keyring tree, parses the schema-1 empty development keyring, loads and frees
   the packaged `onnxruntime.dll` through .NET's native-library API, and requires
   the UI process to remain alive for a bounded 12-second window;
3. force-stops that process, silently uninstalls the current MSI, and waits for
   both the payload and uninstall registration to disappear;
4. repeats prior install, current upgrade, and same-version reinstall through
   the NSIS current-user installer, requires the older silent installer to exit
   with the pinned guard's code `2` while current remains registered, then
   inventories, bounded-launches, and uninstalls current; and
5. preserves verbose MSI logs, NSIS stdout/stderr, launch logs, a progress
   journal, both installed-payload inventories, and a machine-readable summary.

The test has `finally` cleanup and the artifact upload runs even after failure so
partial diagnostics remain available. The payload checks are deliberately
separate from the window-subsystem process, whose stdout/stderr are not a stable
release-mode attestation channel. This proves installer mechanics, packaged DLL
loadability, the expected BuildOnly trust payload, bounded UI startup, and MSI
package-manager transitions only on the hosted Windows image. The `0.0.9`
fixture changes installer metadata while intentionally using the same source
payload; it proves mechanics, not compatibility with a historical release.
A create-new sentinel under `%APPDATA%\LiveBlock` must survive every MSI/NSIS
transition and both uninstalls. Run `29398897155` proved Tauri 2.11.1's stock
silent NSIS path accepted the older fixture despite `allowDowngrades: false`.
LiveBlock therefore pins the exact upstream template at commit
`e5ae5b93cdd310045191cc0526f253140ad64b87` and adds one hash-checked `.onInit`
guard that reads the registered version before any uninstall or payload
mutation. It allows upgrades/reinstalls, but rejects older or malformed incoming
comparisons for silent and interactive entry points. Production distribution
still requires this lifecycle against genuinely signed packages and a shipped
historical payload. BuildOnly fixture success does not prove signatures,
promoted-model authentication, DirectML/GPU execution, capture, anti-cheat,
mixed-DPI, lifecycle recovery, accessibility, application-update transport, or
sustained use.

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

## Selected application-update channel

The selected stable Windows application-update channel is a **stable staged NSIS
GitHub Release**. LiveBlock does not auto-download application updates, execute a
background updater, expose a frontend network API, or import publisher trust
roots. A user or managed deployment system obtains the complete release bundle
outside the app, then verifies the local bytes before running the installer. MSI
remains available for managed initial deployment; it is not the application-
update channel. MSIX/App Installer is not a second implicit channel and remains
unimplemented.

Production `release_windows.ps1 -Mode Execute` creates the staged bundle only
after the NSIS installer has a valid Authenticode signature and RFC-3161
timestamp. The same certificate produces a create-new detached CMS/SHA-256
signature, `windows-application-update.p7s`, over the exact descriptor bytes, so
the installer, SBOM, and obligation hashes cannot be rewritten independently.
`windows-application-update.json` uses closed schema 1 from
`contracts/windows-application-update.schema.json` and binds:

- exact stable version, full Git commit, `desktop-v<version>` tag, repository,
  Windows x86_64 platform, and `stable-staged-nsis` channel;
- exact NSIS basename, size, and SHA-256;
- Authenticode publisher subject, signer-certificate SHA-256, and timestamp-
  certificate SHA-256; and
- exact SBOM, dependency-license report, verified obligation report, component-
  bound license decisions, MPL source offer, and MPL-2.0 license bytes.

The bundle rejects unknown/future fields, missing/extra files, nested paths,
links, special files, mutation, wrong hashes/sizes, invalid/multiple detached CMS
signers, disagreement between CMS and Authenticode identity, or a signer
certificate that does not match an independently supplied trusted SHA-256. Run the offline Windows
verifier before installation:

```powershell
pwsh -File tools/verify_windows_application_update.ps1 `
  -BundleDir D:\staged\LiveBlock-0.2.0 `
  -ExpectedSignerCertificateSha256 '<sha256 from approved publisher policy>' `
  -CurrentVersion 0.1.0
```

The expected signer fingerprint must come from the approved release policy or a
previously authenticated release, never from the untrusted descriptor alone.
The verifier reruns byte/schema closure and detached CMS verification, then
requires Windows Authenticode status `Valid`, an exact shared CMS/installer
signer fingerprint and subject, the code-signing EKU, an exact
timestamp-certificate fingerprint, exact agreement with the Authenticode-covered
installer file version, and a candidate version newer than the supplied current
version. It performs no network request, install, download, or
trust-root mutation. Initial installation may omit `-CurrentVersion`, but still
requires the independent signer fingerprint and every other check.

Publishing is create-new operator work: attach the complete, already verified
bundle to GitHub repository `AdamNolle/LiveBlock` under the exact
`desktop-v<version>` tag for the recorded commit. Never overwrite an existing tag
or asset. GitHub transport is not the signing authority; Authenticode plus the
independently pinned signer is. Certificate rotation requires a separately
reviewed publisher-policy update before accepting the new fingerprint.

This selects and implements the credential-gated build/verification contract; it
does not claim a production release. Genuine certificate custody, timestamping,
publication, signed prior-to-current execution, accountable dependency review,
and real Windows-device validation remain required. BuildOnly packages cannot
produce a staged update bundle and are not channel artifacts.

Model updates remain separate offline authenticated transactions inside the app:
the embedded model keyring authenticates schema-2 manifests, monotonic sequences
prevent rollback, production ORT load occurs before commit, and startup recovers
interrupted swaps. A model update cannot authorize an application update or
import an application publisher trust root.

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
