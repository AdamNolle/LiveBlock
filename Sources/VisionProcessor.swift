import Foundation
import CoreVideo
import CoreImage
import CoreML
import Vision
import AppKit

enum AdSource: String, Codable, Sendable {
    case user
    case detection
}

struct AdBoundingBox: Sendable {
    let rect: CGRect          // pixel coords, bottom-left origin (CV space)
    let confidence: Float
    let label: String
    let source: AdSource
    /// Set when this box originates from a user-drawn region; lets the
    /// capture pipeline attribute blocking events to a specific region.
    let regionID: UUID?

    init(rect: CGRect, confidence: Float, label: String, source: AdSource, regionID: UUID? = nil) {
        self.rect = rect
        self.confidence = confidence
        self.label = label
        self.source = source
        self.regionID = regionID
    }
}

/// Wraps a Vision/CoreML object detector around the bundled
/// `liveblock-detector.mlpackage`.
///
/// The bundled model is the open-vocabulary detector baked by
/// `tools/build_openvocab.py` from YOLO-World-v2 — one primary prompt for each
/// class in `tools/vocab/liveblock-vocab.json` (Logo / Ad banner / Sponsored),
/// exported NMS-baked to CoreML. No training data: the vocabulary is text, and
/// it generalises to unseen brands. Per-class enable flags + score thresholds
/// are read from the shared `liveblock-config` store via `DetectionVocabulary`.
final class VisionProcessor: @unchecked Sendable {

    private let lock = NSLock()
    private var model: VNCoreMLModel?
    private var realtimeRequest: VNCoreMLRequest?  // reused on the videoQueue
    private var labelingRequest: VNCoreMLRequest?  // reused on background labeling tasks
    private var loadFailed = false
    private var _minimumConfidence: Float = 0.25

    /// Per-class vocabulary + thresholds from the shared Rust config. Built
    /// lazily on first detection call so init never blocks.
    private var _vocabulary: DetectionVocabulary?

    var minimumConfidence: Float {
        lock.lock(); defer { lock.unlock() }
        return _minimumConfidence
    }

    init() {
        // Build lazily on first detection call so init never blocks.
    }

    /// Where the persisted per-class detection settings live.
    private static var detectionSettingsURL: URL {
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support")
        return support
            .appendingPathComponent("LiveBlock", isDirectory: true)
            .appendingPathComponent("detection-settings.json")
    }

    /// Lazily open the vocabulary store, seeding from the bundled vocab JSON on
    /// first run. Caller holds `lock`.
    private func ensureVocabularyLocked() -> DetectionVocabulary {
        if let v = _vocabulary { return v }
        let bundledJSON = Bundle.main.url(forResource: "liveblock-vocab", withExtension: "json")
            .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
        let v = DetectionVocabulary(settingsURL: Self.detectionSettingsURL,
                                    bundledVocabJSON: bundledJSON)
        _vocabulary = v
        return v
    }

    /// Detects objects in `pixelBuffer` using the bundled YOLO model.
    /// Returns `[]` if the model cannot be loaded — never throws.
    func detect(in pixelBuffer: CVPixelBuffer) -> [AdBoundingBox] {
        guard let vnModel = ensureModel() else { return [] }
        let globalThreshold: Float
        let vocabulary: DetectionVocabulary
        do {
            lock.lock(); defer { lock.unlock() }
            globalThreshold = _minimumConfidence
            vocabulary = ensureVocabularyLocked()
        }

        // Reuse the cached request — VNCoreMLRequest is designed for create-once-reuse.
        let request: VNCoreMLRequest = {
            lock.lock(); defer { lock.unlock() }
            if let r = realtimeRequest { return r }
            let r = VNCoreMLRequest(model: vnModel)
            r.imageCropAndScaleOption = .scaleFill
            realtimeRequest = r
            return r
        }()

        let handler = VNImageRequestHandler(cvPixelBuffer: pixelBuffer, orientation: .up)
        do {
            try handler.perform([request])
        } catch {
            NSLog("VisionProcessor: detect failed: \(error.localizedDescription)")
            return []
        }

        let width = CGFloat(CVPixelBufferGetWidth(pixelBuffer))
        let height = CGFloat(CVPixelBufferGetHeight(pixelBuffer))

        let observations = (request.results as? [VNRecognizedObjectObservation]) ?? []
        return observations.compactMap { obs -> AdBoundingBox? in
            guard let top = obs.labels.first else { return nil }
            // Per-class effective threshold keyed on the vocab class name. A nil
            // result means the class is known and disabled -> drop the detection.
            guard let threshold = vocabulary.effectiveThreshold(forLabel: top.identifier,
                                                                globalFallback: globalThreshold),
                  top.confidence >= threshold else { return nil }
            // VNRecognizedObjectObservation.boundingBox is normalized [0..1] with origin bottom-left.
            let bb = obs.boundingBox
            let pixelRect = CGRect(x: bb.minX * width,
                                   y: bb.minY * height,
                                   width: bb.width * width,
                                   height: bb.height * height)
            return AdBoundingBox(rect: pixelRect,
                                 confidence: top.confidence,
                                 label: top.identifier,
                                 source: .detection)
        }
    }

