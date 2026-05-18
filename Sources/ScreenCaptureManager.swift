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

    /// Detection on by default so a fresh user sees something happen — they
    /// can flip it off in Settings if they only want manual regions.
    @Published var detectionEnabled = true {
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
        didSet { frontmostExcludedStorage.set(frontmostIsExcluded) }
    }

    // MARK: - Public API
    let regionStore: RegionStore

    // MARK: - Internals
    private var stream: SCStream?
    private var targetScreen: NSScreen?

    nonisolated private let visionProcessor = VisionProcessor()
    nonisolated private let inpaintingEngine = InpaintingEngine()

    nonisolated private let videoQueue = DispatchQueue(label: "com.liveblock.videoQueue", qos: .userInteractive)

    // Atomically swappable booleans (read by nonisolated frame callback)
    nonisolated private let detectionEnabledStorage = AtomicBool(true)
    nonisolated private let pauseOnFullscreenStorage = AtomicBool(true)
    nonisolated private let frontmostFullscreenStorage = AtomicBool(false)
    nonisolated private let frontmostExcludedStorage = AtomicBool(false)
    nonisolated private let fpsCounter = FPSCounter()

    // Frame counter — drives the "detect every Nth frame" cadence.
    nonisolated private let frameCounter = AtomicCounter()
    nonisolated private let detectionInterval = 4 // 4 → ~15Hz at 60fps capture

    // Detection results cached between detection runs.
    nonisolated private let detectionCache = DetectionCache()

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
        guard !isRunning else { return }
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
            guard let display = pickDisplay(for: screen, in: availableContent.displays) else {
                self.lastStartError = "No display matched the target screen."
                NSLog("ScreenCaptureManager: no display matched target screen.")
                return
            }

            // Use pixel dimensions, not points (S7).
            let scale = screen.backingScaleFactor
            let pixelWidth = Int(screen.frame.width * scale)
            let pixelHeight = Int(screen.frame.height * scale)

            let filter = SCContentFilter(display: display,
                                         excludingApplications: [],
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
            try await newStream.startCapture()

            self.stream = newStream
            self.isRunning = true
            self.lastStartError = nil
            NSLog("ScreenCaptureManager: capture started (\(pixelWidth)x\(pixelHeight)).")
        } catch {
            self.lastStartError = error.localizedDescription
            NSLog("ScreenCaptureManager: failed to start capture: \(error.localizedDescription)")
        }
    }

    func stop() async {
        guard let stream else {
            isRunning = false
            return
        }
        do {
            try await stream.stopCapture()
        } catch {
            NSLog("ScreenCaptureManager: stop failed: \(error.localizedDescription)")
        }
        self.stream = nil
        self.isRunning = false
        self.currentPatches = []
        self.lastDetectionLabels = []
        self.lastStartError = nil
    }

    func setMinimumConfidence(_ value: Float) {
        visionProcessor.updateMinimumConfidence(value)
    }

    /// Drop the cached CoreML model so the very next detection call reloads
    /// from disk. Used by TrainingController after a successful install so
    /// the running app picks up the freshly trained model without relaunch.
    nonisolated func reloadDetectionModel() {
        visionProcessor.reloadModel()
    }

    // MARK: - SCStreamOutput
    nonisolated func stream(_ stream: SCStream,
                            didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
                            of type: SCStreamOutputType) {
        guard type == .screen,
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

        // Detection: only every Nth frame, only when enabled, only when not paused.
        if !paused, detectionEnabledStorage.get(), frameIdx % detectionInterval == 0 {
            let detected = visionProcessor.detect(in: pixelBuffer)
            detectionCache.store(detected)
        }

        // Build the active set of regions for this frame.
        let userRegions = paused ? [] : regionStore.current
        let userBoxes = userRegions.map { region -> AdBoundingBox in
            AdBoundingBox(rect: region.cvRect(inPixelBufferSize: bufferSize),
                          confidence: 1.0,
                          label: "User Selected",
                          source: .user,
                          regionID: region.id)
        }
        let detectedBoxes = (!paused && detectionEnabledStorage.get())
            ? detectionCache.load()
            : []
        let allBoxes = userBoxes + detectedBoxes

        // Inpaint every frame regardless — boxes may have moved or disappeared.
        let patches = inpaintingEngine.inpaintPatches(frame: pixelBuffer, regions: allBoxes)

        // Per-region blocking-event count. Increment once per active user region.
        var perRegion: [UUID: Int] = [:]
        for box in userBoxes {
            if let id = box.regionID { perRegion[id, default: 0] += 1 }
        }

        // Throttle SwiftUI updates to ≤30Hz.
        guard lastEmitClock.shouldEmit(minInterval: minEmitInterval) else { return }

        let labels = detectedBoxes.map { "\($0.label) (\(Int($0.confidence * 100))%)" }
        let patchCount = patches.count
        Task { @MainActor [weak self] in
            guard let self else { return }
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
            self?.isRunning = false
            self?.currentPatches = []
        }
    }

    // MARK: - Screenshot for labeling

    /// Save the most recent captured frame as a PNG. Awaits the next stream
    /// callback to obtain a CGImage that's detached from the SCStream buffer
    /// pool, then PNG-encodes on a background queue.
    nonisolated func saveLatestFrameForLabeling() async -> URL? {
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

final class DetectionCache: @unchecked Sendable {
    private let lock = NSLock()
    private var boxes: [AdBoundingBox] = []
    func store(_ newBoxes: [AdBoundingBox]) {
        lock.lock(); boxes = newBoxes; lock.unlock()
    }
    func load() -> [AdBoundingBox] {
        lock.lock(); defer { lock.unlock() }
        return boxes
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
    private var pendingContinuations: [CheckedContinuation<CGImage?, Never>] = []

    func requestSnapshot() async -> CGImage? {
        await withCheckedContinuation { cont in
            lock.lock()
            pendingContinuations.append(cont)
            lock.unlock()
        }
    }

    /// Called from the stream callback on the videoQueue. Renders only if
    /// at least one snapshot has been requested.
    func ingest(_ pixelBuffer: CVPixelBuffer, context: CIContext) {
        let conts: [CheckedContinuation<CGImage?, Never>] = {
            lock.lock(); defer { lock.unlock() }
            let c = pendingContinuations
            pendingContinuations.removeAll()
            return c
        }()
        guard !conts.isEmpty else { return }

        let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
        let cgImage = context.createCGImage(ciImage, from: ciImage.extent)
        for c in conts { c.resume(returning: cgImage) }
    }
}
