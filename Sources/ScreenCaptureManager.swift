import Foundation
import Darwin
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
struct DetectionPreview: Identifiable, Sendable {
    let id = UUID()
    let normalizedRect: CGRect
    let label: String
    let confidence: Float
}

enum SystemSuspensionReason: Hashable {
    case systemSleep
    case displaysAsleep
    case sessionLocked
}

struct CaptureRetryPolicy {
    static let delays: [TimeInterval] = [0.5, 1, 2, 4]

    static func delay(forAttempt attempt: Int) -> TimeInterval? {
        guard delays.indices.contains(attempt) else { return nil }
        return delays[attempt]
    }
}

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
    /// User intent is separate from concrete stream state so system suspension
    /// and bounded recovery never turn into an accidental permanent stop.
    @Published private(set) var captureDesired = false
    @Published private(set) var systemSuspensionReasons: Set<SystemSuspensionReason> = []
    @Published private(set) var currentPatches: [InpaintPatch] = []
    @Published private(set) var currentDetections: [DetectionPreview] = []
    @Published private(set) var lastDetectionLabels: [String] = []
    @Published private(set) var framesPerSecond: Double = 0
    @Published private(set) var renderMilliseconds: Double = 0
    @Published private(set) var droppedRenderFrames: Int = 0
    @Published private(set) var reusedStaticFrames: Int = 0
    @Published private(set) var patchesProduced: Int = 0
    @Published private(set) var blockedEvents: Int = 0
    /// Per-region blocking-event counter, keyed by region UUID.
    @Published private(set) var blocksByRegion: [UUID: Int] = [:]
    /// Last error from `start()`, if any. Cleared on successful start or
    /// stop. The Control Panel banner reads this so the user sees WHY
    /// pressing Start did nothing instead of a silent toggle flip.
    @Published private(set) var lastStartError: String? = nil

    /// Detection on by default so a fresh user sees something happen — they
    /// can flip it off in Settings if they only want manual regions.
    @Published var detectionEnabled = true {
        didSet {
            detectionEnabledStorage.set(detectionEnabled)
            if !detectionEnabled { detectionCache.clear() }
        }
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
    private var targetDisplayID: CGDirectDisplayID?
    private var lifecycleGeneration: UInt64 = 0
    private var isStarting = false
    private var recoveryAttempt = 0
    private var recoveryTask: Task<Void, Never>?

    nonisolated private let visionProcessor = VisionProcessor()
    nonisolated private let inpaintingEngine = InpaintingEngine()
    nonisolated private let activeStreamIdentity = AtomicObjectIdentity()

    nonisolated private let videoQueue = DispatchQueue(label: "com.liveblock.videoQueue", qos: .userInteractive)
    nonisolated private let detectionQueue = DispatchQueue(label: "com.liveblock.detectionQueue", qos: .userInitiated)
    nonisolated private let renderQueue = DispatchQueue(label: "com.liveblock.renderQueue", qos: .userInteractive)

    // Atomically swappable booleans (read by nonisolated frame callback)
    nonisolated private let detectionEnabledStorage = AtomicBool(true)
    nonisolated private let detectionInFlight = AtomicBool(false)
    nonisolated private let renderInFlight = AtomicBool(false)
    nonisolated private let pauseOnFullscreenStorage = AtomicBool(true)
    nonisolated private let frontmostFullscreenStorage = AtomicBool(false)
    nonisolated private let frontmostExcludedStorage = AtomicBool(false)
    nonisolated private let disabledRegionIDsStorage = AtomicUUIDSet()
    nonisolated private let inpaintFillStyleStorage = AtomicInt(InpaintFillStyle.smart.rawValue)
    nonisolated private let fpsCounter = FPSCounter()
    nonisolated private let renderStateTracker = RenderStateTracker()
    nonisolated private let reusedStaticFrameCounter = AtomicCounter()

    // Frame counter — drives the "detect every Nth frame" cadence.
    nonisolated private let frameCounter = AtomicCounter()
    nonisolated private let detectionInterval = 4 // 4 → ~15Hz at 60fps capture

    // Detection results cached between detection runs.
    nonisolated private let detectionCache = DetectionCache()
    nonisolated private let blockEventTracker = BlockEventTracker()

    // Throttle main-actor patch updates.
    nonisolated private let lastEmitClock = AtomicTime()
    nonisolated private let minEmitInterval: TimeInterval = 1.0 / 30.0

    // Latest pixel buffer, retained for hotkey-triggered screenshot saves.
    nonisolated private let latestBufferStorage = LatestBufferStorage()

    // Shared CIContext for PNG encoding (heavy to construct).
    nonisolated private let pngContext = CIContext(options: [.useSoftwareRenderer: false])

    private var lifecycleObservers: [NSObjectProtocol] = []

    init(regionStore: RegionStore) {
        self.regionStore = regionStore
        super.init()
        installSleepObservers()
    }

    deinit {
        recoveryTask?.cancel()
        for obs in lifecycleObservers {
            NSWorkspace.shared.notificationCenter.removeObserver(obs)
        }
    }

    /// Pause capture for sleep, display sleep, and session lock. Multiple
    /// reasons can overlap; capture resumes once all clear and only when the
    /// user still wants it running.
    private func installSleepObservers() {
        let nc = NSWorkspace.shared.notificationCenter
        let pairs: [(Notification.Name, Notification.Name, SystemSuspensionReason)] = [
            (NSWorkspace.willSleepNotification, NSWorkspace.didWakeNotification, .systemSleep),
            (NSWorkspace.screensDidSleepNotification, NSWorkspace.screensDidWakeNotification, .displaysAsleep),
            (NSWorkspace.sessionDidResignActiveNotification, NSWorkspace.sessionDidBecomeActiveNotification, .sessionLocked)
        ]
        for (pauseName, resumeName, reason) in pairs {
            lifecycleObservers.append(nc.addObserver(forName: pauseName, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor [weak self] in await self?.suspend(for: reason) }
            })
            lifecycleObservers.append(nc.addObserver(forName: resumeName, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor [weak self] in await self?.resume(from: reason) }
            })
        }
    }

    private func suspend(for reason: SystemSuspensionReason) async {
        systemSuspensionReasons.insert(reason)
        recoveryTask?.cancel()
        guard isRunning || isStarting else { return }
        NSLog("ScreenCaptureManager: suspending capture for \(reason).")
        await stop(preserveIntent: true)
    }

    private func resume(from reason: SystemSuspensionReason) async {
        systemSuspensionReasons.remove(reason)
        guard systemSuspensionReasons.isEmpty, captureDesired else { return }
        guard let screen = resolvedTargetScreen() else {
            lastStartError = "The selected display is no longer available."
            return
        }
        NSLog("ScreenCaptureManager: resuming capture after \(reason).")
        await startCapture(on: screen)
    }

    // MARK: - Lifecycle

    func start(on screen: NSScreen) async {
        captureDesired = true
        targetDisplayID = screen.liveBlockDisplayID
        recoveryTask?.cancel()
        recoveryAttempt = 0
        await startCapture(on: screen)
    }

    private func startCapture(on screen: NSScreen) async {
        guard captureDesired, systemSuspensionReasons.isEmpty,
              !isRunning, !isStarting, stream == nil else { return }
        guard let screenID = screen.liveBlockDisplayID,
              targetDisplayID == nil || targetDisplayID == screenID else {
            captureDesired = false
            lastStartError = "The selected display no longer matches the capture target."
            return
        }
        targetDisplayID = screenID
        lifecycleGeneration &+= 1
        let generation = lifecycleGeneration
        isStarting = true
        defer {
            if lifecycleGeneration == generation { isStarting = false }
        }
        // Pre-flight: if Screen Recording is denied, fail fast with a clear
        // message instead of a generic SCStream error.
        guard Permissions.screenRecordingGranted() else {
            self.captureDesired = false
            self.lastStartError = "Screen Recording permission needed. Open System Settings → Privacy & Security → Screen Recording, enable LiveBlock, and try again."
            NSLog("ScreenCaptureManager: refusing to start — Screen Recording denied.")
            return
        }
        do {
            let availableContent = try await SCShareableContent.excludingDesktopWindows(false,
                                                                                        onScreenWindowsOnly: true)
            guard lifecycleGeneration == generation else { return }
            guard let display = pickDisplay(for: screen, in: availableContent.displays) else {
                self.captureDesired = false
                self.lastStartError = "No display matched the target screen."
                NSLog("ScreenCaptureManager: no display matched target screen.")
                return
            }

            // Use authoritative CG display pixels. Scaled/mirrored modes can
            // diverge from pointFrame × backingScaleFactor through rounding.
            let pixelWidth = CGDisplayPixelsWide(display.displayID)
            let pixelHeight = CGDisplayPixelsHigh(display.displayID)
            guard pixelWidth > 0, pixelHeight > 0 else {
                self.captureDesired = false
                self.lastStartError = "The selected display reported invalid pixel dimensions."
                return
            }

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
            activeStreamIdentity.set(newStream, generation: generation)
            try await newStream.startCapture()
            guard lifecycleGeneration == generation else {
                activeStreamIdentity.clear(ifMatching: newStream)
                try? await newStream.stopCapture()
                return
            }

            self.stream = newStream
            self.isRunning = true
            self.recoveryAttempt = 0
            self.lastStartError = nil
            NSLog("ScreenCaptureManager: capture started (\(pixelWidth)x\(pixelHeight)).")
        } catch {
            activeStreamIdentity.clear(ifGeneration: generation)
            guard lifecycleGeneration == generation else { return }
            self.lastStartError = error.localizedDescription
            NSLog("ScreenCaptureManager: failed to start capture: \(error.localizedDescription)")
            if captureDesired, Permissions.screenRecordingGranted(), resolvedTargetScreen() != nil {
                scheduleRecovery()
            } else {
                captureDesired = false
            }
        }
    }

    func stop(preserveIntent: Bool = false) async {
        if !preserveIntent {
            captureDesired = false
            recoveryAttempt = 0
        }
        recoveryTask?.cancel()
        lifecycleGeneration &+= 1
        isStarting = false
        let streamToStop = stream
        stream = nil
        activeStreamIdentity.clear()
        resetStreamState(clearError: true)
        guard let streamToStop else { return }
        do {
            try await streamToStop.stopCapture()
        } catch {
            NSLog("ScreenCaptureManager: stop failed: \(error.localizedDescription)")
        }
    }

    /// Fail closed when a topology transaction resolves to no usable display.
    /// Visible frame state is cleared immediately, while the user's intent and
    /// target identity remain available for a later display-change recovery.
    func failClosedForUnavailableDisplay() async -> Bool {
        await stop(preserveIntent: true)
        // Panic/quit/user-stop or a newer topology restart can win while
        // stopCapture is suspended. Never overwrite that newer state or let an
        // obsolete task hide its newly restored render surface.
        guard !Task.isCancelled, captureDesired, !isRunning, !isStarting, stream == nil else {
            return false
        }
        lastStartError = "No usable display is currently available. Capture will remain stopped until a display returns."
        return true
    }

    func setMinimumConfidence(_ value: Float) {
        visionProcessor.updateMinimumConfidence(value)
    }

    func detectorRules() -> [DetectorClassRule] {
        visionProcessor.detectorRules()
    }

    @discardableResult
    func setDetectorClassEnabled(id: UInt32, enabled: Bool) -> Bool {
        visionProcessor.setDetectorClassEnabled(id: id, enabled: enabled)
    }

    func setDisabledRegionIDs(_ ids: Set<UUID>) {
        disabledRegionIDsStorage.store(ids)
    }

    func setInpaintFillStyle(_ rawValue: Int) {
        inpaintFillStyleStorage.set(InpaintFillStyle(rawValue: rawValue)?.rawValue ?? InpaintFillStyle.smart.rawValue)
    }

    /// Drop the cached CoreML model so the very next detection call reloads
    /// from disk. Used by TrainingController after a successful install so
    /// the running app picks up the freshly trained model without relaunch.
    nonisolated func reloadDetectionModel() {
        detectionCache.clear()
        visionProcessor.reloadModel()
    }

    // MARK: - SCStreamOutput
    nonisolated func stream(_ stream: SCStream,
                            didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
                            of type: SCStreamOutputType) {
        guard type == .screen,
              let streamGeneration = activeStreamIdentity.generation(ifMatching: stream),
              let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }

        // Resolve only requests bound to this exact capture generation. This
        // prevents a request delayed across stop/panic from consuming a later
        // stream's frame.
        latestBufferStorage.ingest(pixelBuffer,
                                   context: pngContext,
                                   generation: streamGeneration)

        let bufferWidth = CGFloat(CVPixelBufferGetWidth(pixelBuffer))
        let bufferHeight = CGFloat(CVPixelBufferGetHeight(pixelBuffer))
        let bufferSize = CGSize(width: bufferWidth, height: bufferHeight)

        let frameIdx = frameCounter.increment()
        let fps = fpsCounter.tick()
        let frameIsIdle = Self.frameStatus(sampleBuffer) == .idle

        // Pause hooks: skip detection + inpaint when the frontmost app is
        // fullscreen (and the user opted in) or when it's on the per-app
        // exclusion list. We still ingest the buffer above so the labeling
        // hotkey keeps working, and we still publish FPS so the UI shows
        // the pipeline is alive.
        let pausedForFullscreen = pauseOnFullscreenStorage.get() && frontmostFullscreenStorage.get()
        let pausedForApp = frontmostExcludedStorage.get()
        let paused = pausedForFullscreen || pausedForApp
        if paused { detectionCache.clear() }

        // Detection: only every Nth frame, only when enabled, only when not paused.
        if !paused,
           !frameIsIdle,
           detectionEnabledStorage.get(),
           frameIdx % detectionInterval == 0,
           !detectionInFlight.get() {
            // Keep CoreML off ScreenCaptureKit's callback queue. Retaining the
            // CVPixelBuffer in this closure safely extends its lifetime; the
            // single-flight gate drops work instead of building inference lag.
            detectionInFlight.set(true)
            let retainedBuffer = SendablePixelBuffer(pixelBuffer)
            let cacheGeneration = detectionCache.generation()
            detectionQueue.async { [visionProcessor, detectionCache, detectionInFlight] in
                let detected = visionProcessor.detect(in: retainedBuffer.value)
                detectionCache.store(detected, ifGeneration: cacheGeneration)
                detectionInFlight.set(false)
            }
        }

        // Build the active set of regions for this frame.
        let disabledRegionIDs = disabledRegionIDsStorage.load()
        let userRegions = paused ? [] : regionStore.current(excluding: disabledRegionIDs)
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
        let style = InpaintFillStyle(rawValue: inpaintFillStyleStorage.get()) ?? .smart

        // ScreenCaptureKit marks duplicate frames as idle. Reuse the existing
        // overlay when both pixels and the region/style configuration are
        // unchanged, avoiding Core Image work on static desktops. A changed
        // region configuration always renders once, even on an idle frame.
        guard renderStateTracker.shouldRender(boxes: allBoxes,
                                              styleRawValue: style.rawValue,
                                              frameIsIdle: frameIsIdle) else {
            _ = reusedStaticFrameCounter.increment()
            return
        }

        // Rendering into CGImages is the expensive operation. Admit at most
        // 30 renders/sec and never queue behind a slow render: stale work is
        // dropped while ScreenCaptureKit's callback stays responsive at 60 Hz.
        guard lastEmitClock.shouldEmit(minInterval: minEmitInterval) else { return }
        guard renderInFlight.trySetTrue() else {
            Task { @MainActor [weak self] in self?.droppedRenderFrames &+= 1 }
            return
        }

        let retainedBuffer = SendablePixelBuffer(pixelBuffer)
        let retainedStream = SendableSCStream(stream)
        renderQueue.async { [inpaintingEngine, renderInFlight, blockEventTracker, activeStreamIdentity, renderStateTracker] in
            let started = CACurrentMediaTime()
            defer { renderInFlight.set(false) }
            guard activeStreamIdentity.matches(retainedStream.value) else { return }

            let patches = inpaintingEngine.inpaintPatches(frame: retainedBuffer.value,
                                                           regions: allBoxes,
                                                           style: style)
            renderStateTracker.markRendered(boxes: allBoxes, styleRawValue: style.rawValue)
            let eventDelta = blockEventTracker.update(userBoxes: userBoxes,
                                                      detectedBoxes: detectedBoxes)
            let detections = detectedBoxes.compactMap { box -> DetectionPreview? in
                let rect = box.rect.intersection(CGRect(origin: .zero, size: bufferSize))
                guard !rect.isNull, rect.width > 0, rect.height > 0 else { return nil }
                return DetectionPreview(
                    normalizedRect: CGRect(x: rect.minX / bufferWidth,
                                           y: (bufferHeight - rect.maxY) / bufferHeight,
                                           width: rect.width / bufferWidth,
                                           height: rect.height / bufferHeight),
                    label: box.label,
                    confidence: box.confidence
                )
            }
            let labels = detections.map { "\($0.label) (\(Int($0.confidence * 100))%)" }
            let patchCount = patches.count
            let renderMS = (CACurrentMediaTime() - started) * 1_000

            Task { @MainActor [weak self] in
                guard let self, self.activeStreamIdentity.matches(retainedStream.value), self.isRunning else { return }
                self.currentPatches = patches
                self.currentDetections = detections
                self.lastDetectionLabels = labels
                self.framesPerSecond = fps
                self.renderMilliseconds = renderMS
                self.reusedStaticFrames = self.reusedStaticFrameCounter.get()
                self.patchesProduced &+= patchCount
                self.blockedEvents &+= eventDelta.total
                for id in eventDelta.newUserRegionIDs {
                    self.blocksByRegion[id, default: 0] += 1
                }
            }
        }
    }

    nonisolated private static func frameStatus(_ sampleBuffer: CMSampleBuffer) -> SCFrameStatus? {
        guard let attachmentArray = CMSampleBufferGetSampleAttachmentsArray(
            sampleBuffer,
            createIfNecessary: false
        ) as? [[SCStreamFrameInfo: Any]],
        let attachments = attachmentArray.first,
        let rawValue = attachments[.status] as? Int else { return nil }
        return SCFrameStatus(rawValue: rawValue)
    }

    nonisolated func stream(_ stream: SCStream, didStopWithError error: Error) {
        NSLog("ScreenCaptureManager: stream stopped with error: \(error.localizedDescription)")
        Task { @MainActor [weak self] in
            guard let self, self.stream === stream else { return }
            self.stream = nil
            self.activeStreamIdentity.clear(ifMatching: stream)
            self.lifecycleGeneration &+= 1
            self.resetStreamState(clearError: false)
            self.lastStartError = "Capture interrupted: \(error.localizedDescription)"
            self.scheduleRecovery()
        }
    }

    private func resetStreamState(clearError: Bool) {
        detectionCache.clear()
        blockEventTracker.reset()
        renderStateTracker.reset()
        fpsCounter.reset()
        latestBufferStorage.cancelPending()
        isRunning = false
        currentPatches = []
        currentDetections = []
        lastDetectionLabels = []
        framesPerSecond = 0
        renderMilliseconds = 0
        if clearError { lastStartError = nil }
    }

    private func resolvedTargetScreen() -> NSScreen? {
        if let targetDisplayID, let screen = NSScreen.liveBlockScreen(id: targetDisplayID) {
            return screen
        }
        let descriptors = NSScreen.liveBlockDescriptors()
        guard let fallbackID = DisplayTargetResolver.resolvedID(preferred: targetDisplayID,
                                                                 descriptors: descriptors) else {
            return nil
        }
        targetDisplayID = fallbackID
        return NSScreen.liveBlockScreen(id: fallbackID)
    }

    private func scheduleRecovery() {
        guard captureDesired, systemSuspensionReasons.isEmpty else { return }
        guard let delay = CaptureRetryPolicy.delay(forAttempt: recoveryAttempt) else {
            captureDesired = false
            return
        }
        recoveryAttempt += 1
        recoveryTask?.cancel()
        recoveryTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            guard !Task.isCancelled, let self, self.captureDesired,
                  self.systemSuspensionReasons.isEmpty,
                  let screen = self.resolvedTargetScreen() else { return }
            await self.startCapture(on: screen)
            if !self.isRunning, self.captureDesired { self.scheduleRecovery() }
        }
    }

    // MARK: - Screenshot for labeling

    /// Save a frame from one exact capture generation as a PNG. A stop,
    /// suspension, panic, topology restart, or replacement stream invalidates
    /// the request before it can consume or retain the next generation's frame.
    func saveLatestFrameForLabeling(expectedGeneration: UInt64) async -> URL? {
        guard lifecycleGeneration == expectedGeneration, isRunning,
              activeStreamIdentity.currentGeneration() == expectedGeneration,
              let cgImage = await latestBufferStorage.requestSnapshot(
                timeout: 2.0,
                generation: expectedGeneration
              ) else { return nil }
        guard lifecycleGeneration == expectedGeneration, isRunning,
              activeStreamIdentity.currentGeneration() == expectedGeneration else { return nil }
        let url = await withCheckedContinuation { (cont: CheckedContinuation<URL?, Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                guard TrainingPaths.ensureDirectories() else {
                    cont.resume(returning: nil); return
                }
                let stem = TrainingPaths.newScreenshotStem()
                let url = TrainingPaths.screenshots.appendingPathComponent("\(stem).png")
                cont.resume(returning: SnapshotPNGWriter.write(cgImage, to: url) ? url : nil)
            }
        }
        guard lifecycleGeneration == expectedGeneration, isRunning,
              activeStreamIdentity.currentGeneration() == expectedGeneration else {
            if let url { try? FileManager.default.removeItem(at: url) }
            return nil
        }
        return url
    }

    var currentLifecycleGeneration: UInt64 { lifecycleGeneration }

    // MARK: - Display selection (S8)
    nonisolated private func pickDisplay(for screen: NSScreen, in displays: [SCDisplay]) -> SCDisplay? {
        guard let targetID = screen.liveBlockDisplayID else { return nil }
        return displays.first(where: { $0.displayID == targetID })
    }
}

