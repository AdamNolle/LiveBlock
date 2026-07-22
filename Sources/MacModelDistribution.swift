import Foundation
import CryptoKit
import CoreML
import Vision
import Darwin

/// Authenticated CoreML distribution for production macOS builds. Debug/source
/// training remains a separate candidate workflow and never calls this updater.
enum MacModelDistributionError: LocalizedError, Equatable {
    case invalid(String)
    case unsupportedSchema(Int)
    case untrustedKey(String)
    case signature
    case fingerprint
    case rollback(candidate: UInt64, accepted: UInt64)
    case collision(String)
    case io(String)

    var errorDescription: String? {
        switch self {
        case .invalid(let value): return "Invalid model distribution data: \(value)"
        case .unsupportedSchema(let value): return "Unsupported model distribution schema \(value)"
        case .untrustedKey(let value): return "Untrusted model signing key: \(value)"
        case .signature: return "Model manifest signature verification failed"
        case .fingerprint: return "Model artifact fingerprint mismatch"
        case .rollback(let candidate, let accepted):
            return "Model release sequence \(candidate) is not newer than \(accepted)"
        case .collision(let value): return "Model update path collision: \(value)"
        case .io(let value): return "Model update I/O failure: \(value)"
        }
    }
}

struct MacModelManifest: Codable, Equatable {
    static let keys: Set<String> = [
        "schemaVersion", "modelId", "modelVersion", "artifactFormat",
        "artifactHashAlgorithm", "artifactSha256", "runtimeClasses",
        "inputWidth", "inputHeight", "nmsEmbedded", "releaseSequence",
        "promotionGateSchema", "promotionReportSha256", "createdAt", "keyId", "signature"
    ]

    let schemaVersion: Int
    let modelId: String
    let modelVersion: String
    let artifactFormat: String
    let artifactHashAlgorithm: String
    let artifactSha256: String
    let runtimeClasses: [String]
    let inputWidth: Int
    let inputHeight: Int
    let nmsEmbedded: Bool
    let releaseSequence: UInt64
    let promotionGateSchema: Int
    let promotionReportSha256: String
    let createdAt: String
    let keyId: String
    var signature: String

    static func load(from url: URL) throws -> MacModelManifest {
        let data = try regularFileData(url)
        let object = try jsonObject(data, exactKeys: keys, name: "manifest")
        _ = object
        let manifest = try JSONDecoder().decode(Self.self, from: data)
        try manifest.validate()
        return manifest
    }

    func validate() throws {
        guard schemaVersion == 2 else { throw MacModelDistributionError.unsupportedSchema(schemaVersion) }
        guard !modelId.isEmpty, !modelVersion.isEmpty, artifactFormat == "coreml",
              artifactHashAlgorithm == "sha256-file-or-tree-v1",
              isSHA256(artifactSha256),
              runtimeClasses == ["Logo", "Ad banner", "Sponsored"],
              inputWidth > 0, inputHeight > 0, releaseSequence > 0,
              promotionGateSchema == 5, isSHA256(promotionReportSha256),
              !createdAt.isEmpty, !keyId.isEmpty, !signature.isEmpty
        else { throw MacModelDistributionError.invalid("manifest contract") }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if formatter.date(from: createdAt) == nil {
            formatter.formatOptions = [.withInternetDateTime]
            guard formatter.date(from: createdAt) != nil else {
                throw MacModelDistributionError.invalid("createdAt")
            }
        }
    }

    func signingBytes() throws -> Data {
        let encoded = try JSONEncoder().encode(self)
        guard var object = try JSONSerialization.jsonObject(with: encoded) as? [String: Any] else {
            throw MacModelDistributionError.invalid("manifest JSON")
        }
        object.removeValue(forKey: "signature")
        return try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes])
    }
}

struct MacTrustedKeyring {
    static let documentKeys: Set<String> = ["schemaVersion", "keys"]
    static let entryKeys: Set<String> = ["keyId", "publicKeyBase64"]
    let keys: [String: Curve25519.Signing.PublicKey]

