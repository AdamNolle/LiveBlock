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
        // Pre-flight: if the on-disk file exists but is invalid JSON, the
        // Rust side will silently return an empty store and the user will
        // see "no regions" with no idea their data was lost. Detect that
        // and rename the bad file so it's recoverable later.
        Self.quarantineIfCorrupt(at: self.storageURL)
        self.handle = region_store_open(self.storageURL.path)
        self.regions = Self.snapshot(handle: handle)
    }

    /// If `regions.json` exists but isn't a parseable JSON array, rename it
    /// to `regions.json.corrupt-<timestamp>` so the Rust store starts clean
    /// and the user can recover the bad file by hand if needed.
    private static func quarantineIfCorrupt(at url: URL) {
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else { return }
        guard let data = try? Data(contentsOf: url), !data.isEmpty else { return }
        let parsed = try? JSONDecoder().decode([NormalizedRegion].self, from: data)
        if parsed == nil {
            let stamp = Int(Date().timeIntervalSince1970)
            let dst = url.deletingLastPathComponent()
                .appendingPathComponent("regions.json.corrupt-\(stamp)")
            do {
                try fm.moveItem(at: url, to: dst)
                NSLog("RegionStore: corrupt regions.json moved aside to \(dst.lastPathComponent)")
            } catch {
                NSLog("RegionStore: failed to quarantine corrupt regions.json: \(error.localizedDescription)")
            }
        }
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
        lock.lock()
        defer { lock.unlock() }
        _ = handle.add_with_id(region.id.uuidString,
                               region.x, region.y, region.width, region.height)
        regions = Self.snapshot(handle: handle)
    }

    func remove(id: UUID) {
        lock.lock()
        defer { lock.unlock() }
        _ = handle.remove(id.uuidString)
        regions = Self.snapshot(handle: handle)
    }

    func replace(_ newRegions: [NormalizedRegion]) {
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
        lock.lock()
        defer { lock.unlock() }
        _ = handle.replace_id(id.uuidString,
                              newRegion.x, newRegion.y, newRegion.width, newRegion.height)
        regions = Self.snapshot(handle: handle)
    }

    func clear() {
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