// MARK: - Thread-safe hot-path helpers

/// CVPixelBuffer is reference-counted and safe to retain for read-only Vision
/// inference, but CoreVideo has not annotated the legacy type as Sendable.
final class SendablePixelBuffer: @unchecked Sendable {
    let value: CVPixelBuffer
    init(_ value: CVPixelBuffer) { self.value = value }
}

final class SendableSCStream: @unchecked Sendable {
    let value: SCStream
    init(_ value: SCStream) { self.value = value }
}

final class AtomicObjectIdentity: @unchecked Sendable {
    private let lock = NSLock()
    private var identity: ObjectIdentifier?
    private var generation: UInt64 = 0

    func set(_ object: AnyObject, generation: UInt64) {
        lock.lock()
        identity = ObjectIdentifier(object)
        self.generation = generation
        lock.unlock()
    }
    func matches(_ object: AnyObject) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return identity == ObjectIdentifier(object)
    }
    func generation(ifMatching object: AnyObject) -> UInt64? {
        lock.lock(); defer { lock.unlock() }
        return identity == ObjectIdentifier(object) ? generation : nil
    }
    func currentGeneration() -> UInt64? {
        lock.lock(); defer { lock.unlock() }
        return identity == nil ? nil : generation
    }
    func clear() {
        lock.lock(); identity = nil; generation = 0; lock.unlock()
    }
    func clear(ifGeneration expected: UInt64) {
        lock.lock()
        if generation == expected { identity = nil; generation = 0 }
        lock.unlock()
    }
    func clear(ifMatching object: AnyObject) {
        lock.lock()
        if identity == ObjectIdentifier(object) { identity = nil; generation = 0 }
        lock.unlock()
    }
}

