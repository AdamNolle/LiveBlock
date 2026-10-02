import Foundation
import ScreenCaptureKit
import CoreGraphics
import CoreVideo
import AppKit
import Combine
import QuartzCore
import ImageIO
import UniformTypeIdentifiers

/// Why the capture pipeline isn't actively blocking right now.
/// Surfaced in the Control Panel banner and Mini HUD so the user can
/// always see at a glance whether something's wrong vs deliberately paused.
enum PauseReason: Equatable {
    case none                       // running and unpaused — green
    case stopped                    // not running by user choice — neutral
    case userPaused                 // paused via the Pause button — amber
    case fullscreenApp(String)      // frontmost is fullscreen and pauseOnFullscreen is on — amber
    case excludedApp(String)        // frontmost is in the per-app exclude list — amber
    case permissionDenied           // Screen Recording revoked — red
    case startError(String)         // capture.start() threw — red
}

@MainActor
final class ScreenCaptureManager: NSObject, ObservableObject, SCStreamOutput, SCStreamDelegate {

    // MARK: - Published state (read by SwiftUI)
    @Published private(set) var isRunning = false
    @Published private(set) var isStarting = false
    @Published private(set) var isCoverOnly = false
    @Published private(set) var currentPatches: [InpaintPatch] = []
    @Published private(set) var lastDetectionLabels: [String] = []
    @Published private(set) var framesPerSecond: Double = 0
    @Published private(set) var patchesProduced: Int = 0
    /// Per-region blocking-event counter, keyed by region UUID.
    @Published private(set) var blocksByRegion: [UUID: Int] = [:]
    /// Last error from `start()`, if any. Cleared on successful start or
    /// stop. The Control Panel banner reads this so the user sees WHY
    /// pressing Start did nothing instead of a silent toggle flip.
    @Published private(set) var lastStartError: String? = nil

    /// Automatic detection is opt-in; manual regions work independently.
    @Published var detectionEnabled = false {
        didSet { detectionEnabledStorage.set(detectionEnabled) }
    }

    /// Set by AppController from the @AppStorage("pauseOnFullscreen") toggle.
    /// When `true`, the SCStream callback skips detection + inpaint while
    /// the frontmost app is fullscreen.
    @Published var pauseOnFullscreen: Bool = true {
        didSet { pauseOnFullscreenStorage.set(pauseOnFullscreen) }
    }
    /// Updated by AppController on `NSWorkspace.didActivateApplicationNotification`.
    /// When `true`, capture pipeline pauses.
    @Published var frontmostIsFullscreen: Bool = false {
        didSet { frontmostFullscreenStorage.set(frontmostIsFullscreen) }
    }
    /// Set by AppController when the frontmost app's bundle ID is excluded.
    @Published var frontmostIsExcluded: Bool = false {
        didSet {
            frontmostExcludedStorage.set(frontmostIsExcluded)
            refreshCover()
        }
    }

    // MARK: - Public API
    let regionStore: RegionStore

    // MARK: - Internals
    private var stream: SCStream?
    private var lifecycleGeneration = 0
    nonisolated private let streamGate = StreamGate()
    /// The screen capture is currently bound to. Exposed read-only so the
    /// AppController can keep the overlay + editor aligned to the captured
    /// display across multi-monitor changes.
    private(set) var targetScreen: NSScreen?

    nonisolated private let visionProcessor = VisionProcessor()
    nonisolated private let inpaintingEngine = InpaintingEngine()

    nonisolated private let videoQueue = DispatchQueue(label: "com.liveblock.videoQueue", qos: .userInteractive)

    // Atomically swappable booleans (read by nonisolated frame callback)
    nonisolated private let detectionEnabledStorage = AtomicBool(false)
    nonisolated private let pauseOnFullscreenStorage = AtomicBool(true)
    nonisolated private let frontmostFullscreenStorage = AtomicBool(false)
    nonisolated private let frontmostExcludedStorage = AtomicBool(false)
    nonisolated private let fpsCounter = FPSCounter()

    // Frame counter — drives the "detect every Nth frame" cadence.
    nonisolated private let frameCounter = AtomicCounter()
    nonisolated private let detectionInterval = 4 // 4 → ~15Hz at 60fps capture

