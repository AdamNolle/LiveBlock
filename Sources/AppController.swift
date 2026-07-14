import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

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
    @Published private(set) var modelUpdateInProgress: Bool = false
    @Published private(set) var modelUpdateStatus: String? = nil
    /// Frontmost-app metadata, refreshed on `NSWorkspace.didActivateApplicationNotification`.
    /// Drives the `pauseOnFullscreen` toggle and per-app rules pipeline gates.
    @Published private(set) var frontmostBundleID: String? = nil
    @Published private(set) var frontmostName: String? = nil
    /// Explicit capture target. A CG display ID survives window movement and
    /// is re-resolved against fresh NSScreen snapshots after topology changes.
    @Published private(set) var availableDisplays: [DisplayDescriptor] = []
    @Published private(set) var selectedDisplayID: CGDirectDisplayID? = nil

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

    private var cancellables = Set<AnyCancellable>()
    private var autoCaptureTimer: Timer?
    private var fullscreenMonitorTimer: Timer?
    private var screenRestartTask: Task<Void, Never>?
    private var screenRestartWasRunning = false
    private var workspaceObserver: NSObjectProtocol?
    private var didBecomeActiveObserver: NSObjectProtocol?
    private static let selectedDisplayIDKey = "selectedDisplayID"

    init() {
        if !DesktopContracts.validateMacOS() {
            NSLog("AppController: shared desktop behavior/capability contract validation failed")
        }
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

        captureManager.$isRunning
            .receive(on: RunLoop.main)
            .sink { [weak self] running in self?.updateFullscreenMonitor(running: running) }
            .store(in: &cancellables)

        refreshDisplayTargets()
        regionCount = regionStore.current.count
        screenshotCount = labelingController.totalCount

        labelingController.$totalCount
            .receive(on: RunLoop.main)
            .assign(to: &$screenshotCount)

        // Push the persisted fullscreen-pause preference into the capture
        // manager so the SCStream callback honors it.
        captureManager.pauseOnFullscreen = pauseOnFullscreenStored
        let storedConfidence = UserDefaults.standard.object(forKey: "minConfidence") as? Double ?? 0.25
        captureManager.setMinimumConfidence(Float(storedConfidence))
        captureManager.setInpaintFillStyle(UserDefaults.standard.integer(forKey: "inpaintFillStyle"))
        captureManager.setDisabledRegionIDs(Self.disabledRegionIDs())

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
        fullscreenMonitorTimer?.invalidate()
        screenRestartTask?.cancel()
        if let observer = didBecomeActiveObserver {
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

    /// Save a privacy-minimized support snapshot. The report contains no
    /// pixels, regions, process names, window titles, user paths, or labels.
    func exportDiagnostics() {
        refreshPermissions()
        let panel = NSSavePanel()
        panel.title = "Export LiveBlock Diagnostics"
        panel.nameFieldStringValue = "liveblock-diagnostics.json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try DiagnosticsReport.capture(from: self).write(to: url)
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } catch {
            let alert = NSAlert()
            alert.alertStyle = .critical
            alert.messageText = "Could not export diagnostics"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    private func handleFrontmostAppChange(_ app: NSRunningApplication) {
        frontmostBundleID = app.bundleIdentifier
        frontmostName = app.localizedName
        captureManager.frontmostIsExcluded = perAppRules.isExcluded(app.bundleIdentifier ?? "")
        captureManager.frontmostIsFullscreen = Self.isFrontmostAppFullscreen(app)
    }

    private func updateFullscreenMonitor(running: Bool) {
        fullscreenMonitorTimer?.invalidate()
        fullscreenMonitorTimer = nil
        guard running else { return }
        fullscreenMonitorTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            guard let app = NSWorkspace.shared.frontmostApplication else { return }
            Task { @MainActor [weak self] in self?.handleFrontmostAppChange(app) }
        }
    }

    /// Heuristic: walk Quartz Window Services for windows owned by `app`'s
    /// PID. If any of them spans the full screen frame and is on the active
    /// space, treat the app as fullscreen. Cheap and doesn't require
    /// extra entitlements.
    private static func isFrontmostAppFullscreen(_ app: NSRunningApplication) -> Bool {
        let screenSizes = NSScreen.screens.map(\.frame.size)
        guard !screenSizes.isEmpty else { return false }
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
            if screenSizes.contains(where: { abs(w - $0.width) <= 2 && abs(h - $0.height) <= 2 }) {
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
        if isRunning || captureManager.captureDesired {
            screenRestartWasRunning = false
            screenRestartTask?.cancel()
            Task { await captureManager.stop() }
        } else {
            guard let screen = currentScreen() else { return }
            alignOverlays(to: screen)
            Task { await captureManager.start(on: screen) }
        }
    }

    func selectDisplay(id: CGDirectDisplayID) {
        guard availableDisplays.contains(where: { $0.id == id }) else { return }
        let changed = selectedDisplayID != id
        selectedDisplayID = id
        UserDefaults.standard.set(Int(id), forKey: Self.selectedDisplayIDKey)
        guard let screen = NSScreen.liveBlockScreen(id: id) else { return }
        alignOverlays(to: screen)
        guard changed, captureManager.captureDesired else { return }
        screenRestartTask?.cancel()
        screenRestartTask = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.captureManager.stop(preserveIntent: true)
            guard !Task.isCancelled else { return }
            await self.captureManager.start(on: screen)
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
            UserDefaults.standard.removeObject(forKey: Self.disabledIdsKey)
            captureManager.setDisabledRegionIDs([])
            refreshRegionCount()
        }
    }

    func clearRegions() {
        regionStore.clear()
        UserDefaults.standard.removeObject(forKey: Self.disabledIdsKey)
        captureManager.setDisabledRegionIDs([])
        refreshRegionCount()
    }

    func deleteRegion(id: UUID) {
        regionStore.remove(id: id)
        var disabled = Self.disabledRegionIDs()
        disabled.remove(id)
        UserDefaults.standard.set(disabled.map(\.uuidString).sorted(), forKey: Self.disabledIdsKey)
        captureManager.setDisabledRegionIDs(disabled)
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

    private static func disabledRegionIDs() -> Set<UUID> {
        Set((UserDefaults.standard.stringArray(forKey: disabledIdsKey) ?? []).compactMap(UUID.init(uuidString:)))
    }

    func regionEnabled(id: UUID) -> Bool {
        !Self.disabledRegionIDs().contains(id)
    }

    func setRegionEnabled(id: UUID, on: Bool) {
        var disabled = Self.disabledRegionIDs()
        if on { disabled.remove(id) } else { disabled.insert(id) }
        UserDefaults.standard.set(disabled.map(\.uuidString).sorted(), forKey: Self.disabledIdsKey)
        captureManager.setDisabledRegionIDs(disabled)
        objectWillChange.send()
    }

    func quit() {
        Task { @MainActor in
            screenRestartWasRunning = false
            screenRestartTask?.cancel()
            stopAutoCapture()
            if isRunning || captureManager.captureDesired { await captureManager.stop() }
            NSApp.terminate(nil)
        }
    }

    /// Stop capture, hide the render-layer overlay, close the editor if
    /// open, and surface the control panel. Wired to ⌘⇧⌥. and the visible
    /// Panic button. The user wants a single instant action that gets
    /// LiveBlock out of their way without quitting.
    func panicDisable() {
        Task { @MainActor in
            screenRestartWasRunning = false
            screenRestartTask?.cancel()
            stopAutoCapture()
            // Always clear desired intent, including while system-suspended,
            // so wake/unlock or a queued recovery can never resurrect capture.
            await captureManager.stop()
            renderLayer?.orderOut(nil)
            if isEditorOpen { closeEditor() }
            labelingWindow?.orderOut(nil)
            trainingDashboardWindow?.orderOut(nil)
            miniHUDWindow?.orderOut(nil)
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
                alignOverlays(to: screen)
                await captureManager.start(on: screen)
                guard captureManager.isRunning else {
                    NSLog("AppController: capture could not start; screenshot cancelled.")
                    return
                }
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

    /// Select and install a signed offline update package. The package supplies
    /// no trust root; only the keyring embedded in the signed app is accepted.
    func chooseAndInstallSignedModelUpdate() {
        guard !modelUpdateInProgress else { return }
        let panel = NSOpenPanel()
        panel.title = "Choose signed LiveBlock model update"
        panel.prompt = "Verify and install"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let package = panel.url else { return }

        modelUpdateInProgress = true
        modelUpdateStatus = "Verifying signed model update…"
        let captureManager = self.captureManager
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            do {
                let receipt = try MacModelDistribution().installUpdatePackage(at: package) {
                    captureManager.reloadDetectionModel()
                }
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.labelingController.reloadDetectionModel()
                    self.modelUpdateStatus = "Installed model \(receipt.modelVersion) (sequence \(receipt.releaseSequence))."
                    self.modelUpdateInProgress = false
                }
            } catch {
                Task { @MainActor [weak self] in
                    self?.modelUpdateStatus = "Model update rejected: \(error.localizedDescription)"
                    self?.modelUpdateInProgress = false
                }
            }
        }
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

    // MARK: - Helpers

    func currentScreen() -> NSScreen? {
        if let selectedDisplayID,
           let screen = NSScreen.liveBlockScreen(id: selectedDisplayID) {
            return screen
        }
        refreshDisplayTargets()
        guard let selectedDisplayID else { return nil }
        return NSScreen.liveBlockScreen(id: selectedDisplayID)
    }

    func refreshDisplayTargets() {
        let descriptors = NSScreen.liveBlockDescriptors()
        let stored = UserDefaults.standard.object(forKey: Self.selectedDisplayIDKey) as? NSNumber
        let preferred = selectedDisplayID ?? stored?.uint32Value
        let resolved = DisplayTargetResolver.resolvedID(preferred: preferred,
                                                        descriptors: descriptors)
        availableDisplays = descriptors
        selectedDisplayID = resolved
        if let resolved {
            UserDefaults.standard.set(Int(resolved), forKey: Self.selectedDisplayIDKey)
        } else {
            UserDefaults.standard.removeObject(forKey: Self.selectedDisplayIDKey)
        }
    }

    func handleActiveSpaceChange() {
        guard captureManager.captureDesired, let screen = currentScreen() else { return }
        alignOverlays(to: screen)
        if isRunning, !isEditorOpen { renderLayer?.orderFrontRegardless() }
    }

    func handleScreenConfigurationChange() {
        screenRestartWasRunning = screenRestartWasRunning || captureManager.captureDesired
        screenRestartTask?.cancel()
        screenRestartTask = Task { @MainActor [weak self] in
            // macOS emits bursts while displays settle. Debounce them so one
            // stable configuration produces one serialized capture restart.
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled, let self else { return }
            self.refreshDisplayTargets()
            guard let screen = self.currentScreen() else {
                self.screenRestartWasRunning = false
                return
            }
            self.alignOverlays(to: screen)
            guard self.screenRestartWasRunning else { return }
            if self.isRunning { await self.captureManager.stop(preserveIntent: true) }
            guard !Task.isCancelled else { return }
            await self.captureManager.start(on: screen)
            self.screenRestartWasRunning = false
        }
    }

    private func alignOverlays(to screen: NSScreen) {
        renderLayer?.align(to: screen)
        regionEditor?.align(to: screen)
        miniHUDWindow?.align(to: screen)
    }
}
