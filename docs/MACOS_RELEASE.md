# macOS release runbook

LiveBlock targets macOS 26. A source build or self-signed development build is
**not** release evidence. A distributable is release-eligible only after the
Developer ID, notarization, stapling, Gatekeeper, test, and real-device checks
below complete with preserved artifacts.

## Credential-free preflight

```bash
tools/release_macos.sh --dry-run
```

This regenerates the Xcode project, validates Release build settings, checks the
required command-line tools, and prints the exact release commands. It does not
access a keychain, sign, upload, or claim notarization.

## One-time credential setup

Use genuine Apple Developer credentials. Do not store identities, passwords,
API keys, or private keys in this repository.

```bash
xcrun notarytool store-credentials LIVEBLOCK_NOTARY
export LB_DEVELOPER_ID_APPLICATION='Developer ID Application: Example (TEAMID)'
export LB_TEAM_ID='TEAMID'
export LB_NOTARY_PROFILE='LIVEBLOCK_NOTARY'
export LB_MACOS_MODEL_BUNDLE_DIR='/protected/release/liveblock-model'
```

`store-credentials` saves the secret in the login keychain. CI should use its
secret store and an ephemeral keychain instead.

## Build and notarize

Start from a clean, reviewed commit, then run:

```bash
tools/release_macos.sh --execute
```

The script fails closed unless all required environment variables are present
and the tree is clean, including untracked files. It:

1. archives Release with hardened runtime and timestamped Developer ID signing;
2. verifies the archived signature;
3. submits a zip to Apple and waits for the notarization result;
4. staples and validates the ticket;
5. performs a Gatekeeper assessment;
6. verifies and stages the protected precompiled CoreML artifact, promotion-bound
   manifest, and nonempty public keyring before Xcode generation;
7. creates the final post-stapling zip; and
8. writes SHA-256 and JSON release manifests under
   `tools/runs/release-macos/<UTC timestamp>/` (gitignored).

Preserve the archive, notarization JSON, final zip, checksum, manifest, command
log, and exact git commit as release evidence. Never infer signing or
notarization success from a dry run.

## Authenticated detector updates

Release builds load CoreML only through `MacModelDistribution`. An offline update
package selected with **Install signed update** must contain exactly the expected
artifacts (additional transport metadata may be outside this directory):

```text
liveblock-detector-update/
├── liveblock-detector.manifest.json
└── liveblock-detector.mlmodelc/
```

The app never accepts a key from that package. It verifies manifest schema 2
against `trusted-model-keys.json` embedded in the signed app, checks the complete
tree hash and monotonic release sequence, copies without trusting symlinks,
loads through production CoreML, fsyncs, atomically exchanges directories, and
commits signed schema-1 state. Startup reauthenticates and recovers an interrupted
swap before inference. A rejected accepted update never falls back to an older
bundle; a newer authenticated packaged release is the only baseline override.

The committed keyring is deliberately empty. `tools/release_macos.sh --execute`
and the Xcode Release build phase require `LB_MACOS_MODEL_BUNDLE_DIR` containing
`liveblock-detector.mlmodelc`, `liveblock-detector.manifest.json`, and
`trusted-model-keys.json`; execute mode cryptographically verifies the complete
bundle, stages it under ignored `release-*` resource paths before `xcodegen`,
and removes staging on exit. `LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1` is only for
unsigned source/CI compilation and is forbidden by execute mode. Update packages must distribute the precompiled `.mlmodelc` measured by
schema-5. Xcode also compiles bundled `.mlpackage` content to `.mlmodelc`; that
compiled directory requires its own exact schema-5/parity
fingerprint and cannot inherit the source package's signature.

Debug builds retain direct developer candidate loading for source training. This
code path is compiled out of Release and is not distribution evidence.

## Final macOS release gates

On clean macOS installations, verify at minimum:

- first-launch Gatekeeper acceptance and application identity;
- Screen Recording and Accessibility onboarding, denial, grant, revocation,
  and recovery;
- one-, two-, and three-display targeting, negative origins, mixed scaling,
  hot-plug, sleep/wake, lock/unlock, Spaces, and fullscreen transitions;
- panic disable and quit while active and suspended;
- sustained capture/inpaint soak within the limits in
  `DESKTOP_RELEASE_MATRIX.md`;
- VoiceOver, keyboard navigation, contrast, reduced motion, and text scaling;
- schema-5 detector promotion and fingerprint-bound installation; and
- exported diagnostics contain no frames, process/window names, labels,
  regions, or user paths.

Record hardware model, GPU, display topology, OS build, test commit, timestamps,
and artifacts. Missing hardware or credentials remains an explicit blocker.