    static func load(from url: URL, requireNonempty: Bool = true) throws -> Self {
        let data = try regularFileData(url)
        let object = try jsonObject(data, exactKeys: documentKeys, name: "keyring")
        guard let schema = object["schemaVersion"] as? NSNumber,
              !isBoolean(schema), schema.intValue == 1,
              !CFNumberIsFloatType(schema),
              let entries = object["keys"] as? [[String: Any]] else {
            throw MacModelDistributionError.invalid("keyring")
        }
        if requireNonempty && entries.isEmpty { throw MacModelDistributionError.invalid("empty keyring") }
        var result: [String: Curve25519.Signing.PublicKey] = [:]
        for entry in entries {
            guard Set(entry.keys) == entryKeys,
                  let keyID = entry["keyId"] as? String, !keyID.isEmpty,
                  let encoded = entry["publicKeyBase64"] as? String,
                  let bytes = Data(base64Encoded: encoded), bytes.count == 32,
                  result[keyID] == nil else {
                throw MacModelDistributionError.invalid("keyring entry")
            }
            do { result[keyID] = try Curve25519.Signing.PublicKey(rawRepresentation: bytes) }
            catch { throw MacModelDistributionError.invalid("public key") }
        }
        return Self(keys: result)
    }

    func verify(_ manifest: MacModelManifest, artifact: URL) throws {
        guard let key = keys[manifest.keyId] else {
            throw MacModelDistributionError.untrustedKey(manifest.keyId)
        }
        guard let signature = Data(base64Encoded: manifest.signature), signature.count == 64,
              key.isValidSignature(signature, for: try manifest.signingBytes()) else {
            throw MacModelDistributionError.signature
        }
        guard try MacArtifactHash.sha256(artifact) == manifest.artifactSha256 else {
            throw MacModelDistributionError.fingerprint
        }
    }
}

struct MacModelUpdateState: Codable, Equatable {
    static let keys: Set<String> = ["schemaVersion", "highestReleaseSequence", "acceptedManifest"]
    let schemaVersion: Int
    let highestReleaseSequence: UInt64
    let acceptedManifest: MacModelManifest

    static func load(from url: URL) throws -> Self {
        let data = try regularFileData(url)
        let object = try jsonObject(data, exactKeys: keys, name: "update state")
        guard let accepted = object["acceptedManifest"] as? [String: Any],
              Set(accepted.keys) == MacModelManifest.keys else {
            throw MacModelDistributionError.invalid("accepted manifest fields")
        }
        let state = try JSONDecoder().decode(Self.self, from: data)
        guard state.schemaVersion == 1 else {
            throw MacModelDistributionError.unsupportedSchema(state.schemaVersion)
        }
        try state.acceptedManifest.validate()
        guard state.highestReleaseSequence == state.acceptedManifest.releaseSequence else {
            throw MacModelDistributionError.invalid("update state sequence")
        }
        return state
    }
}

struct MacModelUpdateReceipt: Equatable, Sendable {
    let modelVersion: String
    let releaseSequence: UInt64
    let artifactSha256: String
    let previousArtifactPreserved: Bool
}

enum MacArtifactHash {
    private static let domain = Data("liveblock-tree-sha256-v1\0".utf8)

    static func sha256(_ url: URL) throws -> String {
        let values = try noFollowValues(url)
        if values.isRegularFile == true { return hex(try fileDigest(url)) }
        guard values.isDirectory == true else {
            throw MacModelDistributionError.invalid("artifact root type")
        }
        var entries: [(Data, URL)] = []
        guard let enumerator = FileManager.default.enumerator(
            at: url,
            includingPropertiesForKeys: [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey],
            options: [],
            errorHandler: { _, _ in false }
        ) else { throw MacModelDistributionError.io("enumerate artifact") }
        for case let item as URL in enumerator {
            let itemValues = try item.resourceValues(forKeys: [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey])
            if itemValues.isSymbolicLink == true { throw MacModelDistributionError.invalid("artifact symlink") }
            if itemValues.isDirectory == true { continue }
            guard itemValues.isRegularFile == true else {
                throw MacModelDistributionError.invalid("artifact special entry")
            }
            // FileManager's enumerator canonicalizes `/var` to `/private/var`;
            // normalize both sides only after rejecting symbolic-link entries.
            let rootPath = url.resolvingSymlinksInPath().standardizedFileURL.path
            let itemPath = item.standardizedFileURL.path
            let prefix = rootPath.hasSuffix("/") ? rootPath : rootPath + "/"
            guard itemPath.hasPrefix(prefix),
                  let relative = String(itemPath.dropFirst(prefix.count)).data(using: .utf8) else {
                throw MacModelDistributionError.invalid("artifact relative path")
            }
            entries.append((relative, item))
        }
        entries.sort { $0.0.lexicographicallyPrecedes($1.0) }
        var digest = SHA256()
        digest.update(data: domain)
        for (relative, item) in entries {
            var length = UInt64(relative.count).bigEndian
            digest.update(data: Data(bytes: &length, count: MemoryLayout<UInt64>.size))
            digest.update(data: relative)
            digest.update(data: try fileDigest(item))
        }
        return hex(Data(digest.finalize()))
    }

