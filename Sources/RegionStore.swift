import Foundation
import CoreGraphics

/// A rectangle expressed in normalized [0..1] coordinates with origin at top-left.
/// This survives window resize, display change, and DPI change.
struct NormalizedRegion: Codable, Identifiable, Hashable, Sendable {
    let id: UUID
    var x: Double
    var y: Double
    var width: Double
    var height: Double

    init(id: UUID = UUID(), x: Double, y: Double, width: Double, height: Double) {
        self.id = id
        self.x = max(0, min(1, x))
        self.y = max(0, min(1, y))
        self.width = max(0, min(1 - self.x, width))
        self.height = max(0, min(1 - self.y, height))
    }

    /// Convert to a CGRect in a target coordinate space sized `size`. Origin remains top-left.
    func rect(in size: CGSize) -> CGRect {
        CGRect(x: x * size.width,
               y: y * size.height,
               width: width * size.width,
               height: height * size.height)
    }

    /// Same as `rect(in:)` but flipped to a CoreVideo pixel-buffer's bottom-left origin.
    func cvRect(inPixelBufferSize size: CGSize) -> CGRect {
        let rect = self.rect(in: size)
        return CGRect(x: rect.minX,
                      y: size.height - rect.maxY,
                      width: rect.width,
                      height: rect.height)
    }
}

/// Thread-safe persistent store for user-drawn regions.
/// Persistence is delegated to `liveblock-bridge` (Rust) so the on-disk format
/// is byte-identical with the Windows + Linux ports. The Swift side keeps an
/// in-memory cache of decoded `NormalizedRegion`s for fast `current` reads.
final class RegionStore: @unchecked Sendable {
    private let lock = NSLock()
    private var regions: [NormalizedRegion] = []
    private let handle: RegionStoreHandle
    private let storageURL: URL
    private let unsupportedSchemaVersion: Int?

    init(storageURL: URL? = nil) {
        if let storageURL {
            self.storageURL = storageURL
        } else {
            let appSupport = FileManager.default.urls(for: .applicationSupportDirectory,
                                                      in: .userDomainMask).first
                ?? FileManager.default.temporaryDirectory
            let dir = appSupport.appendingPathComponent("LiveBlock", isDirectory: true)
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            self.storageURL = dir.appendingPathComponent("regions.json")
        }
        // Pre-flight malformed data so it remains recoverable. Valid legacy
        // arrays are migrated by Rust. Unknown future schema envelopes are
        // deliberately left untouched so a downgrade never destroys them.
        self.unsupportedSchemaVersion = Self.prepareStorage(at: self.storageURL)
        self.handle = region_store_open(self.storageURL.path)
        self.regions = Self.snapshot(handle: handle)
    }

    private struct PersistedRegionDocument: Decodable {
        let regions: [NormalizedRegion]
        let schemaVersion: Int
    }

    /// Quarantine malformed JSON, but preserve well-formed future schemas in
    /// place. The Rust store rejects those versions and will not persist over
    /// the file.
    private static func prepareStorage(at url: URL) -> Int? {
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else { return nil }
        guard let data = try? Data(contentsOf: url), !data.isEmpty else { return nil }
        let decoder = JSONDecoder()
        if (try? decoder.decode([NormalizedRegion].self, from: data)) != nil { return nil }
        // Inspect the discriminator before decoding the payload. A future
        // schema may intentionally change the regions shape; downgrades must
        // preserve it rather than misclassifying it as corrupt.
        if let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let version = object["schemaVersion"] as? Int,
           version > 1 {
            NSLog("RegionStore: refusing unsupported regions schema \(version)")
            return version
        }
        if let document = try? decoder.decode(PersistedRegionDocument.self, from: data),
           document.schemaVersion == 1 {
            return nil
        }

        let stamp = Int(Date().timeIntervalSince1970)
        let dst = url.deletingLastPathComponent()
            .appendingPathComponent("regions.json.corrupt-\(stamp)")
        do {
            try fm.moveItem(at: url, to: dst)
            NSLog("RegionStore: corrupt regions.json moved aside to \(dst.lastPathComponent)")
        } catch {
            NSLog("RegionStore: failed to quarantine corrupt regions.json: \(error.localizedDescription)")
        }
        return nil
    }

    var persistenceCompatibilityError: String? {
        unsupportedSchemaVersion.map { "Regions use unsupported schema \($0); file is read-only." }
    }

    var current: [NormalizedRegion] {
        lock.lock(); defer { lock.unlock() }
        return regions
    }

    func current(excluding disabledIDs: Set<UUID>) -> [NormalizedRegion] {
        lock.lock(); defer { lock.unlock() }
        guard !disabledIDs.isEmpty else { return regions }
        return regions.filter { !disabledIDs.contains($0.id) }
    }

    func add(_ region: NormalizedRegion) {
        guard unsupportedSchemaVersion == nil else { return }
        lock.lock()
        defer { lock.unlock() }
        _ = handle.add_with_id(region.id.uuidString,
                               region.x, region.y, region.width, region.height)
        regions = Self.snapshot(handle: handle)
    }

    func remove(id: UUID) {
        guard unsupportedSchemaVersion == nil else { return }
        lock.lock()
        defer { lock.unlock() }
        _ = handle.remove(id.uuidString)
        regions = Self.snapshot(handle: handle)
    }

    func replace(_ newRegions: [NormalizedRegion]) {
        guard unsupportedSchemaVersion == nil else { return }
        lock.lock()
        defer { lock.unlock() }
        _ = handle.clear()
        for r in newRegions {
            _ = handle.add_with_id(r.id.uuidString, r.x, r.y, r.width, r.height)
        }
        regions = Self.snapshot(handle: handle)
    }

    /// Replace a single region by id, preserving its id.
    func replace(id: UUID, with newRegion: NormalizedRegion) {
        guard unsupportedSchemaVersion == nil else { return }
        lock.lock()
        defer { lock.unlock() }
        _ = handle.replace_id(id.uuidString,
                              newRegion.x, newRegion.y, newRegion.width, newRegion.height)
        regions = Self.snapshot(handle: handle)
    }

    func clear() {
        guard unsupportedSchemaVersion == nil else { return }
        lock.lock()
        defer { lock.unlock() }
        _ = handle.clear()
        regions = []
    }

    // MARK: - Persistence helpers

    private static func snapshot(handle: RegionStoreHandle) -> [NormalizedRegion] {
        let json = handle.to_json().toString()
        guard !json.isEmpty, let data = json.data(using: .utf8) else { return [] }
        return (try? JSONDecoder().decode([NormalizedRegion].self, from: data)) ?? []
    }
}