    func updateMinimumConfidence(_ value: Float) {
        lock.lock(); defer { lock.unlock() }
        _minimumConfidence = max(0, min(1, value))
    }

    func detectorRules() -> [DetectorClassRule] {
        lock.lock(); defer { lock.unlock() }
        return ensureVocabularyLocked().rules
    }

    @discardableResult
    func setDetectorClassEnabled(id: UInt32, enabled: Bool) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return ensureVocabularyLocked().setClassEnabled(id: id, enabled: enabled)
    }

    /// Detect against a PNG on disk. Returns proposals as `LabelBox` (normalized,
    /// top-left origin) suitable for the labeling UI. Lower confidence threshold
    /// than realtime detection so the user can prune false positives quickly.
    func detectBoxesInPNGFile(at url: URL) async -> [LabelBox] {
        guard let vnModel = ensureModel() else { return [] }
        guard let nsImage = NSImage(contentsOf: url),
              let cgImage = nsImage.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            return []
        }

        let request: VNCoreMLRequest = {
            lock.lock(); defer { lock.unlock() }
            if let r = labelingRequest { return r }
            let r = VNCoreMLRequest(model: vnModel)
            r.imageCropAndScaleOption = .scaleFill
            labelingRequest = r
            return r
        }()
        let proposalThreshold: Float = 0.20  // lower than the 0.35 realtime default

        let handler = VNImageRequestHandler(cgImage: cgImage, orientation: .up)
        do {
            try handler.perform([request])
        } catch {
            NSLog("VisionProcessor: PNG detect failed: \(error.localizedDescription)")
            return []
        }

        let observations = (request.results as? [VNRecognizedObjectObservation]) ?? []
        return observations.compactMap { obs -> LabelBox? in
            guard let top = obs.labels.first, top.confidence >= proposalThreshold else { return nil }
            // VNRecognizedObjectObservation rect is bottom-left origin, normalized.
            // LabelBox uses top-left origin, normalized.
            let bb = obs.boundingBox
            return LabelBox(x: Double(bb.minX),
                            y: Double(1.0 - bb.maxY),
                            width: Double(bb.width),
                            height: Double(bb.height))
        }
    }

    // MARK: - Model loading

    /// Where the training pipeline drops the freshly trained model so the
    /// running app picks it up on next inference call.
    private static var runtimeModelDirectory: URL {
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent("LiveBlock/models", isDirectory: true)
    }

    /// Public entry the TrainingController calls after a successful install.
    /// Clears the cached VNCoreMLModel so the very next detection call
    /// reloads from disk, picking up whatever the training pipeline wrote
    /// into Application Support/LiveBlock/models/.
    func reloadModel() {
        lock.lock()
        defer { lock.unlock() }
        model = nil
        realtimeRequest = nil
        labelingRequest = nil
        loadFailed = false
        NSLog("VisionProcessor: model cache cleared; will reload on next inference.")
    }

    private func ensureModel() -> VNCoreMLModel? {
        lock.lock(); defer { lock.unlock() }
        if let model { return model }
        if loadFailed { return nil }

        do {
            let mlModel = try Self.loadModel()
            let vnModel = try VNCoreMLModel(for: mlModel)
            self.model = vnModel
            NSLog("VisionProcessor: loaded liveblock-detector CoreML model.")
            return vnModel
        } catch {
            loadFailed = true
            NSLog("VisionProcessor: failed to load liveblock-detector model: \(error.localizedDescription)")
            return nil
        }
    }

    /// Look up the model in priority order:
    ///   1. The runtime directory the trainer writes to (newer than bundle).
    ///   2. The app bundle (the original ships-with-app default).
    private static func loadModel() throws -> MLModel {
        let configuration = MLModelConfiguration()
        configuration.computeUnits = .all

        let runtimeDir = runtimeModelDirectory
        let runtimeCandidates = ["mlmodelc", "mlpackage"]
            .map { runtimeDir.appendingPathComponent("liveblock-detector.\($0)") }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
            .sorted {
                let lhs = (try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                let rhs = (try? $1.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                return lhs > rhs
            }
        if let url = runtimeCandidates.first {
            NSLog("VisionProcessor: loading newest runtime model \(url.path)")
            return try MLModel(contentsOf: url, configuration: configuration)
        }

        let bundle = Bundle.main
        for ext in ["mlmodelc", "mlpackage"] {
            if let url = bundle.url(forResource: "liveblock-detector", withExtension: ext) {
                return try MLModel(contentsOf: url, configuration: configuration)
            }
        }
        throw NSError(domain: "VisionProcessor", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "liveblock-detector model not found in bundle or runtime dir"
        ])
    }
}
