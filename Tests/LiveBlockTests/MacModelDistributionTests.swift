import XCTest
import CryptoKit
import CoreML
import Darwin
@testable import LiveBlock

final class MacModelDistributionTests: XCTestCase {
    private var roots: [URL] = []

    override func tearDown() {
        for root in roots { try? FileManager.default.removeItem(at: root) }
        roots.removeAll()
        super.tearDown()
    }

    private func temporaryRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("liveblock-mac-update-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        roots.append(root)
        return root
    }

    private func makeArtifact(at url: URL, marker: String = "X") throws {
        try FileManager.default.createDirectory(
            at: url.appendingPathComponent("nested", isDirectory: true),
            withIntermediateDirectories: true
        )
        try Data(marker.utf8).write(to: url.appendingPathComponent("a.txt"))
        try Data("Y".utf8).write(to: url.appendingPathComponent("nested/b.bin"))
    }

    private func writeKeyring(_ key: Curve25519.Signing.PrivateKey, to url: URL) throws {
        let object: [String: Any] = [
            "schemaVersion": 1,
            "keys": [[
                "keyId": "test-release",
                "publicKeyBase64": key.publicKey.rawRepresentation.base64EncodedString()
            ]]
        ]
        try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
            .write(to: url)
    }

    private func signedManifest(
        artifact: URL,
        key: Curve25519.Signing.PrivateKey,
        sequence: UInt64
    ) throws -> MacModelManifest {
        var manifest = MacModelManifest(
            schemaVersion: 2,
            modelId: "liveblock-detector",
            modelVersion: "1.0.\(sequence)",
            artifactFormat: "coreml",
            artifactHashAlgorithm: "sha256-file-or-tree-v1",
            artifactSha256: try MacArtifactHash.sha256(artifact),
            runtimeClasses: ["Logo", "Ad banner", "Sponsored"],
            inputWidth: 640,
            inputHeight: 640,
            nmsEmbedded: true,
            releaseSequence: sequence,
            promotionGateSchema: 5,
            promotionReportSha256: String(repeating: "a", count: 64),
            createdAt: "2026-07-13T00:00:00Z",
            keyId: "test-release",
            signature: "pending"
        )
        manifest.signature = try key.signature(for: manifest.signingBytes()).base64EncodedString()
        return manifest
    }

    private func writeManifest(_ manifest: MacModelManifest, to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        try encoder.encode(manifest).write(to: url)
    }

    private func makeUpdate(
        root: URL,
        key: Curve25519.Signing.PrivateKey,
        sequence: UInt64,
        marker: String
    ) throws -> URL {
        let update = root.appendingPathComponent("update-\(sequence)", isDirectory: true)
        try FileManager.default.createDirectory(at: update, withIntermediateDirectories: true)
        let artifact = update.appendingPathComponent("liveblock-detector.mlmodelc", isDirectory: true)
        try makeArtifact(at: artifact, marker: marker)
        try writeManifest(
            signedManifest(artifact: artifact, key: key, sequence: sequence),
            to: update.appendingPathComponent("liveblock-detector.manifest.json")
        )
        return update
    }

    private func environment(
        root: URL,
        keyring: URL,
        packagedUpdate: URL? = nil
    ) -> MacModelDistributionEnvironment {
        MacModelDistributionEnvironment(
            modelsDirectory: root.appendingPathComponent("models", isDirectory: true),
            keyringURL: keyring,
            packagedArtifactURL: packagedUpdate?.appendingPathComponent("liveblock-detector.mlmodelc"),
            packagedManifestURL: packagedUpdate?.appendingPathComponent("liveblock-detector.manifest.json")
        )
    }

    func testTreeHashMatchesPythonAndRustFixture() throws {
        let root = try temporaryRoot()
        let artifact = root.appendingPathComponent("fixture", isDirectory: true)
        try makeArtifact(at: artifact)
        XCTAssertEqual(
            try MacArtifactHash.sha256(artifact),
            "8961fa489500947939aca204c59fff6f27f19e0fabb49cab47e2d0158efcf134"
        )
    }