    private static func fileDigest(_ url: URL) throws -> Data {
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw MacModelDistributionError.io("open artifact file") }
        defer { close(fd) }
        var status = stat()
        guard fstat(fd, &status) == 0, status.st_mode & S_IFMT == S_IFREG else {
            throw MacModelDistributionError.invalid("artifact file type")
        }
        var digest = SHA256()
        var buffer = [UInt8](repeating: 0, count: 1024 * 1024)
        while true {
            let count = buffer.withUnsafeMutableBytes { bytes in
                Darwin.read(fd, bytes.baseAddress, bytes.count)
            }
            if count < 0 { throw MacModelDistributionError.io("read artifact file") }
            if count == 0 { break }
            digest.update(data: Data(buffer[0..<count]))
        }
        return Data(digest.finalize())
    }

    private static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }
}

private func regularFileData(_ url: URL) throws -> Data {
    guard try noFollowValues(url).isRegularFile == true else {
        throw MacModelDistributionError.invalid("regular file required")
    }
    return try Data(contentsOf: url)
}

private func noFollowValues(_ url: URL) throws -> URLResourceValues {
    let values = try url.resourceValues(forKeys: [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey])
    guard values.isSymbolicLink != true else { throw MacModelDistributionError.invalid("symlink") }
    return values
}

private func jsonObject(_ data: Data, exactKeys: Set<String>, name: String) throws -> [String: Any] {
    guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          Set(object.keys) == exactKeys else {
        throw MacModelDistributionError.invalid("\(name) fields")
    }
    return object
}

private func isSHA256(_ value: String) -> Bool {
    value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
}

private func isBoolean(_ number: NSNumber) -> Bool {
    CFGetTypeID(number) == CFBooleanGetTypeID()
}

struct MacModelDistributionEnvironment {
    let modelsDirectory: URL
    let keyringURL: URL
    let packagedArtifactURL: URL?
    let packagedManifestURL: URL?

    static func production(bundle: Bundle = .main) -> Self {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return Self(
            modelsDirectory: support.appendingPathComponent("LiveBlock/models", isDirectory: true),
            keyringURL: bundle.url(forResource: "release-trusted-model-keys", withExtension: "json")
                ?? bundle.url(forResource: "trusted-model-keys", withExtension: "json")
                ?? bundle.bundleURL.appendingPathComponent("trusted-model-keys.json"),
            packagedArtifactURL: bundle.url(forResource: "release-liveblock-detector", withExtension: "mlmodelc")
                ?? bundle.url(forResource: "liveblock-detector", withExtension: "mlmodelc"),
            packagedManifestURL: bundle.url(forResource: "release-liveblock-detector.manifest", withExtension: "json")
                ?? bundle.url(forResource: "liveblock-detector.manifest", withExtension: "json")
        )
    }
}

final class MacModelDistribution: @unchecked Sendable {
    private let environment: MacModelDistributionEnvironment
    private let fileManager: FileManager
    private let modelValidator: (URL) throws -> Void
    private let beforeStateCommit: () throws -> Void

    init(
        environment: MacModelDistributionEnvironment = .production(),
        fileManager: FileManager = .default,
        modelValidator: @escaping (URL) throws -> Void = { url in
            let configuration = MLModelConfiguration()
            configuration.computeUnits = .all
            let model = try MLModel(contentsOf: url, configuration: configuration)
            _ = try VNCoreMLModel(for: model)
        },
        beforeStateCommit: @escaping () throws -> Void = {}
    ) {
        self.environment = environment
        self.fileManager = fileManager
        self.modelValidator = modelValidator
        self.beforeStateCommit = beforeStateCommit
    }