    // Detection results cached between detection runs.
    // `detectionCache` holds allowlist-gated boxes that feed the eraser
    // (currently empty). `displayDetectionCache` holds the ungated boxes used
    // only for the live ML-detector log so the UI shows what the model sees.
    nonisolated private let detectionCache = DetectionCache()
    nonisolated private let displayDetectionCache = DetectionCache()

    // Throttle main-actor patch updates.
    nonisolated private let lastEmitClock = AtomicTime()
    nonisolated private let minEmitInterval: TimeInterval = 1.0 / 30.0

    // Latest pixel buffer, retained for hotkey-triggered screenshot saves.
    nonisolated private let latestBufferStorage = LatestBufferStorage()

    // Shared CIContext for PNG encoding (heavy to construct).
    nonisolated private let pngContext = CIContext(options: [.useSoftwareRenderer: false])

    /// Tracks whether we paused capture because the system slept or the
    /// screen locked. On wake/unlock we auto-resume only if the user had
    /// capture running before sleep — otherwise we leave it as it was.
    private var sleepResumeRequested: Bool = false
    private var sleepObservers: [NSObjectProtocol] = []

    init(regionStore: RegionStore) {
        self.regionStore = regionStore
        super.init()
        installSleepObservers()
    }

    deinit {
        for obs in sleepObservers {
            NSWorkspace.shared.notificationCenter.removeObserver(obs)
        }
    }