    func testKeyringRejectsFractionalSchemaAndDuplicateIDs() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let encoded = key.publicKey.rawRepresentation.base64EncodedString()
        let keyring = root.appendingPathComponent("keys.json")
        try JSONSerialization.data(withJSONObject: ["schemaVersion": 1.5, "keys": []])
            .write(to: keyring)
        XCTAssertThrowsError(try MacTrustedKeyring.load(from: keyring, requireNonempty: false))
        try JSONSerialization.data(withJSONObject: [
            "schemaVersion": 1,
            "keys": [
                ["keyId": "duplicate", "publicKeyBase64": encoded],
                ["keyId": "duplicate", "publicKeyBase64": encoded]
            ]
        ]).write(to: keyring)
        XCTAssertThrowsError(try MacTrustedKeyring.load(from: keyring))
    }

    func testVerifiesPythonGeneratedSignatureFixture() throws {
        let root = try temporaryRoot()
        let artifact = root.appendingPathComponent("fixture", isDirectory: true)
        try makeArtifact(at: artifact)
        let publicKey = try Curve25519.Signing.PublicKey(rawRepresentation: XCTUnwrap(
            Data(base64Encoded: "A6EHv/POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg=")
        ))
        let manifest = MacModelManifest(
            schemaVersion: 2,
            modelId: "liveblock-detector",
            modelVersion: "fixture-1",
            artifactFormat: "coreml",
            artifactHashAlgorithm: "sha256-file-or-tree-v1",
            artifactSha256: "8961fa489500947939aca204c59fff6f27f19e0fabb49cab47e2d0158efcf134",
            runtimeClasses: ["Logo", "Ad banner", "Sponsored"],
            inputWidth: 640,
            inputHeight: 640,
            nmsEmbedded: true,
            releaseSequence: 7,
            promotionGateSchema: 5,
            promotionReportSha256: String(repeating: "ab", count: 32),
            createdAt: "2026-07-13T00:00:00Z",
            keyId: "fixture",
            signature: "PEh891Ttqf7YmhyrHYX/G8C0pGHwAprcpDoR6S4w5dCSarlEN/vM6+zVlwsgn7S9xzDf2fGrTcK+g9bgTLwoDw=="
        )
        try MacTrustedKeyring(keys: ["fixture": publicKey]).verify(manifest, artifact: artifact)
    }

    func testStrictManifestAndKeyringVerifySignatureAndArtifact() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let artifact = root.appendingPathComponent("model.mlmodelc", isDirectory: true)
        let keyringURL = root.appendingPathComponent("keys.json")
        let manifestURL = root.appendingPathComponent("manifest.json")
        try makeArtifact(at: artifact)
        try writeKeyring(key, to: keyringURL)
        let manifest = try signedManifest(artifact: artifact, key: key, sequence: 1)
        try writeManifest(manifest, to: manifestURL)
        let loaded = try MacModelManifest.load(from: manifestURL)
        let ring = try MacTrustedKeyring.load(from: keyringURL)
        try ring.verify(loaded, artifact: artifact)
        var changedSignature = loaded
        changedSignature.signature = Data(repeating: 0, count: 64).base64EncodedString()
        XCTAssertThrowsError(try ring.verify(changedSignature, artifact: artifact))
        let wrongKey = Curve25519.Signing.PrivateKey().publicKey
        XCTAssertThrowsError(
            try MacTrustedKeyring(keys: ["test-release": wrongKey]).verify(loaded, artifact: artifact)
        )
        try Data("tampered".utf8).write(to: artifact.appendingPathComponent("a.txt"))
        XCTAssertThrowsError(try ring.verify(loaded, artifact: artifact))

        var object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: manifestURL)) as? [String: Any]
        )
        object["unexpected"] = true
        try JSONSerialization.data(withJSONObject: object).write(to: manifestURL)
        XCTAssertThrowsError(try MacModelManifest.load(from: manifestURL))
    }

    func testRealCompiledCoreMLUpdateLoadsWithProductionValidator() async throws {
        let repository = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let source = repository.appendingPathComponent("Sources/liveblock-detector.mlpackage")
        let compiled = try await Task.detached {
            try MLModel.compileModel(at: source)
        }.value
        let root = try temporaryRoot()
        let update = root.appendingPathComponent("compiled-update", isDirectory: true)
        try FileManager.default.createDirectory(at: update, withIntermediateDirectories: true)
        let artifact = update.appendingPathComponent("liveblock-detector.mlmodelc", isDirectory: true)
        try FileManager.default.copyItem(at: compiled, to: artifact)
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        try writeManifest(
            signedManifest(artifact: artifact, key: key, sequence: 1),
            to: update.appendingPathComponent("liveblock-detector.manifest.json")
        )
        let manager = MacModelDistribution(environment: environment(root: root, keyring: keyring))
        XCTAssertNoThrow(try manager.installUpdatePackage(at: update))
        XCTAssertEqual(try manager.resolveAuthenticatedModel(), manager.activeArtifactURL)
    }

    func testInstallPersistsSignedStateAndRejectsRollback() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let update = try makeUpdate(root: root, key: key, sequence: 2, marker: "v2")
        var activations = 0
        let manager = MacModelDistribution(
            environment: environment(root: root, keyring: keyring),
            modelValidator: { _ in }
        )
        let receipt = try manager.installUpdatePackage(at: update) { activations += 1 }
        XCTAssertEqual(receipt.releaseSequence, 2)
        XCTAssertEqual(activations, 1)
        XCTAssertEqual(try manager.resolveAuthenticatedModel(), manager.activeArtifactURL)
        XCTAssertThrowsError(try manager.installUpdatePackage(at: update)) { error in
            guard case MacModelDistributionError.rollback(2, 2) = error else {
                return XCTFail("unexpected error: \(error)")
            }
        }
    }

    func testSuccessfulReplacementCommitsNewArtifactAndRejectsActiveTamper() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let first = try makeUpdate(root: root, key: key, sequence: 1, marker: "v1")
        let second = try makeUpdate(root: root, key: key, sequence: 2, marker: "v2")
        let manager = MacModelDistribution(
            environment: environment(root: root, keyring: keyring),
            modelValidator: { _ in }
        )
        _ = try manager.installUpdatePackage(at: first)
        let receipt = try manager.installUpdatePackage(at: second)
        XCTAssertTrue(receipt.previousArtifactPreserved)
        let expected = try MacArtifactHash.sha256(
            second.appendingPathComponent("liveblock-detector.mlmodelc")
        )
        XCTAssertEqual(try MacArtifactHash.sha256(manager.activeArtifactURL), expected)

        let activeFile = manager.activeArtifactURL.appendingPathComponent("a.txt")
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: activeFile.path)
        try Data("tampered".utf8).write(to: activeFile)
        XCTAssertThrowsError(try manager.resolveAuthenticatedModel())
    }

    func testLoadFailureAndStateCommitFailurePreservePreviousModel() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let first = try makeUpdate(root: root, key: key, sequence: 1, marker: "good")
        let broken = try makeUpdate(root: root, key: key, sequence: 2, marker: "broken")
        let env = environment(root: root, keyring: keyring)
        let initial = MacModelDistribution(environment: env, modelValidator: { _ in })
        _ = try initial.installUpdatePackage(at: first)
        let originalHash = try MacArtifactHash.sha256(initial.activeArtifactURL)
        let rejecting = MacModelDistribution(environment: env, modelValidator: { url in
            if try String(contentsOf: url.appendingPathComponent("a.txt"), encoding: .utf8) == "broken" {
                throw MacModelDistributionError.invalid("runtime rejected")
            }
        })
        XCTAssertThrowsError(try rejecting.installUpdatePackage(at: broken))
        XCTAssertEqual(try MacArtifactHash.sha256(initial.activeArtifactURL), originalHash)

        let failingCommit = MacModelDistribution(
            environment: env,
            modelValidator: { _ in },
            beforeStateCommit: { throw MacModelDistributionError.io("injected state failure") }
        )
        XCTAssertThrowsError(try failingCommit.installUpdatePackage(at: broken))
        XCTAssertEqual(try MacArtifactHash.sha256(initial.activeArtifactURL), originalHash)
        XCTAssertEqual(try initial.resolveAuthenticatedModel(), initial.activeArtifactURL)
    }

    func testFirstInstallCommitFailureRemovesUncommittedArtifact() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let update = try makeUpdate(root: root, key: key, sequence: 1, marker: "v1")
        let manager = MacModelDistribution(
            environment: environment(root: root, keyring: keyring),
            modelValidator: { _ in },
            beforeStateCommit: { throw MacModelDistributionError.io("injected") }
        )
        XCTAssertThrowsError(try manager.installUpdatePackage(at: update))
        XCTAssertFalse(FileManager.default.fileExists(atPath: manager.activeArtifactURL.path))

        try FileManager.default.copyItem(
            at: update.appendingPathComponent("liveblock-detector.mlmodelc"),
            to: manager.activeArtifactURL
        )
        let recovery = MacModelDistribution(
            environment: environment(root: root, keyring: keyring),
            modelValidator: { _ in }
        )
        XCTAssertNil(try recovery.resolveAuthenticatedModel())
        XCTAssertFalse(FileManager.default.fileExists(atPath: manager.activeArtifactURL.path))
    }

    func testStartupRecoversInterruptedSwap() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let first = try makeUpdate(root: root, key: key, sequence: 1, marker: "v1")
        let second = try makeUpdate(root: root, key: key, sequence: 2, marker: "v2")
        let env = environment(root: root, keyring: keyring)
        let manager = MacModelDistribution(environment: env, modelValidator: { _ in })
        _ = try manager.installUpdatePackage(at: first)
        let acceptedHash = try MacArtifactHash.sha256(manager.activeArtifactURL)

        let staging = env.modelsDirectory.appendingPathComponent(".liveblock-detector.mlmodelc.installing")
        try FileManager.default.moveItem(at: manager.activeArtifactURL, to: staging)
        try FileManager.default.copyItem(
            at: second.appendingPathComponent("liveblock-detector.mlmodelc"),
            to: manager.activeArtifactURL
        )
        XCTAssertEqual(try manager.resolveAuthenticatedModel(), manager.activeArtifactURL)
        XCTAssertEqual(try MacArtifactHash.sha256(manager.activeArtifactURL), acceptedHash)
        XCTAssertFalse(FileManager.default.fileExists(atPath: staging.path))

        let backup = env.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc.pre-update")
        try FileManager.default.moveItem(at: manager.activeArtifactURL, to: backup)
        try FileManager.default.copyItem(
            at: second.appendingPathComponent("liveblock-detector.mlmodelc"),
            to: manager.activeArtifactURL
        )
        XCTAssertEqual(try manager.resolveAuthenticatedModel(), manager.activeArtifactURL)
        XCTAssertEqual(try MacArtifactHash.sha256(manager.activeArtifactURL), acceptedHash)
        XCTAssertFalse(FileManager.default.fileExists(atPath: backup.path))

        try FileManager.default.moveItem(at: manager.activeArtifactURL, to: backup)
        XCTAssertEqual(try manager.resolveAuthenticatedModel(), manager.activeArtifactURL)
        XCTAssertEqual(try MacArtifactHash.sha256(manager.activeArtifactURL), acceptedHash)
    }

    func testPackagedReleaseIsFloorAndSupersedesOlderActiveUpdate() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let old = try makeUpdate(root: root, key: key, sequence: 5, marker: "old")
        let packaged = try makeUpdate(root: root, key: key, sequence: 100, marker: "packaged")
        let oldManager = MacModelDistribution(
            environment: environment(root: root, keyring: keyring),
            modelValidator: { _ in }
        )
        _ = try oldManager.installUpdatePackage(at: old)
        let upgraded = MacModelDistribution(
            environment: environment(root: root, keyring: keyring, packagedUpdate: packaged),
            modelValidator: { _ in }
        )
        XCTAssertEqual(
            try upgraded.resolveAuthenticatedModel(),
            packaged.appendingPathComponent("liveblock-detector.mlmodelc")
        )
        XCTAssertThrowsError(try upgraded.installUpdatePackage(at: old))
        let betweenFloors = try makeUpdate(root: root, key: key, sequence: 50, marker: "stale")
        XCTAssertThrowsError(try upgraded.installUpdatePackage(at: betweenFloors))
        let newer = try makeUpdate(root: root, key: key, sequence: 101, marker: "newer")
        XCTAssertEqual(try upgraded.installUpdatePackage(at: newer).releaseSequence, 101)
        XCTAssertEqual(try upgraded.resolveAuthenticatedModel(), upgraded.activeArtifactURL)
    }

    func testResolveWaitsForInstallerLockInsteadOfFailingPermanently() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let packaged = try makeUpdate(root: root, key: key, sequence: 1, marker: "packaged")
        let env = environment(root: root, keyring: keyring, packagedUpdate: packaged)
        try FileManager.default.createDirectory(at: env.modelsDirectory, withIntermediateDirectories: true)
        let lockURL = env.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc.transaction.lock")
        let fd = open(lockURL.path, O_RDWR | O_CREAT | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        XCTAssertGreaterThanOrEqual(fd, 0)
        XCTAssertEqual(flock(fd, LOCK_EX | LOCK_NB), 0)
        let semaphore = DispatchSemaphore(value: 0)
        let manager = MacModelDistribution(environment: env, modelValidator: { _ in })
        nonisolated(unsafe) var result: Result<URL?, Error>?
        DispatchQueue.global().async {
            result = Result { try manager.resolveAuthenticatedModel() }
            semaphore.signal()
        }
        XCTAssertEqual(semaphore.wait(timeout: .now() + 0.1), .timedOut)
        close(fd)
        XCTAssertEqual(semaphore.wait(timeout: .now() + 2), .success)
        XCTAssertEqual(try result?.get(), packaged.appendingPathComponent("liveblock-detector.mlmodelc"))
    }

    func testAdvisoryLockBlocksConcurrentInstallAndIsReusable() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let update = try makeUpdate(root: root, key: key, sequence: 1, marker: "v1")
        let env = environment(root: root, keyring: keyring)
        try FileManager.default.createDirectory(at: env.modelsDirectory, withIntermediateDirectories: true)
        let lockURL = env.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc.transaction.lock")
        let fd = open(lockURL.path, O_RDWR | O_CREAT | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        XCTAssertGreaterThanOrEqual(fd, 0)
        XCTAssertEqual(flock(fd, LOCK_EX | LOCK_NB), 0)
        let manager = MacModelDistribution(environment: env, modelValidator: { _ in })
        XCTAssertThrowsError(try manager.installUpdatePackage(at: update))
        close(fd)
        XCTAssertNoThrow(try manager.installUpdatePackage(at: update))
        try FileManager.default.removeItem(at: lockURL)
        let target = root.appendingPathComponent("lock-target")
        try Data().write(to: target)
        try FileManager.default.createSymbolicLink(at: lockURL, withDestinationURL: target)
        let next = try makeUpdate(root: root, key: key, sequence: 2, marker: "v2")
        XCTAssertThrowsError(try manager.installUpdatePackage(at: next))
    }

    func testFutureStateAndSymlinkArtifactFailClosed() throws {
        let root = try temporaryRoot()
        let key = Curve25519.Signing.PrivateKey()
        let keyring = root.appendingPathComponent("keys.json")
        try writeKeyring(key, to: keyring)
        let update = try makeUpdate(root: root, key: key, sequence: 1, marker: "v1")
        let env = environment(root: root, keyring: keyring)
        let manager = MacModelDistribution(environment: env, modelValidator: { _ in })
        _ = try manager.installUpdatePackage(at: update)
        let stateURL = env.modelsDirectory.appendingPathComponent("update-state.json")
        var state = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: stateURL)) as? [String: Any]
        )
        state["schemaVersion"] = 2
        let future = try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
        try future.write(to: stateURL)
        XCTAssertThrowsError(try manager.resolveAuthenticatedModel())
        XCTAssertEqual(try Data(contentsOf: stateURL), future)

        let symlinkRoot = try temporaryRoot()
        let target = symlinkRoot.appendingPathComponent("target")
        try Data("x".utf8).write(to: target)
        let artifact = symlinkRoot.appendingPathComponent("artifact", isDirectory: true)
        try FileManager.default.createDirectory(at: artifact, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(
            at: artifact.appendingPathComponent("link"),
            withDestinationURL: target
        )
        XCTAssertThrowsError(try MacArtifactHash.sha256(artifact))
    }
}