final class AtomicBool: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Bool
    init(_ initial: Bool) { self.value = initial }
    func get() -> Bool { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ newValue: Bool) { lock.lock(); value = newValue; lock.unlock() }
    func trySetTrue() -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard !value else { return false }
        value = true
        return true
    }
}

final class AtomicInt: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Int
    init(_ initial: Int) { value = initial }
    func get() -> Int { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ newValue: Int) { lock.lock(); value = newValue; lock.unlock() }
}

final class AtomicCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Int = 0
    func increment() -> Int {
        lock.lock(); defer { lock.unlock() }
        value &+= 1
        return value
    }
    func get() -> Int {
        lock.lock(); defer { lock.unlock() }
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

/// Tracks the region/style configuration represented by the current overlay.
/// Idle frames can reuse it; configuration changes still force one render.
final class RenderStateTracker: @unchecked Sendable {
    private struct RegionKey: Equatable {
        let x: CGFloat
        let y: CGFloat
        let width: CGFloat
        let height: CGFloat
        let confidence: Float
        let label: String
        let source: String
        let regionID: UUID?
    }

    private struct Configuration: Equatable {
        let regions: [RegionKey]
        let styleRawValue: Int
    }

    private let lock = NSLock()
    private var lastRendered: Configuration?

    func shouldRender(boxes: [AdBoundingBox], styleRawValue: Int, frameIsIdle: Bool) -> Bool {
        let configuration = Self.configuration(boxes: boxes, styleRawValue: styleRawValue)
        lock.lock(); defer { lock.unlock() }
        guard configuration == lastRendered else { return true }
        return !frameIsIdle && !boxes.isEmpty
    }

    func markRendered(boxes: [AdBoundingBox], styleRawValue: Int) {
        let configuration = Self.configuration(boxes: boxes, styleRawValue: styleRawValue)
        lock.lock()
        lastRendered = configuration
        lock.unlock()
    }

    func reset() {
        lock.lock()
        lastRendered = nil
        lock.unlock()
    }

    private static func configuration(boxes: [AdBoundingBox], styleRawValue: Int) -> Configuration {
        Configuration(regions: boxes.map {
            RegionKey(x: $0.rect.origin.x,
                      y: $0.rect.origin.y,
                      width: $0.rect.width,
                      height: $0.rect.height,
                      confidence: $0.confidence,
                      label: $0.label,
                      source: $0.source.rawValue,
                      regionID: $0.regionID)
        }, styleRawValue: styleRawValue)
    }
}

final class DetectionCache: @unchecked Sendable {
    private let lock = NSLock()
    private var boxes: [AdBoundingBox] = []
    private var cacheGeneration: UInt64 = 0

    func generation() -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        return cacheGeneration
    }
    func store(_ newBoxes: [AdBoundingBox], ifGeneration expected: UInt64) {
        lock.lock(); defer { lock.unlock() }
        guard cacheGeneration == expected else { return }
        boxes = newBoxes
    }
    func load() -> [AdBoundingBox] {
        lock.lock(); defer { lock.unlock() }
        return boxes
    }
    func clear() {
        lock.lock()
        boxes = []
        cacheGeneration &+= 1
        lock.unlock()
    }
}