    /// Pause capture on sleep / display lock, resume on wake / unlock.
    /// Without this, capture runs through sleep, drains battery, and can
    /// produce garbage frames if the display reconfigures while suspended.
    private func installSleepObservers() {
        let nc = NSWorkspace.shared.notificationCenter
        let pauseNames: [Notification.Name] = [
            NSWorkspace.willSleepNotification,
            NSWorkspace.screensDidSleepNotification
        ]
        let resumeNames: [Notification.Name] = [
            NSWorkspace.didWakeNotification,
            NSWorkspace.screensDidWakeNotification
        ]
        for name in pauseNames {
            let obs = nc.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor [weak self] in
                    guard let self, self.isRunning else { return }
                    NSLog("ScreenCaptureManager: pausing for sleep/lock.")
                    self.sleepResumeRequested = true
                    await self.stop()
                }
            }
            sleepObservers.append(obs)
        }
        for name in resumeNames {
            let obs = nc.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor [weak self] in
                    guard let self, self.sleepResumeRequested else { return }
                    self.sleepResumeRequested = false
                    if let screen = self.targetScreen ?? NSScreen.main {
                        NSLog("ScreenCaptureManager: resuming after wake.")
                        await self.start(on: screen)
                    }
                }
            }
            sleepObservers.append(obs)
        }
    }

    // MARK: - Lifecycle

    func start(on screen: NSScreen) async {
        guard !isRunning, !isStarting else { return }
        isCoverOnly = false
        isStarting = true
        lifecycleGeneration &+= 1
        let generation = lifecycleGeneration
        lastStartError = nil
        // ScreenCaptureKit can leave its completion pending after a service
        // interruption. Bound the visible start state and permit a fresh retry.
        let timeout = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(10))
            guard !Task.isCancelled, let self,
                  generation == self.lifecycleGeneration, self.isStarting else { return }
            self.lifecycleGeneration &+= 1
            self.isStarting = false
            self.streamGate.clear()
            let stalledStream = self.stream
            self.stream = nil
            self.currentPatches = []
            self.detectionCache.store([])
            self.displayDetectionCache.store([])
            self.lastDetectionLabels = []
            self.framesPerSecond = 0
            self.latestBufferStorage.cancelPending()
            self.lastStartError = "The macOS capture service did not respond. Try again; if this persists, relaunch LiveBlock or restart your Mac."
            if let stalledStream {
                Task { try? await stalledStream.stopCapture() }
            }
        }
        defer {
            timeout.cancel()
            if generation == lifecycleGeneration { isStarting = false }
        }
        targetScreen = screen
        // Pre-flight: if Screen Recording is denied, fail fast with a clear
        // message instead of a generic SCStream error.
        guard Permissions.screenRecordingGranted() else {
            self.lastStartError = "Screen Recording permission needed. Open System Settings → Privacy & Security → Screen Recording, enable LiveBlock, and try again."
            NSLog("ScreenCaptureManager: refusing to start — Screen Recording denied.")
            return
        }
        do {
            let availableContent = try await SCShareableContent.excludingDesktopWindows(false,
                                                                                        onScreenWindowsOnly: true)
            guard generation == lifecycleGeneration else { return }
            guard let display = pickDisplay(for: screen, in: availableContent.displays) else {
                self.lastStartError = "No display matched the target screen."
                NSLog("ScreenCaptureManager: no display matched target screen.")
                return
            }

            // Use pixel dimensions, not points (S7).
            let scale = screen.backingScaleFactor
            let pixelWidth = Int(screen.frame.width * scale)
            let pixelHeight = Int(screen.frame.height * scale)

            // Exclude the whole application, including windows opened later.
            // ScreenCaptureKit owns self-exclusion; sharing flags alone are
            // not a reliable capture filter on recent macOS releases.
            let ownPID = ProcessInfo.processInfo.processIdentifier
            let selfApps = availableContent.applications.filter { $0.processID == ownPID }
            guard !selfApps.isEmpty else {
                self.lastStartError = "LiveBlock could not exclude its own overlay from capture. Show the control panel and try again."
                return
            }
            let filter = SCContentFilter(display: display,
                                         excludingApplications: selfApps,
                                         exceptingWindows: [])

            let configuration = SCStreamConfiguration()
            configuration.width = pixelWidth
            configuration.height = pixelHeight
            configuration.showsCursor = true
            configuration.minimumFrameInterval = CMTime(value: 1, timescale: 60)
            configuration.queueDepth = 6
            configuration.pixelFormat = kCVPixelFormatType_32BGRA

            let newStream = SCStream(filter: filter, configuration: configuration, delegate: self)
            try newStream.addStreamOutput(self, type: .screen, sampleHandlerQueue: videoQueue)
            self.stream = newStream
            streamGate.activate(newStream)
            try await newStream.startCapture()
            guard generation == lifecycleGeneration else {
                try? await newStream.stopCapture()
                return
            }

            self.stream = newStream
            self.isRunning = true
            self.lastStartError = nil
            NSLog("ScreenCaptureManager: capture started (\(pixelWidth)x\(pixelHeight)).")
        } catch {
            guard generation == lifecycleGeneration else { return }
            streamGate.clear()
            self.stream = nil
            self.lastStartError = error.localizedDescription
            NSLog("ScreenCaptureManager: failed to start capture: \(error.localizedDescription)")
        }
    }

    /// Ordinary opaque overlays. This path never creates a capture stream.
    func startCover(on screen: NSScreen) {
        lifecycleGeneration &+= 1
        streamGate.clear()
        let oldStream = stream
        stream = nil
        isStarting = false
        isCoverOnly = true
        detectionEnabled = false
        detectionCache.store([])
        displayDetectionCache.store([])
        latestBufferStorage.cancelPending()
        lastDetectionLabels = []
        lastStartError = nil
        framesPerSecond = 0
        targetScreen = screen
        refreshCover()
        isRunning = true
        if let oldStream { Task { try? await oldStream.stopCapture() } }
    }

    func refreshCover() {
        guard isCoverOnly, let screen = targetScreen else { return }
        let size = CGSize(width: screen.frame.width * screen.backingScaleFactor,
                          height: screen.frame.height * screen.backingScaleFactor)
        let regions = frontmostIsExcluded ? [] : regionStore.enabled
        let boxes = regions.map {
            AdBoundingBox(rect: $0.cvRect(inPixelBufferSize: size), confidence: 1,
                          label: "User Selected", source: .user, regionID: $0.id)
        }
        currentPatches = inpaintingEngine.paintOverPatches(regions: boxes, bufferSize: size)
    }

    func stop() async {
        lifecycleGeneration &+= 1
        isStarting = false
        streamGate.clear()
        let oldStream = stream
        self.stream = nil
        self.isRunning = false
        self.isCoverOnly = false
        self.currentPatches = []
        self.lastDetectionLabels = []
        self.lastStartError = nil
        detectionCache.store([])
        displayDetectionCache.store([])
        latestBufferStorage.cancelPending()
        framesPerSecond = 0
        if let oldStream {
            do { try await oldStream.stopCapture() }
            catch { NSLog("ScreenCaptureManager: stop failed: \(error.localizedDescription)") }
        }
    }

    func setMinimumConfidence(_ value: Float) {
        visionProcessor.updateMinimumConfidence(value)
    }

    /// Drop the cached CoreML model so the very next detection call reloads
    /// from disk. Used by TrainingController after a successful install so
    /// the running app picks up the freshly trained model without relaunch.
    nonisolated func reloadDetectionModel() {
        visionProcessor.reloadModel()
        detectionCache.store([])
        displayDetectionCache.store([])
    }

    // MARK: - SCStreamOutput
    nonisolated func stream(_ stream: SCStream,
                            didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
                            of type: SCStreamOutputType) {
        guard streamGate.contains(stream), type == .screen, sampleBuffer.isValid,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let status = attachments.first?[.status] as? Int else { return }
        if [SCFrameStatus.blank.rawValue, SCFrameStatus.suspended.rawValue, SCFrameStatus.stopped.rawValue].contains(status) {
            detectionCache.store([])
            displayDetectionCache.store([])
            latestBufferStorage.cancelPending()
            Task { @MainActor [weak self] in
                guard let self, self.streamGate.contains(stream) else { return }
                self.currentPatches = []
                self.lastDetectionLabels = []
            }
            return
        }
        guard status == SCFrameStatus.complete.rawValue,
              let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }

        // Resolve any pending screenshot request synchronously here so the
        // CGImage detaches from the recycled SCStream buffer pool.
        latestBufferStorage.ingest(pixelBuffer, context: pngContext)

        let bufferWidth = CGFloat(CVPixelBufferGetWidth(pixelBuffer))
        let bufferHeight = CGFloat(CVPixelBufferGetHeight(pixelBuffer))
        let bufferSize = CGSize(width: bufferWidth, height: bufferHeight)

        let frameIdx = frameCounter.increment()
        let fps = fpsCounter.tick()

        // Pause hooks: skip detection + inpaint when the frontmost app is
        // fullscreen (and the user opted in) or when it's on the per-app
        // exclusion list. We still ingest the buffer above so the labeling
        // hotkey keeps working, and we still publish FPS so the UI shows
        // the pipeline is alive.
        let pausedForFullscreen = pauseOnFullscreenStorage.get() && frontmostFullscreenStorage.get()
        let pausedForApp = frontmostExcludedStorage.get()
        let paused = pausedForFullscreen || pausedForApp
        if paused || !detectionEnabledStorage.get() {
            detectionCache.store([])
            displayDetectionCache.store([])
        }

        // One inference pass supplies semantic removals and display labels.
        // Cache normalized boxes between inference ticks.
        if !paused, detectionEnabledStorage.get(), frameIdx % detectionInterval == 0 {
            // Single inference pass; gated set feeds the eraser (empty until a
            // real sponsor model ships), ungated set feeds the live UI log.
            let result = visionProcessor.detectGatedAndDisplay(in: pixelBuffer)
            detectionCache.store(result.forErase)
            displayDetectionCache.store(result.forDisplay)
        }

        // Build the active set of regions for this frame.
        let userRegions = paused ? [] : regionStore.enabled
        let userBoxes = userRegions.map { region -> AdBoundingBox in
            AdBoundingBox(rect: region.cvRect(inPixelBufferSize: bufferSize),
                          confidence: 1.0,
                          label: "User Selected",
                          source: .user,
                          regionID: region.id)
        }
        // Model boxes with explicit removable class names. Converted from
        // the normalized cache to pixel space for THIS frame's buffer size.
        let detectedBoxes = (!paused && detectionEnabledStorage.get())
            ? detectionCache.load(forPixelBufferSize: bufferSize)
            : []
        let allBoxes = userBoxes + detectedBoxes

        // DRM-safe paint-over split. For every active region, sample the
        // captured buffer: if it reads as protected/near-uniform black (the
        // HDCP/DRM blanking signature) we draw a SOLID opaque cover instead of
        // mirror-blending — mirror-blend on black only smears black, and we must
        // not rely on reading protected pixels. Everything else takes the normal
        // content-extrapolating inpaint path.
        //
        // Note: on a fully-protected video the WHOLE frame is black, so there's
        // nothing to auto-detect beyond the regions the user marked — paint-over
        // is then driven entirely by those user/static regions, exactly the
        // capture-free PaintOver design.
        var protectedBoxes: [AdBoundingBox] = []
        var inpaintBoxes: [AdBoundingBox] = []
        for box in allBoxes {
            let normTopLeft = Self.normalizedTopLeftRect(forCVRect: box.rect, bufferSize: bufferSize)
            if ProtectedRegionDetector.regionIsProtectedBlack(pixelBuffer, normRect: normTopLeft) {
                protectedBoxes.append(box)
            } else {
                inpaintBoxes.append(box)
            }
        }

        // Inpaint every frame regardless — boxes may have moved or disappeared.
        var patches = inpaintingEngine.inpaintPatches(frame: pixelBuffer, regions: inpaintBoxes)
        // Append solid opaque-black covers for the protected regions.
        patches += inpaintingEngine.paintOverPatches(regions: protectedBoxes, bufferSize: bufferSize)

        // Per-region blocking-event count. Increment once per active user region.
        var perRegion: [UUID: Int] = [:]
        for box in userBoxes {
            if let id = box.regionID { perRegion[id, default: 0] += 1 }
        }

        // Throttle SwiftUI updates to ≤30Hz.
        guard lastEmitClock.shouldEmit(minInterval: minEmitInterval) else { return }

        let displayBoxes = (!paused && detectionEnabledStorage.get())
            ? displayDetectionCache.load(forPixelBufferSize: bufferSize)
            : []
        let labels = displayBoxes.map { "\($0.label) (\(Int($0.confidence * 100))%)" }
        let patchCount = patches.count
        Task { @MainActor [weak self] in
            guard let self, self.isRunning, self.streamGate.contains(stream) else { return }
            self.currentPatches = patches
            self.lastDetectionLabels = labels
            self.framesPerSecond = fps
            self.patchesProduced &+= patchCount
            for (id, n) in perRegion {
                self.blocksByRegion[id, default: 0] += n
            }
        }
    }

    nonisolated func stream(_ stream: SCStream, didStopWithError error: Error) {
        NSLog("ScreenCaptureManager: stream stopped with error: \(error.localizedDescription)")
        Task { @MainActor [weak self] in
            guard let self, self.streamGate.contains(stream) else { return }
            self.lastStartError = error.localizedDescription
            await self.stop()
            self.lastStartError = error.localizedDescription
        }
    }

    // MARK: - Screenshot for labeling

    /// Save the most recent captured frame as a PNG. Awaits the next stream
    /// callback to obtain a CGImage that's detached from the SCStream buffer
    /// pool, then PNG-encodes on a background queue.
    nonisolated func saveLatestFrameForLabeling() async -> URL? {
        guard streamGate.isActive else { return nil }
        guard let cgImage = await latestBufferStorage.requestSnapshot() else { return nil }
        return await withCheckedContinuation { (cont: CheckedContinuation<URL?, Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                TrainingPaths.ensureDirectories()
                let stem = TrainingPaths.newScreenshotStem()
                let url = TrainingPaths.screenshots.appendingPathComponent("\(stem).png")
                guard let dest = CGImageDestinationCreateWithURL(url as CFURL,
                                                                  "public.png" as CFString,
                                                                  1, nil) else {
                    cont.resume(returning: nil); return
                }
                CGImageDestinationAddImage(dest, cgImage, nil)
                if CGImageDestinationFinalize(dest) {
                    cont.resume(returning: url)
                } else {
                    NSLog("ScreenCaptureManager: PNG finalize failed")
                    cont.resume(returning: nil)
                }
            }
        }
    }

    /// Convert a CV-space pixel rect (bottom-left origin) into a normalized
    /// [0..1] TOP-LEFT rect, the convention `ProtectedRegionDetector` (and
    /// `NormalizedRegion`) expect. Y is flipped because pixel buffers are
    /// row-major from the top while CV rects originate bottom-left.
    nonisolated static func normalizedTopLeftRect(forCVRect rect: CGRect,
                                                  bufferSize: CGSize) -> CGRect {
        guard bufferSize.width > 0, bufferSize.height > 0 else { return .zero }
        return CGRect(x: rect.minX / bufferSize.width,
                      y: (bufferSize.height - rect.maxY) / bufferSize.height,
                      width: rect.width / bufferSize.width,
                      height: rect.height / bufferSize.height)
    }

    // MARK: - Display selection (S8)
    nonisolated private func pickDisplay(for screen: NSScreen, in displays: [SCDisplay]) -> SCDisplay? {
        let targetID = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
        if let targetID, let match = displays.first(where: { $0.displayID == targetID }) {
            return match
        }
        return displays.first
    }
}

