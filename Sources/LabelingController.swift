import Foundation
import AppKit
import Combine

/// One labeled box on a screenshot. Coords normalized [0..1] with origin top-left.
struct LabelBox: Codable, Identifiable, Hashable, Sendable {
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

    func rect(in size: CGSize) -> CGRect {
        CGRect(x: x * size.width,
               y: y * size.height,
               width: width * size.width,
               height: height * size.height)
    }
}

/// Persisted JSON sidecar for one labeled screenshot.
struct LabelDocument: Codable, Sendable {
    static let currentSchemaVersion = 1
    var schemaVersion: Int = currentSchemaVersion
    var image: String          // filename only (no path) so the dataset is portable
    var imageWidth: Int
    var imageHeight: Int
    var boxes: [LabelBox]
    var labeledAt: Date
}

enum LabelingFileMover {
    /// Move a screenshot and optional sidecar into trash without replacing any
    /// existing bytes. Moving the label first permits rollback if the screenshot
    /// move fails, matching the paired no-replace behavior of other adapters.
    static func movePairToTrashNoReplace(
        screenshot: URL,
        label: URL,
        trash: URL,
        fileManager: FileManager = .default
    ) throws {
        let screenshotDestination = trash.appendingPathComponent(screenshot.lastPathComponent)
        let labelDestination = trash.appendingPathComponent(label.lastPathComponent)
        let labelExists = fileManager.fileExists(atPath: label.path)
        guard !fileManager.fileExists(atPath: screenshotDestination.path),
              !labelExists || !fileManager.fileExists(atPath: labelDestination.path) else {
            throw CocoaError(.fileWriteFileExists)
        }

        var movedLabel = false
        if labelExists {
            try fileManager.moveItem(at: label, to: labelDestination)
            movedLabel = true
        }
        do {
            try fileManager.moveItem(at: screenshot, to: screenshotDestination)
        } catch {
            if movedLabel {
                try? fileManager.moveItem(at: labelDestination, to: label)
            }
            throw error
        }
    }
}

extension LabelDocument {
    private enum CodingKeys: String, CodingKey {
        case schemaVersion, image, imageWidth, imageHeight, boxes, labeledAt
    }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        let version = try values.decodeIfPresent(Int.self, forKey: .schemaVersion) ?? 1
        guard version == Self.currentSchemaVersion else {
            throw DecodingError.dataCorruptedError(
                forKey: .schemaVersion,
                in: values,
                debugDescription: "Unsupported labels schema \(version)"
            )
        }
        schemaVersion = version
        image = try values.decode(String.self, forKey: .image)
        imageWidth = try values.decode(Int.self, forKey: .imageWidth)
        imageHeight = try values.decode(Int.self, forKey: .imageHeight)
        boxes = try values.decode([LabelBox].self, forKey: .boxes)
        labeledAt = try values.decode(Date.self, forKey: .labeledAt)
    }
}

/// Manages the unlabeled / labeled screenshot queue and persistence.
@MainActor
final class LabelingController: ObservableObject {

    @Published private(set) var screenshots: [URL] = []
    @Published private(set) var index: Int = 0
    @Published private(set) var totalCount: Int = 0
    @Published private(set) var labeledCount: Int = 0
    @Published var currentBoxes: [LabelBox] = []
    @Published private(set) var currentImageSize: CGSize = .zero
    @Published private(set) var currentImage: NSImage? = nil

    /// True if the current image already has a label JSON. Drawing additional boxes
    /// will overwrite on save.
    @Published private(set) var currentIsLabeled: Bool = false
    /// Non-nil when an existing sidecar is malformed or from a future schema.
    /// Such files are read-only and never overwritten by navigation/autosave.
    @Published private(set) var currentLabelCompatibilityError: String? = nil

    /// Boxes proposed by the bundled detector but not yet confirmed by the user.
    /// Drawn in yellow. Accepting one moves it into `currentBoxes` (drawn green)
    /// and removes it from this list. Saved JSON contains only confirmed boxes.
    @Published var currentSuggestions: [LabelBox] = []

    private let visionProcessor = VisionProcessor()
    private var shutdownRequested = false

    init() {
        TrainingPaths.ensureDirectories()
        refresh()
    }

    /// Install a terminal barrier before windows are hidden. AppKit may still
    /// deliver a close callback during termination; it must not save, migrate,
    /// or discard labeling files after quit begins.
    func prepareForShutdown() {
        shutdownRequested = true
        currentSuggestions = []
    }