struct BlockEventDelta: Sendable {
    let total: Int
    let newUserRegionIDs: Set<UUID>
}

/// Counts appearances, not rendered frames. A static region increments once;
/// it becomes a new event only after disappearing and appearing again.
final class BlockEventTracker: @unchecked Sendable {
    private let lock = NSLock()
    private var activeUserIDs: Set<UUID> = []
    private var activeDetections: [AdBoundingBox] = []

    func update(userBoxes: [AdBoundingBox], detectedBoxes: [AdBoundingBox]) -> BlockEventDelta {
        lock.lock(); defer { lock.unlock() }
        let userIDs = Set(userBoxes.compactMap(\.regionID))
        let newUserIDs = userIDs.subtracting(activeUserIDs)

        let newDetectionCount = detectedBoxes.reduce(into: 0) { count, box in
            let alreadyActive = activeDetections.contains {
                $0.label == box.label && Self.iou($0.rect, box.rect) >= 0.5
            }
            if !alreadyActive { count += 1 }
        }

        activeUserIDs = userIDs
        activeDetections = detectedBoxes
        return BlockEventDelta(total: newUserIDs.count + newDetectionCount,
                               newUserRegionIDs: newUserIDs)
    }

    func reset() {
        lock.lock()
        activeUserIDs = []
        activeDetections = []
        lock.unlock()
    }