// MARK: - Lock-free atomics (small enough to be sound under NSLock)

final class AtomicBool: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Bool
    init(_ initial: Bool) { self.value = initial }
    func get() -> Bool { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ newValue: Bool) { lock.lock(); value = newValue; lock.unlock() }
}

final class AtomicCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Int = 0
    func increment() -> Int {
        lock.lock(); defer { lock.unlock() }
        value &+= 1
        return value
    }
}

final class AtomicTime: @unchecked Sendable {
    private let lock = NSLock()
    private var lastEmit: TimeInterval = 0
    func shouldEmit(minInterval: TimeInterval) -> Bool {
        let now = CACurrentMediaTime()
        lock.lock(); defer { lock.unlock() }
        if now - lastEmit >= minInterval {
            lastEmit = now
            return true
        }
        return false
    }
}

/// Caches detections BETWEEN detection runs so every frame can inpaint even
/// though we only run the model every Nth frame. Stores resolution-independent
/// NORMALIZED detections (not absolute pixels): the cached set is projected
/// onto the CURRENT buffer size on every frame via `load(forPixelBufferSize:)`,
/// so a mid-stream resolution change can never mis-map stale pixel rects.
final class DetectionCache: @unchecked Sendable {
    private let lock = NSLock()
    private var detections: [NormalizedDetection] = []
    func store(_ newDetections: [NormalizedDetection]) {
        lock.lock(); detections = newDetections; lock.unlock()
    }
    /// Project the cached normalized detections onto a concrete buffer size,
    /// returning pixel-space boxes for the current frame.
    func load(forPixelBufferSize size: CGSize) -> [AdBoundingBox] {
        lock.lock(); let snapshot = detections; lock.unlock()
        return snapshot.map { $0.adBox(inPixelBufferSize: size) }
    }
}