    /// Drop the cached CoreML model so the next labeling-suggestion call
    /// reloads from disk. Called by TrainingController after install.
    func reloadDetectionModel() {
        guard !shutdownRequested else { return }
        visionProcessor.reloadModel()
    }

    // MARK: - Disk discovery

    func refresh() {
        guard !shutdownRequested else { return }
        TrainingPaths.ensureDirectories()
        let fm = FileManager.default
        let dir = TrainingPaths.screenshots
        let urls = (try? fm.contentsOfDirectory(at: dir,
                                                includingPropertiesForKeys: nil,
                                                options: [.skipsHiddenFiles])) ?? []
        let pngs = urls
            .filter { $0.pathExtension.lowercased() == "png" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
        self.screenshots = pngs
        self.totalCount = pngs.count
        self.labeledCount = countLabeled(in: pngs)
        if index >= pngs.count { index = max(0, pngs.count - 1) }
        loadCurrent()
    }

    // MARK: - Navigation

    var currentURL: URL? {
        guard !screenshots.isEmpty, index >= 0, index < screenshots.count else { return nil }
        return screenshots[index]
    }

    func goNext(saveCurrent: Bool = true) {
        if saveCurrent { _ = saveLabels() }
        guard !screenshots.isEmpty else { return }
        index = min(index + 1, screenshots.count - 1)
        loadCurrent()
    }

    func goPrev(saveCurrent: Bool = true) {
        if saveCurrent { _ = saveLabels() }
        guard !screenshots.isEmpty else { return }
        index = max(0, index - 1)
        loadCurrent()
    }

    func goToFirstUnlabeled() {
        guard !screenshots.isEmpty else { return }
        for (i, url) in screenshots.enumerated() {
            if !FileManager.default.fileExists(atPath: TrainingPaths.labelURL(forScreenshot: url).path) {
                index = i
                loadCurrent()
                return
            }
        }
        // All labeled — go to last.
        index = screenshots.count - 1
        loadCurrent()
    }

    // MARK: - Editing

    func addBox(_ box: LabelBox) {
        currentBoxes.append(box)
    }

    func removeLastBox() {
        if !currentBoxes.isEmpty { currentBoxes.removeLast() }
    }

    func removeBox(id: UUID) {
        currentBoxes.removeAll { $0.id == id }
    }

    func clearBoxes() {
        currentBoxes.removeAll()
    }

    /// Promote a yellow proposal into a confirmed (green) box.
    func acceptSuggestion(id: UUID) {
        guard let idx = currentSuggestions.firstIndex(where: { $0.id == id }) else { return }
        let s = currentSuggestions.remove(at: idx)
        currentBoxes.append(s)
    }

    func acceptAllSuggestions() {
        currentBoxes.append(contentsOf: currentSuggestions)
        currentSuggestions.removeAll()
    }

    func dismissSuggestion(id: UUID) {
        currentSuggestions.removeAll { $0.id == id }
    }

    func dismissAllSuggestions() {
        currentSuggestions.removeAll()
    }

    /// Generate detector proposals for the current image, using the controller's
    /// own VisionProcessor. Runs off the main actor.
    func generateSuggestionsForCurrent() async {
        guard !shutdownRequested, let url = currentURL else { return }
        let detector = visionProcessor
        let detected = await detector.detectBoxesInPNGFile(at: url)

        // Only apply if the user hasn't moved on or shutdown while detection
        // was suspended off the main actor.
        guard !shutdownRequested, currentURL == url else { return }
        let confirmed = currentBoxes
        let proposals = detected.filter { proposal in
            !confirmed.contains { confirmedOverlaps(proposal, $0, threshold: 0.5) }
        }
        currentSuggestions = proposals
    }

    private func confirmedOverlaps(_ a: LabelBox, _ b: LabelBox, threshold: Double) -> Bool {
        let ax1 = a.x, ax2 = a.x + a.width
        let ay1 = a.y, ay2 = a.y + a.height
        let bx1 = b.x, bx2 = b.x + b.width
        let by1 = b.y, by2 = b.y + b.height
        let interX = max(0, min(ax2, bx2) - max(ax1, bx1))
        let interY = max(0, min(ay2, by2) - max(ay1, by1))
        let inter = interX * interY
        let union = a.width * a.height + b.width * b.height - inter
        return union > 0 && inter / union > threshold
    }

    // MARK: - Persistence

    /// Save the current state. Returns true on success or if there's nothing to save.
    @discardableResult
    func saveLabels() -> Bool {
        guard !shutdownRequested else { return false }
        guard currentLabelCompatibilityError == nil else {
            NSLog("LabelingController: refusing to overwrite incompatible label sidecar")
            return false
        }
        guard let url = currentURL else { return true }
        let labelURL = TrainingPaths.labelURL(forScreenshot: url)
        let doc = LabelDocument(image: url.lastPathComponent,
                                imageWidth: Int(currentImageSize.width.rounded()),
                                imageHeight: Int(currentImageSize.height.rounded()),
                                boxes: currentBoxes,
                                labeledAt: Date())
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            encoder.dateEncodingStrategy = .iso8601
            let data = try encoder.encode(doc)
            try data.write(to: labelURL, options: [.atomic])
            currentIsLabeled = true
            // Update count if this was the first time.
            recomputeLabeledCount()
            return true
        } catch {
            NSLog("LabelingController: save failed: \(error.localizedDescription)")
            return false
        }
    }

