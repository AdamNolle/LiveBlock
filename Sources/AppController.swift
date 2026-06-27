import AppKit
import Combine
import SwiftUI

/// Central observable state for the whole app.
///
/// Owns the capture manager, region store, and weak references to the three
/// runtime windows (Control Panel, Region Editor, Render Layer). Every action
/// the user can take — start/stop capture, open/close editor, quit — flows
/// through here, so the menu bar, hotkeys, and Control Panel stay in sync.
@MainActor
final class AppController: ObservableObject {
    let regionStore: RegionStore
    let captureManager: ScreenCaptureManager
    let labelingController: LabelingController
    let perAppRules: PerAppRulesStore

    // Windows are created by AppDelegate after launch and owned here so they
    // survive the launch scope. Previously these were `weak` and
    // RegionEditorWindow/RenderLayerWindow had no `isReleasedWhenClosed = false`,
    // causing the editor (and every entry point that opened it) to silently
    // no-op. Strong refs + isReleasedWhenClosed=false is the standard AppKit
    // pattern for app-lifetime singletons.
    var controlPanel: ControlPanelWindow?
    var regionEditor: RegionEditorWindow?
    var renderLayer: RenderLayerWindow?
    var labelingWindow: LabelingWindow?
    var trainingDashboardWindow: TrainingDashboardWindow?
    var miniHUDWindow: MiniHUDWindow?
    var onboardingWindow: OnboardingWindow?
    let trainingController: TrainingController

    @Published private(set) var isRunning: Bool = false
    @Published private(set) var detectionEnabled: Bool = true
    @Published private(set) var regionCount: Int = 0
    @Published private(set) var isEditorOpen: Bool = false
    @Published private(set) var screenshotCount: Int = 0
    @Published var autoCaptureEnabled: Bool = false {
        didSet { updateAutoCaptureTimer() }
    }
    @Published var autoCaptureIntervalSeconds: Double = 60.0 {
        didSet { updateAutoCaptureTimer() }
    }
    @Published private(set) var lastCaptureTimestamp: Date? = nil
    /// Frontmost-app metadata, refreshed on `NSWorkspace.didActivateApplicationNotification`.
    /// Drives the `pauseOnFullscreen` toggle and per-app rules pipeline gates.
    @Published private(set) var frontmostBundleID: String? = nil
    @Published private(set) var frontmostName: String? = nil

    /// Live permission state. Re-checked when LiveBlock becomes active
    /// (e.g. user returned from System Settings) so the UI never lies
    /// about whether a grant was issued.
    @Published private(set) var screenRecordingGranted: Bool = Permissions.screenRecordingGranted()
    @Published private(set) var accessibilityGranted: Bool = Permissions.accessibilityGranted()

    /// One source of truth for "what is the app actually doing right now."
    /// Derived from running state, frontmost app, fullscreen, exclusion,
    /// permission, and start error — composed into a single enum the
    /// banner UI reads. Updated reactively, not polled.
    @Published private(set) var pauseReason: PauseReason = .stopped

    @AppStorage("pauseOnFullscreen") private var pauseOnFullscreenStored: Bool = true

    /// One canonical default for the auto-block confidence threshold. The UI
    /// (SettingsView/MLDetectorView) and the capture pipeline both read this so
    /// the persisted value, the slider, and VisionProcessor never disagree.
    /// Previously SettingsView defaulted to 0.84, the slider clamped to
    /// 0.5...0.99, and VisionProcessor's internal default was 0.35 — and nothing
    /// ever pushed the persisted value into VisionProcessor at launch, so the
    /// first session always ran at 0.35 regardless of the saved setting.
    static let defaultMinConfidence: Double = 0.5

    private var cancellables = Set<AnyCancellable>()
    private var autoCaptureTimer: Timer?
    private var workspaceObserver: NSObjectProtocol?
    private var didBecomeActiveObserver: NSObjectProtocol?
    private var screenParamsObserver: NSObjectProtocol?