/// Rolling-average frames-per-second over the last second of stream callbacks.
/// Cheap enough to call on every frame.
final class FPSCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var samples: [TimeInterval] = []
    private let window: TimeInterval = 1.0

    func tick() -> Double {
        let now = CACurrentMediaTime()
        lock.lock(); defer { lock.unlock() }
        samples.append(now)
        let cutoff = now - window
        while let first = samples.first, first < cutoff {
            samples.removeFirst()
        }
        return Double(samples.count) / window
    }
}

/// Request-driven snapshot store. Avoids pinning a recycled CVPixelBuffer
/// out of the SCStream queue. When `requestSnapshot()` is called, the next
/// `ingest(...)` from the stream callback materializes a detached CGImage
/// and resolves the continuation. After delivery, no buffer is held.
final class LatestBufferStorage: @unchecked Sendable {
    private let lock = NSLock()
    private var pendingContinuations: [UUID: CheckedContinuation<CGImage?, Never>] = [:]

    func requestSnapshot() async -> CGImage? {
        await withCheckedContinuation { cont in
            lock.lock()
            let id = UUID()
            pendingContinuations[id] = cont
            lock.unlock()
            DispatchQueue.global().asyncAfter(deadline: .now() + 2) { [self] in
                lock.lock()
                let expired = pendingContinuations.removeValue(forKey: id)
                lock.unlock()
                expired?.resume(returning: nil)
            }
        }
    }