    var activeArtifactURL: URL {
        environment.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc", isDirectory: true)
    }
    private var stateURL: URL { environment.modelsDirectory.appendingPathComponent("update-state.json") }
    private var stagingURL: URL { environment.modelsDirectory.appendingPathComponent(".liveblock-detector.mlmodelc.installing", isDirectory: true) }
    private var backupURL: URL { environment.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc.pre-update", isDirectory: true) }
    private var lockURL: URL { environment.modelsDirectory.appendingPathComponent("liveblock-detector.mlmodelc.transaction.lock") }

    /// Resolve the only production-loadable model. Accepted update corruption
    /// fails closed and never falls back; a newer authenticated app-bundled
    /// release legitimately supersedes an older accepted update.
    func resolveAuthenticatedModel() throws -> URL? {
        let keyring = try MacTrustedKeyring.load(from: environment.keyringURL)
        let packaged = try verifiedPackaged(keyring: keyring)
        try fileManager.createDirectory(at: environment.modelsDirectory, withIntermediateDirectories: true)
        return try withLock(blocking: true) {
            let state = try recover(keyring: keyring)
            guard let state else { return packaged?.artifact }
            if let packaged {
                guard packaged.manifest.modelId == state.acceptedManifest.modelId else {
                    throw MacModelDistributionError.invalid("packaged modelId differs from accepted update")
                }
                if packaged.manifest.releaseSequence > state.highestReleaseSequence {
                    return packaged.artifact
                }
                if packaged.manifest.releaseSequence == state.highestReleaseSequence,
                   packaged.manifest.artifactSha256 != state.acceptedManifest.artifactSha256 {
                    throw MacModelDistributionError.invalid("release sequence authenticates two artifacts")
                }
            }
            return activeArtifactURL
        }
    }

    /// Install a directory containing precompiled `liveblock-detector.mlmodelc` and
    /// `liveblock-detector.manifest.json`. No network or trust-root selection is
    /// performed here; transport is intentionally outside this primitive.
    func installUpdatePackage(
        at package: URL,
        activate: () -> Void = {}
    ) throws -> MacModelUpdateReceipt {
        guard try noFollowValues(package).isDirectory == true else {
            throw MacModelDistributionError.invalid("update package directory")
        }
        let packageEntries = try Set(fileManager.contentsOfDirectory(atPath: package.path))
        guard packageEntries == ["liveblock-detector.mlmodelc", "liveblock-detector.manifest.json"] else {
            throw MacModelDistributionError.invalid("update package entries")
        }
        let source = package.appendingPathComponent("liveblock-detector.mlmodelc", isDirectory: true)
        let manifestURL = package.appendingPathComponent("liveblock-detector.manifest.json")
        let keyring = try MacTrustedKeyring.load(from: environment.keyringURL)
        let candidate = try MacModelManifest.load(from: manifestURL)
        try keyring.verify(candidate, artifact: source)
        let packaged = try verifiedPackaged(keyring: keyring)
        try fileManager.createDirectory(at: environment.modelsDirectory, withIntermediateDirectories: true)

        return try withLock(blocking: false) {
            let previous = try recover(keyring: keyring)
            var acceptedFloor: UInt64 = 0
            if let packaged {
                guard packaged.manifest.modelId == candidate.modelId else {
                    throw MacModelDistributionError.invalid("packaged modelId differs from candidate")
                }
                acceptedFloor = packaged.manifest.releaseSequence
            }
            if let previous {
                guard previous.acceptedManifest.modelId == candidate.modelId else {
                    throw MacModelDistributionError.invalid("accepted modelId differs from candidate")
                }
                acceptedFloor = max(acceptedFloor, previous.highestReleaseSequence)
            }
            guard candidate.releaseSequence > acceptedFloor else {
                throw MacModelDistributionError.rollback(candidate: candidate.releaseSequence, accepted: acceptedFloor)
            }

            if entryExists(backupURL) { try safeRemoveTree(backupURL) }
            guard !entryExists(stagingURL) else {
                throw MacModelDistributionError.collision(stagingURL.path)
            }
            do {
                try fileManager.copyItem(at: source, to: stagingURL)
                guard try MacArtifactHash.sha256(stagingURL) == candidate.artifactSha256 else {
                    throw MacModelDistributionError.fingerprint
                }
                try modelValidator(stagingURL)
                try makeTreeReadOnly(stagingURL)
                try syncTree(stagingURL)
                guard try MacArtifactHash.sha256(stagingURL) == candidate.artifactSha256 else {
                    throw MacModelDistributionError.fingerprint
                }

                let hadPrevious = entryExists(activeArtifactURL)
                if hadPrevious {
                    try atomicSwap(stagingURL, activeArtifactURL)
                    try renameItem(stagingURL, backupURL)
                } else {
                    try renameItem(stagingURL, activeArtifactURL)
                }
                let state = MacModelUpdateState(
                    schemaVersion: 1,
                    highestReleaseSequence: candidate.releaseSequence,
                    acceptedManifest: candidate
                )
                do {
                    try beforeStateCommit()
                    try persist(state: state)
                } catch {
                    try rollbackAfterFailedCommit(hadPrevious: hadPrevious)
                    throw error
                }
                // Keep activation inside both the advisory lock and committed
                // disk transaction so concurrent installs cannot reorder memory.
                activate()
                return MacModelUpdateReceipt(
                    modelVersion: candidate.modelVersion,
                    releaseSequence: candidate.releaseSequence,
                    artifactSha256: candidate.artifactSha256,
                    previousArtifactPreserved: hadPrevious
                )
            } catch {
                if entryExists(stagingURL) {
                    // A pre-swap staging candidate is safe to remove. After a
                    // swap it contains the accepted old model and rollback owns it.
                    if (try? MacArtifactHash.sha256(stagingURL)) == candidate.artifactSha256 {
                        try? safeRemoveTree(stagingURL)
                    }
                }
                throw error
            }
        }
    }

    private func verifiedPackaged(keyring: MacTrustedKeyring) throws -> (manifest: MacModelManifest, artifact: URL)? {
        switch (environment.packagedArtifactURL, environment.packagedManifestURL) {
        case (nil, nil): return nil
        case let (.some(artifact), .some(manifestURL)):
            let manifest = try MacModelManifest.load(from: manifestURL)
            try keyring.verify(manifest, artifact: artifact)
            return (manifest, artifact)
        default:
            throw MacModelDistributionError.invalid("packaged artifact and manifest must both exist")
        }
    }

    private func recover(keyring: MacTrustedKeyring) throws -> MacModelUpdateState? {
        let stateExists = entryExists(stateURL)
        guard stateExists else {
            if entryExists(backupURL) { throw MacModelDistributionError.collision(backupURL.path) }
            if entryExists(stagingURL) { try safeRemoveTree(stagingURL) }
            if entryExists(activeArtifactURL) { try safeRemoveTree(activeArtifactURL) }
            return nil
        }
        let state = try MacModelUpdateState.load(from: stateURL)
        let acceptedHash = state.acceptedManifest.artifactSha256
        if (try? MacArtifactHash.sha256(activeArtifactURL)) == acceptedHash {
            if entryExists(stagingURL) { try safeRemoveTree(stagingURL) }
            try keyring.verify(state.acceptedManifest, artifact: activeArtifactURL)
            return state
        }
        if (try? MacArtifactHash.sha256(stagingURL)) == acceptedHash {
            try restoreAccepted(from: stagingURL)
        } else if (try? MacArtifactHash.sha256(backupURL)) == acceptedHash {
            try restoreAccepted(from: backupURL)
        } else {
            throw MacModelDistributionError.fingerprint
        }
        try keyring.verify(state.acceptedManifest, artifact: activeArtifactURL)
        return state
    }

    private func restoreAccepted(from recovery: URL) throws {
        if entryExists(activeArtifactURL) {
            try atomicSwap(activeArtifactURL, recovery)
            try safeRemoveTree(recovery)
        } else {
            try renameItem(recovery, activeArtifactURL)
        }
    }

    private func rollbackAfterFailedCommit(hadPrevious: Bool) throws {
        if hadPrevious {
            if entryExists(backupURL) {
                try atomicSwap(activeArtifactURL, backupURL)
                try safeRemoveTree(backupURL)
            } else if entryExists(stagingURL) {
                try atomicSwap(activeArtifactURL, stagingURL)
                try safeRemoveTree(stagingURL)
            } else {
                throw MacModelDistributionError.fingerprint
            }
        } else if entryExists(activeArtifactURL) {
            try safeRemoveTree(activeArtifactURL)
        }
    }

    private func persist(state: MacModelUpdateState) throws {
        let data = try JSONEncoder.sortedPretty.encode(state)
        let temporary = environment.modelsDirectory.appendingPathComponent(".update-state.\(UUID().uuidString).installing")
        let fd = open(temporary.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard fd >= 0 else { throw posixError("create update state") }
        do {
            try data.withUnsafeBytes { bytes in
                var offset = 0
                while offset < bytes.count {
                    let count = write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                    if count < 0 { throw posixError("write update state") }
                    offset += count
                }
            }
            guard fsync(fd) == 0 else { throw posixError("sync update state") }
        } catch {
            close(fd); unlink(temporary.path); throw error
        }
        close(fd)
        guard rename(temporary.path, stateURL.path) == 0 else {
            unlink(temporary.path); throw posixError("commit update state")
        }
        // Rename is the transaction commit point. A later directory-fsync
        // failure cannot safely trigger model rollback against visible new state.
        try? syncDirectory(environment.modelsDirectory)
    }

    private func withLock<T>(blocking: Bool, _ body: () throws -> T) throws -> T {
        let fd = open(lockURL.path, O_RDWR | O_CREAT | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard fd >= 0 else { throw posixError("open update lock") }
        defer { close(fd) }
        var status = stat()
        guard fstat(fd, &status) == 0, status.st_mode & S_IFMT == S_IFREG else {
            throw MacModelDistributionError.invalid("update lock type")
        }
        let operation = blocking ? LOCK_EX : (LOCK_EX | LOCK_NB)
        guard flock(fd, operation) == 0 else {
            throw MacModelDistributionError.collision(lockURL.path)
        }
        return try body()
    }

    private func safeRemoveTree(_ url: URL) throws {
        _ = try MacArtifactHash.sha256(url) // rejects symlinks/special entries before deletion
        var directories = [url]
        if let enumerator = fileManager.enumerator(
            at: url,
            includingPropertiesForKeys: [.isSymbolicLinkKey, .isDirectoryKey],
            options: []
        ) {
            for case let item as URL in enumerator {
                let values = try noFollowValues(item)
                if values.isDirectory == true { directories.append(item) }
            }
        }
        for directory in directories {
            guard chmod(directory.path, S_IRUSR | S_IWUSR | S_IXUSR) == 0 else {
                throw posixError("unlock model directory for removal")
            }
        }
        try fileManager.removeItem(at: url)
        try syncDirectory(url.deletingLastPathComponent())
    }

    private func makeTreeReadOnly(_ root: URL) throws {
        var directories = [root]
        guard let enumerator = fileManager.enumerator(
            at: root,
            includingPropertiesForKeys: [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey],
            options: []
        ) else { throw MacModelDistributionError.io("enumerate staged model permissions") }
        for case let item as URL in enumerator {
            let values = try noFollowValues(item)
            if values.isDirectory == true {
                directories.append(item)
            } else if values.isRegularFile == true {
                guard chmod(item.path, S_IRUSR) == 0 else { throw posixError("lock staged file") }
            } else {
                throw MacModelDistributionError.invalid("staged special entry")
            }
        }
        for directory in directories.reversed() {
            guard chmod(directory.path, S_IRUSR | S_IXUSR) == 0 else {
                throw posixError("lock staged directory")
            }
        }
    }

    private func syncTree(_ root: URL) throws {
        var directories = [root]
        guard let enumerator = fileManager.enumerator(
            at: root,
            includingPropertiesForKeys: [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey],
            options: []
        ) else { throw MacModelDistributionError.io("enumerate staged model") }
        for case let item as URL in enumerator {
            let values = try noFollowValues(item)
            if values.isDirectory == true { directories.append(item); continue }
            guard values.isRegularFile == true else {
                throw MacModelDistributionError.invalid("staged special entry")
            }
            try syncFile(item)
        }
        for directory in directories.reversed() { try syncDirectory(directory) }
    }

    private func syncFile(_ url: URL) throws {
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw posixError("open staged file") }
        defer { close(fd) }
        guard fsync(fd) == 0 else { throw posixError("sync staged file") }
    }

    private func syncDirectory(_ url: URL) throws {
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw posixError("open directory") }
        defer { close(fd) }
        guard fsync(fd) == 0 else { throw posixError("sync directory") }
    }

    private func atomicSwap(_ first: URL, _ second: URL) throws {
        guard renameatx_np(AT_FDCWD, first.path, AT_FDCWD, second.path, UInt32(RENAME_SWAP)) == 0 else {
            throw posixError("atomic model directory swap")
        }
        try syncDirectory(first.deletingLastPathComponent())
    }

    private func renameItem(_ source: URL, _ destination: URL) throws {
        guard rename(source.path, destination.path) == 0 else { throw posixError("rename model directory") }
        try syncDirectory(destination.deletingLastPathComponent())
    }

    private func entryExists(_ url: URL) -> Bool {
        var value = stat()
        return lstat(url.path, &value) == 0
    }

    private func posixError(_ operation: String) -> MacModelDistributionError {
        MacModelDistributionError.io("\(operation): \(String(cString: strerror(errno)))")
    }
}

private extension JSONEncoder {
    static var sortedPretty: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return encoder
    }
}