    func markCurrentAsNoAds() {
        guard !shutdownRequested, currentLabelCompatibilityError == nil else { return }
        currentBoxes.removeAll()
        _ = saveLabels()
    }

    /// Move the current screenshot (and any partial label) to the trash folder.
    func discardCurrent() {
        guard !shutdownRequested, let url = currentURL else { return }
        let label = TrainingPaths.labelURL(forScreenshot: url)
        guard TrainingPaths.ensureDirectories() else { return }
        do {
            // Preserve sidecars—including unknown future schemas—beside the
            // discarded screenshot without replacing an earlier trash entry.
            try LabelingFileMover.movePairToTrashNoReplace(
                screenshot: url,
                label: label,
                trash: TrainingPaths.trash
            )
        } catch {
            NSLog("LabelingController: discard failed: \(error.localizedDescription)")
            return
        }
        // Remove from list and adjust index only after the paired move commits.
        if index < screenshots.count { screenshots.remove(at: index) }
        totalCount = screenshots.count
        recomputeLabeledCount()
        if index >= screenshots.count { index = max(0, screenshots.count - 1) }
        loadCurrent()
    }

    // MARK: - Loading

    private func loadCurrent() {
        guard let url = currentURL else {
            currentImage = nil
            currentImageSize = .zero
            currentBoxes = []
            currentSuggestions = []
            currentIsLabeled = false
            currentLabelCompatibilityError = nil
            return
        }

        let img = NSImage(contentsOf: url)
        currentImage = img
        if let rep = img?.representations.first as? NSBitmapImageRep {
            currentImageSize = CGSize(width: rep.pixelsWide, height: rep.pixelsHigh)
        } else {
            currentImageSize = img?.size ?? .zero
        }

        let labelURL = TrainingPaths.labelURL(forScreenshot: url)
        currentLabelCompatibilityError = nil
        if let data = try? Data(contentsOf: labelURL) {
            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            if let doc = try? decoder.decode(LabelDocument.self, from: data) {
                currentBoxes = doc.boxes
                currentIsLabeled = true
                // Legacy sidecars had no schemaVersion. Rewrite only after a
                // successful decode so malformed/future documents stay intact.
                let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                if !shutdownRequested, object?["schemaVersion"] == nil {
                    let encoder = JSONEncoder()
                    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
                    encoder.dateEncodingStrategy = .iso8601
                    if let migrated = try? encoder.encode(doc) {
                        try? migrated.write(to: labelURL, options: .atomic)
                    }
                }
            } else {
                currentBoxes = []
                currentIsLabeled = false
                let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                if let version = object?["schemaVersion"] as? Int, version > 1 {
                    currentLabelCompatibilityError = "Labels use unsupported schema \(version); file preserved."
                } else {
                    currentLabelCompatibilityError = "Label sidecar is unreadable; file preserved."
                }
            }
        } else {
            currentBoxes = []
            currentIsLabeled = false
        }

        // Reset proposals and trigger fresh detector run in background.
        currentSuggestions = []
        Task { [weak self] in await self?.generateSuggestionsForCurrent() }
    }

    private func recomputeLabeledCount() {
        labeledCount = countLabeled(in: screenshots)
    }

    private func countLabeled(in urls: [URL]) -> Int {
        let fm = FileManager.default
        var n = 0
        for url in urls {
            let labelPath = TrainingPaths.labelURL(forScreenshot: url).path
            if fm.fileExists(atPath: labelPath) { n += 1 }
        }
        return n
    }
}