    private static func iou(_ a: CGRect, _ b: CGRect) -> CGFloat {
        let intersection = a.intersection(b)
        guard !intersection.isNull, !intersection.isEmpty else { return 0 }
        let intersectionArea = intersection.width * intersection.height
        let unionArea = a.width * a.height + b.width * b.height - intersectionArea
        return unionArea > 0 ? intersectionArea / unionArea : 0
    }
}

final class AtomicUUIDSet: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Set<UUID> = []
    func store(_ newValue: Set<UUID>) {
        lock.lock(); value = newValue; lock.unlock()
    }
    func load() -> Set<UUID> {
        lock.lock(); defer { lock.unlock() }
        return value
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

    func reset() {
        lock.lock()
        samples.removeAll(keepingCapacity: true)
        lock.unlock()
    }
}

enum SnapshotPNGWriter {
    /// Encode before creating the destination, then create-new with private
    /// permissions, flush to stable storage, and remove any partial write.
    static func write(_ image: CGImage, to url: URL) -> Bool {
        let encoded = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            encoded as CFMutableData,
            "public.png" as CFString,
            1,
            nil
        ) else { return false }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            NSLog("ScreenCaptureManager: PNG finalize failed")
            return false
        }

        let descriptor = open(url.path,
                              O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW,
                              S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else {
            NSLog("ScreenCaptureManager: refusing to replace an existing screenshot")
            return false
        }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        do {
            try handle.write(contentsOf: encoded as Data)
            try handle.synchronize()
            try handle.close()
            return true
        } catch {
            try? handle.close()
            try? FileManager.default.removeItem(at: url)
            NSLog("ScreenCaptureManager: PNG write failed: \(error.localizedDescription)")
            return false
        }
    }
}

