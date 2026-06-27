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

/// A detection cached BETWEEN detection runs, stored in resolution-independent
/// normalized [0..1] coordinates with a **bottom-left** origin (CV space, the
/// same convention `AdBoundingBox.rect` uses once converted to pixels).
///
/// Why normalized: the previous code cached `AdBoundingBox` rects in ABSOLUTE
/// pixels and reused them across frames. If the captured buffer changed size
/// mid-stream (display resolution / scale change, or a reconfigure after a
/// monitor swap) those pixel rects mis-mapped onto the new frame. Caching
/// normalized rects and converting per-frame from the CURRENT buffer size
/// keeps cached detections correct across any resize.
struct NormalizedDetection: Sendable {
    /// Normalized rect in [0..1], bottom-left origin.
    let normRect: CGRect
    let confidence: Float
    let label: String

    /// Project this normalized detection onto a concrete pixel buffer size,
    /// yielding a pixel-space `AdBoundingBox` (bottom-left origin) for the
    /// inpaint pipeline.
    func adBox(inPixelBufferSize size: CGSize) -> AdBoundingBox {
        let pixelRect = CGRect(x: normRect.minX * size.width,
                               y: normRect.minY * size.height,
                               width: normRect.width * size.width,
                               height: normRect.height * size.height)
        return AdBoundingBox(rect: pixelRect,
                             confidence: confidence,
                             label: label,
                             source: .detection)
    }
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

/// Wraps a Vision/CoreML object detector around the bundled `yolov8n.mlpackage`.
///
/// IMPORTANT: the shipped model is **generic YOLOv8n trained on COCO** (80 classes:
/// `person`, `car`, `dog`, …). It is NOT an ad/logo detector. Detection mode wires
/// the pipeline end-to-end; producing actual ad masks requires a fine-tune
/// (see ASSESSMENT.md §3 Path A) or a different model entirely.
final class VisionProcessor: @unchecked Sendable {

    private let lock = NSLock()
    private var model: VNCoreMLModel?
    private var realtimeRequest: VNCoreMLRequest?  // reused on the videoQueue
    private var labelingRequest: VNCoreMLRequest?  // reused on background labeling tasks
    private var loadFailed = false
    // Unified default (was 0.35). AppController pushes the persisted value
    // (or AppController.defaultMinConfidence = 0.5) at launch via
    // setMinimumConfidence, so this is just the pre-startup fallback and now
    // matches the rest of the app instead of silently running 0.15 lower.
    private var _minimumConfidence: Float = 0.5

    var minimumConfidence: Float {
        lock.lock(); defer { lock.unlock() }
        return _minimumConfidence
    }

    init() {
        // Build lazily on first detection call so init never blocks.
    }

    /// Detects objects in `pixelBuffer` using the bundled YOLO model.
    /// Returns `[]` if the model cannot be loaded — never throws.
    ///
    /// Boxes are returned in resolution-independent NORMALIZED [0..1]
    /// coordinates (bottom-left origin) so the capture pipeline can cache them
    /// across detection intervals and project them onto whatever the current
    /// buffer size is, frame by frame.
    ///
    /// CLASS-ALLOWLIST SAFETY GATE: the shipped model is generic COCO
    /// (person/car/dog/…), NOT an ad/sponsor detector. Auto-erasing every COCO
    /// detection erased people and cars — the confirmed "class-blind erasure"
    /// bug. We gate detections through `SponsorClassAllowlist`, which is
    /// intentionally EMPTY until a real sponsor/logo model ships, so nothing
    /// auto-erases. The raw observations are still surfaced for the live
    /// detection log (`detectForDisplay`); only the auto-block path is gated.
    func detect(in pixelBuffer: CVPixelBuffer) -> [NormalizedDetection] {
        return runDetection(in: pixelBuffer).filter {
            SponsorClassAllowlist.allowsAutoBlock(label: $0.label)
        }
    }

    /// Same inference as `detect(in:)` but WITHOUT the auto-block allowlist
    /// gate. Used purely to populate the live detection log / ML detector UI so
    /// the user can see what the model sees, without any of those detections
    /// feeding the eraser. Never wire this into the inpaint path.
    func detectForDisplay(in pixelBuffer: CVPixelBuffer) -> [NormalizedDetection] {
        return runDetection(in: pixelBuffer)
    }

    /// One inference pass, two views of the result:
    ///   • `forErase` — allowlist-gated (currently empty), feeds the eraser.
    ///   • `forDisplay` — ungated, feeds the live detection log only.
    /// Use this on the hot capture path so we don't pay for inference twice.
    func detectGatedAndDisplay(in pixelBuffer: CVPixelBuffer)
        -> (forErase: [NormalizedDetection], forDisplay: [NormalizedDetection]) {
        let all = runDetection(in: pixelBuffer)
        let gated = all.filter { SponsorClassAllowlist.allowsAutoBlock(label: $0.label) }
        return (forErase: gated, forDisplay: all)
    }

    private func runDetection(in pixelBuffer: CVPixelBuffer) -> [NormalizedDetection] {
        guard let vnModel = ensureModel() else { return [] }
        let threshold = minimumConfidence  // snapshot under lock

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

        let observations = (request.results as? [VNRecognizedObjectObservation]) ?? []
        return observations.compactMap { obs -> NormalizedDetection? in
            guard let top = obs.labels.first, top.confidence >= threshold else { return nil }
            // VNRecognizedObjectObservation.boundingBox is already normalized
            // [0..1] with bottom-left origin — cache it as-is, resolution-free.
            return NormalizedDetection(normRect: obs.boundingBox,
                                       confidence: top.confidence,
                                       label: top.identifier)
        }
    }

    func updateMinimumConfidence(_ value: Float) {
        lock.lock(); defer { lock.unlock() }
        _minimumConfidence = max(0, min(1, value))
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
        let proposalThreshold: Float = 0.20  // lower than the realtime default so the user can prune false positives

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
            NSLog("VisionProcessor: loaded yolov8n CoreML model.")
            return vnModel
        } catch {
            loadFailed = true
            NSLog("VisionProcessor: failed to load yolov8n model: \(error.localizedDescription)")
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
        for ext in ["mlmodelc", "mlpackage"] {
            let url = runtimeDir.appendingPathComponent("yolov8n.\(ext)")
            if FileManager.default.fileExists(atPath: url.path) {
                NSLog("VisionProcessor: loading model from runtime dir \(url.path)")
                return try MLModel(contentsOf: url, configuration: configuration)
            }
        }

        let bundle = Bundle.main
        for ext in ["mlmodelc", "mlpackage"] {
            if let url = bundle.url(forResource: "yolov8n", withExtension: ext) {
                return try MLModel(contentsOf: url, configuration: configuration)
            }
        }
        throw NSError(domain: "VisionProcessor", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "yolov8n model not found in bundle or runtime dir"
        ])
    }
}