    init() {
        self.regionStore = RegionStore()
        self.captureManager = ScreenCaptureManager(regionStore: regionStore)
        self.labelingController = LabelingController()
        self.trainingController = TrainingController()
        self.perAppRules = PerAppRulesStore()

        // After training installs a fresh model, drop both VisionProcessor
        // caches so the next inference picks it up — no relaunch needed.
        let cm = self.captureManager
        let lc = self.labelingController
        self.trainingController.onModelInstalled = { [weak cm, weak lc] in
            cm?.reloadDetectionModel()
            lc?.reloadDetectionModel()
        }

        captureManager.$isRunning
            .receive(on: RunLoop.main)
            .assign(to: &$isRunning)

        captureManager.$detectionEnabled
            .receive(on: RunLoop.main)
            .assign(to: &$detectionEnabled)

        regionCount = regionStore.current.count
        screenshotCount = labelingController.totalCount

        labelingController.$totalCount
            .receive(on: RunLoop.main)
            .assign(to: &$screenshotCount)

        // Push the persisted fullscreen-pause preference into the capture
        // manager so the SCStream callback honors it.
        captureManager.pauseOnFullscreen = pauseOnFullscreenStored

        // Confidence desync fix: the persisted "minConfidence" never reached
        // VisionProcessor (which sat at its internal 0.35 default) until the
        // user touched the slider. Read the stored value once at startup —
        // falling back to the single canonical default — and push it into the
        // capture pipeline so the very first detection honors the saved
        // threshold. UserDefaults.double returns 0 when the key is absent, so
        // treat 0 as "unset" and substitute the default.
        let storedConfidence = UserDefaults.standard.object(forKey: "minConfidence") as? Double
        let effectiveConfidence = storedConfidence ?? Self.defaultMinConfidence
        captureManager.setMinimumConfidence(Float(effectiveConfidence))

        // Track which app is frontmost so we can pause for fullscreen video
        // and per-app exclusions without polling on every captured frame.
        workspaceObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            guard let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            else { return }
            Task { @MainActor [weak self] in
                self?.handleFrontmostAppChange(app)
            }
        }
        // Seed once on launch.
        if let app = NSWorkspace.shared.frontmostApplication {
            handleFrontmostAppChange(app)
        }