/// Request-driven snapshot store. Avoids pinning a recycled CVPixelBuffer
/// out of the SCStream queue. When `requestSnapshot()` is called, the next
/// `ingest(...)` from the stream callback materializes a detached CGImage
/// and resolves the continuation. After delivery, no buffer is held.
final class LatestBufferStorage: @unchecked Sendable {
    private struct Request {
        let generation: UInt64
        let continuation: CheckedContinuation<CGImage?, Never>
    }

    private let lock = NSLock()
    private var pendingRequests: [UUID: Request] = [:]

    func requestSnapshot(timeout: TimeInterval, generation: UInt64) async -> CGImage? {
        let requestID = UUID()
        return await withCheckedContinuation { continuation in
            lock.lock()
            pendingRequests[requestID] = Request(generation: generation,
                                                 continuation: continuation)
            lock.unlock()

            DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + max(0.1, timeout)) { [weak self] in
                guard let self else { return }
                let timedOut: CheckedContinuation<CGImage?, Never>? = {
                    self.lock.lock(); defer { self.lock.unlock() }
                    return self.pendingRequests.removeValue(forKey: requestID)?.continuation
                }()
                timedOut?.resume(returning: nil)
            }
        }
    }

    func pendingCount() -> Int {
        lock.lock(); defer { lock.unlock() }
        return pendingRequests.count
    }

    /// Resolve every outstanding labeling request without a frame. Stop,
    /// panic, suspension, and stream failure call this so an old request can
    /// never consume a frame from a later capture generation.
    @discardableResult
    func cancelPending() -> Int {
        let continuations: [CheckedContinuation<CGImage?, Never>] = {
            lock.lock(); defer { lock.unlock() }
            let values = pendingRequests.values.map(\.continuation)
            pendingRequests.removeAll()
            return values
        }()
        for continuation in continuations {
            continuation.resume(returning: nil)
        }
        return continuations.count
    }

    /// Called from the stream callback on the videoQueue. Renders only if
    /// at least one snapshot has been requested.
    func ingest(_ pixelBuffer: CVPixelBuffer, context: CIContext, generation: UInt64) {
        let conts: [CheckedContinuation<CGImage?, Never>] = {
            lock.lock(); defer { lock.unlock() }
            let matching = pendingRequests.compactMap { id, request in
                request.generation == generation ? id : nil
            }
            return matching.compactMap { pendingRequests.removeValue(forKey: $0)?.continuation }
        }()
        guard !conts.isEmpty else { return }

        let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
        let cgImage = context.createCGImage(ciImage, from: ciImage.extent)
        for c in conts { c.resume(returning: cgImage) }
    }
}