    /// Called from the stream callback on the videoQueue. Renders only if
    /// at least one snapshot has been requested.
    func ingest(_ pixelBuffer: CVPixelBuffer, context: CIContext) {
        let conts: [CheckedContinuation<CGImage?, Never>] = {
            lock.lock(); defer { lock.unlock() }
            let c = Array(pendingContinuations.values)
            pendingContinuations.removeAll()
            return c
        }()
        guard !conts.isEmpty else { return }

        let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
        let cgImage = context.createCGImage(ciImage, from: ciImage.extent)
        for c in conts { c.resume(returning: cgImage) }
    }

    func cancelPending() {
        lock.lock()
        let pending = Array(pendingContinuations.values)
        pendingContinuations.removeAll()
        lock.unlock()
        for continuation in pending { continuation.resume(returning: nil) }
    }
}

/// Reject queued callbacks from a stopped or replaced capture session.
final class StreamGate: @unchecked Sendable {
    private let lock = NSLock()
    private var active: ObjectIdentifier?
    var isActive: Bool { lock.lock(); defer { lock.unlock() }; return active != nil }
    func activate(_ stream: SCStream) { lock.lock(); active = ObjectIdentifier(stream); lock.unlock() }
    func clear() { lock.lock(); active = nil; lock.unlock() }
    func contains(_ stream: SCStream) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return active == ObjectIdentifier(stream)
    }
}