        // Re-evaluate exclusion when the user toggles a rule.
        perAppRules.$excludedBundleIDs
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                guard let self, let app = NSWorkspace.shared.frontmostApplication
                else { return }
                self.handleFrontmostAppChange(app)
            }
            .store(in: &cancellables)

        // Re-check macOS permissions whenever LiveBlock returns to the
        // foreground. The user has just been to System Settings; the UI
        // must reflect the new grant state immediately, not on next launch.
        didBecomeActiveObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.refreshPermissions()
            }
        }
        // Seed once on construction.
        refreshPermissions()

        // Multi-display fix: when the display layout changes (a monitor is
        // added/removed, resolution changes, or arrangement is reshuffled) the
        // overlay + editor windows must follow the actual capture screen.
        // Previously RenderLayer/RegionEditor were pinned to NSScreen.main at
        // launch and never re-aligned, so on a multi-monitor setup the overlay
        // landed on the wrong display. Re-align here; if capture is running on
        // a screen that vanished, reconfigure onto the new current screen.
        screenParamsObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.handleScreenParametersChanged()
            }
        }

        // Compose pauseReason from every input that affects it. This is
        // the SINGLE source of truth the UI reads. Anything published here
        // becomes the next banner state within one runloop tick.
        Publishers.CombineLatest4(
            captureManager.$isRunning,
            captureManager.$lastStartError,
            captureManager.$frontmostIsFullscreen,
            captureManager.$frontmostIsExcluded
        )
        .combineLatest(
            captureManager.$pauseOnFullscreen,
            $screenRecordingGranted,
            $frontmostName
        )
        .receive(on: RunLoop.main)
        .map { tuple, pauseOnFullscreen, screenGranted, frontmostName -> PauseReason in
            let (running, startErr, frontFullscreen, frontExcluded) = tuple
            if !screenGranted { return .permissionDenied }
            if let err = startErr, !err.isEmpty { return .startError(err) }
            if !running { return .stopped }
            if frontExcluded { return .excludedApp(frontmostName ?? "this app") }
            if pauseOnFullscreen && frontFullscreen {
                return .fullscreenApp(frontmostName ?? "this app")
            }
            return .none
        }
        .removeDuplicates()
        .assign(to: &$pauseReason)
    }

    deinit {
        if let observer = didBecomeActiveObserver {
            NotificationCenter.default.removeObserver(observer)
        }
        if let observer = screenParamsObserver {
            NotificationCenter.default.removeObserver(observer)
        }
        if let observer = workspaceObserver {
            NSWorkspace.shared.notificationCenter.removeObserver(observer)
        }
    }

    private func refreshPermissions() {
        let sr = Permissions.screenRecordingGranted()
        let ax = Permissions.accessibilityGranted()
        if sr != screenRecordingGranted { screenRecordingGranted = sr }
        if ax != accessibilityGranted { accessibilityGranted = ax }
    }

    private func handleFrontmostAppChange(_ app: NSRunningApplication) {
        frontmostBundleID = app.bundleIdentifier
        frontmostName = app.localizedName
        captureManager.frontmostIsExcluded = perAppRules.isExcluded(app.bundleIdentifier ?? "")
        captureManager.frontmostIsFullscreen = Self.isFrontmostAppFullscreen(app)
    }

    /// Heuristic: walk Quartz Window Services for windows owned by `app`'s
    /// PID. If any of them spans the full screen frame and is on the active
    /// space, treat the app as fullscreen. Cheap and doesn't require
    /// extra entitlements.
    private static func isFrontmostAppFullscreen(_ app: NSRunningApplication) -> Bool {
        guard let mainScreen = NSScreen.main else { return false }
        let screenSize = mainScreen.frame.size
        let info = CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly, .excludeDesktopElements],
            kCGNullWindowID
        ) as? [[String: Any]] ?? []
        for window in info {
            guard let pid = window[kCGWindowOwnerPID as String] as? pid_t,
                  pid == app.processIdentifier,
                  let bounds = window[kCGWindowBounds as String] as? [String: CGFloat],
                  let w = bounds["Width"], let h = bounds["Height"]
            else { continue }
            // ±2 px slop for menu-bar inset and scaling rounding.
            if abs(w - screenSize.width) <= 2 && abs(h - screenSize.height) <= 2 {
                return true
            }
        }
        return false
    }

    func updatePauseOnFullscreen(_ on: Bool) {
        pauseOnFullscreenStored = on
        captureManager.pauseOnFullscreen = on
    }

    // MARK: - Actions

    func toggleCapture() {
        if isRunning {
            Task { await captureManager.stop() }
        } else {
            guard let screen = currentScreen() else { return }
            // Align the overlay + editor to the screen we're about to capture
            // BEFORE the first frame lands, so patches composite over the right
            // display on multi-monitor setups.
            alignOverlayWindows(to: screen)
            Task { await captureManager.start(on: screen) }
        }
    }

    func setDetectionEnabled(_ enabled: Bool) {
        captureManager.detectionEnabled = enabled
    }

    func openEditor() {
        guard let panel = regionEditor else {
            NSLog("AppController.openEditor: regionEditor is nil — window was never created or got deallocated.")
            assertionFailure("regionEditor missing")
            return
        }
        panel.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        isEditorOpen = true
    }

    func closeEditor() {
        regionEditor?.orderOut(nil)
        isEditorOpen = false
        refreshRegionCount()
    }

    func toggleEditor() {
        if isEditorOpen { closeEditor() } else { openEditor() }
    }

    func showControlPanel() {
        controlPanel?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func hideControlPanel() {
        controlPanel?.orderOut(nil)
    }

    /// Show a confirm dialog before deleting all regions. Surfaced from
    /// every "Clear regions" entry point. Skips the dialog if there's
    /// nothing to delete.
    func clearRegionsWithConfirm() {
        let count = regionCount
        guard count > 0 else { return }
        let alert = NSAlert()
        alert.messageText = "Delete all \(count) region\(count == 1 ? "" : "s")?"
        alert.informativeText = "This can't be undone. Regions you've drawn (and their per-region toggles) will be removed."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Delete \(count)")
        alert.addButton(withTitle: "Cancel")
        // Make the destructive option the default-Enter target.
        if let destructive = alert.buttons.first {
            destructive.hasDestructiveAction = true
        }
        let response = alert.runModal()
        if response == .alertFirstButtonReturn {
            regionStore.clear()
            refreshRegionCount()
        }
    }

    func clearRegions() {
        regionStore.clear()
        refreshRegionCount()
    }

    func deleteRegion(id: UUID) {
        regionStore.remove(id: id)
        refreshRegionCount()
    }

    func addRegion(_ region: NormalizedRegion) {
        regionStore.add(region)
        refreshRegionCount()
    }

    func refreshRegionCount() {
        regionCount = regionStore.current.count
    }

    // MARK: - Region enabled-state (per-region kill switch)
    //
    // The region store keeps every drawn region; this side-channel toggles
    // whether each one feeds the inpaint pipeline. UserDefaults-backed so
    // the state survives relaunch.

    private static let disabledIdsKey = "disabledRegionIds"

    func regionEnabled(id: UUID) -> Bool {
        let disabled = UserDefaults.standard.stringArray(forKey: Self.disabledIdsKey) ?? []
        return !disabled.contains(id.uuidString)
    }

    func setRegionEnabled(id: UUID, on: Bool) {
        var disabled = Set(UserDefaults.standard.stringArray(forKey: Self.disabledIdsKey) ?? [])
        if on { disabled.remove(id.uuidString) } else { disabled.insert(id.uuidString) }
        UserDefaults.standard.set(Array(disabled), forKey: Self.disabledIdsKey)
        // Trigger UI refresh
        objectWillChange.send()
    }

    func quit() {
        Task { @MainActor in
            stopAutoCapture()
            if isRunning { await captureManager.stop() }
            NSApp.terminate(nil)
        }
    }

    /// Stop capture, hide the render-layer overlay, close the editor if
    /// open, and surface the control panel. Wired to ⌘⇧⌥. and the visible
    /// Panic button. The user wants a single instant action that gets
    /// LiveBlock out of their way without quitting.
    func panicDisable() {
        Task { @MainActor in
            stopAutoCapture()
            if isRunning { await captureManager.stop() }
            renderLayer?.orderOut(nil)
            if isEditorOpen { closeEditor() }
            showControlPanel()
        }
    }

    // MARK: - Labeling pipeline

    /// Capture the most recent frame as a PNG into the labeling queue.
    /// If capture is off we start it briefly, save a frame, then leave it
    /// running — visual users shouldn't have to first toggle capture before
    /// being able to grab a screenshot.
    func captureScreenshotForLabeling() {
        Task { @MainActor in
            if !isRunning {
                guard let screen = currentScreen() else {
                    NSLog("AppController: no screen to start capture on.")
                    return
                }
                alignOverlayWindows(to: screen)
                await captureManager.start(on: screen)
                // Give SCStream one frame to land before we ask for it.
                try? await Task.sleep(nanoseconds: 200_000_000)
            }
            let url = await captureManager.saveLatestFrameForLabeling()
            if url != nil {
                self.lastCaptureTimestamp = Date()
                self.labelingController.refresh()
                NSSound(named: NSSound.Name("Tink"))?.play()
            } else {
                NSLog("AppController: capture screenshot returned nil.")
            }
        }
    }

    func showLabelingWindow() {
        labelingController.refresh()
        guard let win = labelingWindow else {
            NSLog("AppController.showLabelingWindow: labelingWindow is nil.")
            assertionFailure("labelingWindow missing")
            return
        }
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func showTrainingDashboard() {
        guard let win = trainingDashboardWindow else {
            NSLog("AppController.showTrainingDashboard: trainingDashboardWindow is nil.")
            assertionFailure("trainingDashboardWindow missing")
            return
        }
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func showMiniHUD() {
        miniHUDWindow?.orderFrontRegardless()
    }
    func hideMiniHUD() {
        miniHUDWindow?.orderOut(nil)
    }
    func showOnboarding() {
        guard let win = onboardingWindow else {
            NSLog("AppController.showOnboarding: onboardingWindow is nil.")
            return
        }
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// Re-show onboarding even if the user finished it before. Surfaced
    /// from the menu bar so users can re-watch the tour.
    func restartOnboarding() {
        UserDefaults.standard.set(false, forKey: "didOnboard")
        showOnboarding()
    }

    private func stopAutoCapture() {
        autoCaptureTimer?.invalidate()
        autoCaptureTimer = nil
    }

    private func updateAutoCaptureTimer() {
        stopAutoCapture()
        guard autoCaptureEnabled else { return }
        let interval = max(5.0, autoCaptureIntervalSeconds)
        autoCaptureTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.captureScreenshotForLabeling()
            }
        }
    }

    // MARK: - Multi-display overlay alignment

    /// Stable display identifier for an NSScreen. After a display reconfigure
    /// AppKit can hand back fresh NSScreen *instances* for the same physical
    /// display, so pointer-identity comparison is unreliable — compare on the
    /// CoreGraphics display ID instead (the same key `ScreenCaptureManager`
    /// uses to pick the SCDisplay).
    static func displayID(of screen: NSScreen) -> CGDirectDisplayID? {
        (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
    }

    /// Look up the live NSScreen for a display ID, if it's still attached.
    private func liveScreen(forDisplayID id: CGDirectDisplayID) -> NSScreen? {
        NSScreen.screens.first { Self.displayID(of: $0) == id }
    }

    /// The screen the overlay + editor should currently track. When capture is
    /// running we follow the screen we started capture on (so the overlay sits
    /// on the captured display even if the user moves the control panel to
    /// another monitor). Otherwise we fall back to wherever the control panel
    /// lives, then to the main screen.
    func captureScreen() -> NSScreen? {
        if let target = captureManager.targetScreen,
           let id = Self.displayID(of: target),
           let live = liveScreen(forDisplayID: id) {
            return live
        }
        return currentScreen()
    }

    /// Point the render-layer overlay and the region editor at `screen`. Wired
    /// on every capture start and whenever the display layout changes, replacing
    /// the dead `align(to:)` calls that only ever ran once at launch against
    /// NSScreen.main.
    func alignOverlayWindows(to screen: NSScreen) {
        renderLayer?.align(to: screen)
        regionEditor?.align(to: screen)
    }

    /// React to `NSApplication.didChangeScreenParametersNotification`.
    /// Re-align the overlay + editor onto the current capture screen. If we
    /// were capturing a display that no longer exists, restart capture on the
    /// new current screen so blocking keeps working after a monitor unplug.
    private func handleScreenParametersChanged() {
        // Did the display we were capturing disappear? Compare on display ID,
        // not NSScreen identity, since AppKit replaces NSScreen instances on
        // reconfigure even when the physical display persists.
        if isRunning,
           let target = captureManager.targetScreen,
           let id = Self.displayID(of: target),
           liveScreen(forDisplayID: id) == nil {
            NSLog("AppController: capture screen vanished — reconfiguring on current screen.")
            Task { @MainActor in
                await captureManager.stop()
                if let screen = currentScreen() {
                    alignOverlayWindows(to: screen)
                    await captureManager.start(on: screen)
                }
            }
            return
        }
        // Otherwise the captured display is still around (possibly at a new
        // size/origin) — just re-align the overlay + editor onto it.
        if let screen = captureScreen() {
            alignOverlayWindows(to: screen)
        }
    }

    // MARK: - Helpers

    func currentScreen() -> NSScreen? {
        if let panel = controlPanel, let screen = panel.screen { return screen }
        return NSScreen.main ?? NSScreen.screens.first
    }
}
