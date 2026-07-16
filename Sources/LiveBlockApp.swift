import SwiftUI
import AppKit
import Combine

@main
struct LiveBlockApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        Settings {
            SettingsView(controller: appDelegate.controller)
        }

        MenuBarExtra {
            MenuBarPopover(controller: appDelegate.controller)
        } label: {
            MenuBarIcon(controller: appDelegate.controller)
        }
        .menuBarExtraStyle(.window)
    }
}

// MARK: - Menu bar UI

private struct MenuBarIcon: View {
    @ObservedObject var controller: AppController
    var body: some View {
        Image(systemName: controller.isRunning
              ? "shield.lefthalf.filled"
              : "shield.slash")
    }
}

/// Rich popover surfacing every action with a button so users never need
/// to memorize a hotkey to use LiveBlock from the menu bar.
private struct MenuBarPopover: View {
    @ObservedObject var controller: AppController

    private var statusLine: String {
        if !controller.screenRecordingGranted { return "Permission needed" }
        if !controller.isRunning { return "Idle" }
        switch controller.pauseReason {
        case .fullscreenApp(let n): return "Paused — \(n) is fullscreen"
        case .excludedApp(let n):   return "Paused — \(n) excluded"
        case .startError(let m):    return "Error: \(m)"
        case .permissionDenied:     return "Permission needed"
        case .stopped:              return "Idle"
        case .userPaused:           return "Paused"
        case .none:                 return "Blocking · \(Int(controller.captureManager.framesPerSecond.rounded())) fps"
        }
    }

    private var statusTint: Color {
        if !controller.screenRecordingGranted { return Theme.warn }
        if !controller.isRunning { return Color.secondary }
        switch controller.pauseReason {
        case .none: return Theme.success
        case .stopped: return Color.secondary
        case .startError, .permissionDenied: return Theme.warn
        default: return Theme.warn
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Status row + on/off toggle
            HStack(spacing: 10) {
                StatusDot(color: statusTint, size: 8, pulse: controller.isRunning)
                VStack(alignment: .leading, spacing: 1) {
                    Text("LiveBlock")
                        .font(Theme.display(size: 14, weight: .bold))
                        .foregroundStyle(Color.primary)
                    Text(statusLine)
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Color.secondary)
                }
                Spacer()
                Toggle("", isOn: Binding(
                    get: { controller.isRunning },
                    set: { _ in controller.toggleCapture() }
                ))
                .labelsHidden()
            }

            Divider()

            // Primary actions
            VStack(spacing: 4) {
                row(label: controller.isEditorOpen ? "Close region editor" : "Draw region",
                    systemImage: "rectangle.dashed",
                    tint: Theme.block) { controller.toggleEditor() }
                row(label: "Capture screenshot",
                    systemImage: "camera",
                    tint: Theme.detect) { controller.captureScreenshotForLabeling() }
                row(label: "Label screenshots  (\(controller.screenshotCount))",
                    systemImage: "rectangle.and.pencil.and.ellipsis",
                    tint: Theme.detect) { controller.showLabelingWindow() }
                row(label: "Training dashboard",
                    systemImage: "brain.head.profile",
                    tint: Theme.train) { controller.showTrainingDashboard() }
                row(label: controller.autoCaptureEnabled ? "Stop auto-capture" : "Start auto-capture (60 s)",
                    systemImage: controller.autoCaptureEnabled ? "stop.circle" : "timer",
                    tint: Theme.detect) { controller.autoCaptureEnabled.toggle() }
                if controller.regionCount > 0 {
                    row(label: "Clear regions  (\(controller.regionCount))",
                        systemImage: "trash",
                        tint: Theme.warn,
                        role: .destructive) { controller.clearRegionsWithConfirm() }
                }
                row(label: controller.detectionEnabled ? "Turn off ML detection" : "Turn on ML detection",
                    systemImage: controller.detectionEnabled ? "viewfinder.circle.fill" : "viewfinder.circle",
                    tint: Theme.detect) { controller.setDetectionEnabled(!controller.detectionEnabled) }
            }

            Divider()

            // Window / app management
            VStack(spacing: 4) {
                row(label: "Show Control Panel",
                    systemImage: "macwindow",
                    tint: Color.secondary) { controller.showControlPanel() }
                row(label: "Show onboarding tour",
                    systemImage: "sparkles",
                    tint: Color.secondary) { controller.restartOnboarding() }
                SettingsLink {
                    rowLabel(label: "Preferences\u{2026}",
                             systemImage: "gearshape",
                             tint: Color.secondary,
                             trailing: "\u{2318},")
                }
                .buttonStyle(.plain)
                .keyboardShortcut(",", modifiers: [.command])
                row(label: "Panic — stop everything",
                    systemImage: "stop.circle.fill",
                    tint: Theme.warn,
                    role: .destructive) { controller.panicDisable() }
                row(label: "Quit LiveBlock",
                    systemImage: "power",
                    tint: Color.secondary,
                    role: .destructive) { controller.quit() }
            }
        }
        .padding(14)
        .frame(width: 300)
    }

    @ViewBuilder
    private func row(label: String,
                     systemImage: String,
                     tint: Color,
                     role: ButtonRole? = nil,
                     action: @escaping () -> Void) -> some View {
        Button(role: role, action: action) {
            rowLabel(label: label, systemImage: systemImage, tint: tint)
        }
        .buttonStyle(.plain)
    }

    private func rowLabel(label: String,
                          systemImage: String,
                          tint: Color,
                          trailing: String? = nil) -> some View {
        HStack(spacing: 10) {
            Image(systemName: systemImage)
                .frame(width: 18)
                .foregroundStyle(tint)
            Text(label)
                .font(Theme.ui(size: 12, weight: .medium))
                .foregroundStyle(Color.primary)
            Spacer()
            if let trailing {
                Text(trailing)
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Color.secondary.opacity(0.6))
            }
        }
        .contentShape(Rectangle())
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
    }
}

// MARK: - App delegate

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let controller = AppController()
    private let hotKeys = HotKeyMonitor()
    private var cancellables = Set<AnyCancellable>()

    func applicationDidFinishLaunching(_ notification: Notification) {
        Theme.registerFonts()
        guard let screen = controller.currentScreen() ?? NSScreen.main ?? NSScreen.screens.first else {
            NSLog("LiveBlock: no display is available during launch.")
            return
        }

        // Render layer (always click-through)
        let renderLayer = RenderLayerWindow(
            rootView: AnyView(RenderLayerView(captureManager: controller.captureManager)),
            targetScreen: screen
        )
        controller.renderLayer = renderLayer

        // Region editor (on demand)
        let regionEditor = RegionEditorWindow(
            rootView: AnyView(RegionEditorView(controller: controller)),
            targetScreen: screen
        )
        controller.regionEditor = regionEditor

        // Control panel (always visible main UI)
        let controlPanel = ControlPanelWindow(
            rootView: AnyView(ControlPanelView(controller: controller))
        )
        controller.controlPanel = controlPanel
        controlPanel.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        // Labeling window (created hidden; shown on demand)
        let labelingWindow = LabelingWindow(
            rootView: AnyView(LabelingView(labeling: controller.labelingController,
                                            controller: controller))
        )
        labelingWindow.onWillClose = { [weak self] in
            self?.controller.labelingController.saveLabels()
        }
        controller.labelingWindow = labelingWindow

        // Training dashboard window (also hidden; shown on demand)
        let trainingDashboard = TrainingDashboardWindow(
            rootView: AnyView(TrainingDashboardView(controller: controller,
                                                     training: controller.trainingController,
                                                     labeling: controller.labelingController))
        )
        controller.trainingDashboardWindow = trainingDashboard

        // Mini HUD — shown automatically while capture is running.
        let miniHUD = MiniHUDWindow(
            rootView: AnyView(MiniHUDView(controller: controller))
        )
        controller.miniHUDWindow = miniHUD
        miniHUD.align(to: screen)

        // Show / hide HUD with capture state.
        controller.captureManager.$isRunning
            .receive(on: RunLoop.main)
            .sink { [weak self] running in
                if running { self?.controller.showMiniHUD() }
                else { self?.controller.hideMiniHUD() }
            }
            .store(in: &cancellables)

        NotificationCenter.default.publisher(for: NSApplication.didChangeScreenParametersNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.controller.handleScreenConfigurationChange() }
            .store(in: &cancellables)

        NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.activeSpaceDidChangeNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.controller.handleActiveSpaceChange() }
            .store(in: &cancellables)

        // Build the onboarding window up-front so the menu bar's "Show
        // onboarding tour" can re-show it later. It is created hidden;
        // first-launch is what calls showOnboarding().
        let onboarding = OnboardingWindow(
            rootView: AnyView(OnboardingView(
                onFinish: { [weak self] in
                    self?.controller.finishOnboarding()
                },
                onTryFirstBlock: { [weak self] in
                    // Start blocking AND open the editor through one sequenced
                    // action. Panic/stop/quit can invalidate the delayed open.
                    self?.controller.startFirstBlockFromOnboarding()
                }
            ))
        )
        controller.onboardingWindow = onboarding
        if !UserDefaults.standard.bool(forKey: "didOnboard") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak controller = self.controller] in
                controller?.showOnboarding()
            }
        }

        // Show the render layer only when capture is running AND the user
        // is not editing regions. Hiding while the editor is open lets the
        // user see the unblocked frame they're drawing on top of (and
        // sidesteps the z-order problem where the click-through render
        // layer sits visually above the editor).
        Publishers.CombineLatest(
            controller.captureManager.$isRunning,
            controller.$isEditorOpen
        )
        .receive(on: RunLoop.main)
        .sink { [weak self] running, editing in
            guard let self else { return }
            if running && !editing && self.controller.allowsRenderVisibility {
                self.controller.renderLayer?.orderFrontRegardless()
            } else {
                self.controller.renderLayer?.orderOut(nil)
            }
        }
        .store(in: &cancellables)

        // Hotkeys
        hotKeys.register(key: "l", flags: [.command, .shift]) { [weak self] in
            self?.controller.toggleCapture()
        }
        hotKeys.register(key: "b", flags: [.command, .shift]) { [weak self] in
            self?.controller.toggleEditor()
        }
        hotKeys.register(key: ".", flags: [.command, .shift, .option]) { [weak self] in
            // Panic disable — actually stop capture and hide overlays.
            self?.controller.panicDisable()
        }
        hotKeys.register(key: "s", flags: [.command, .shift]) { [weak self] in
            self?.controller.captureScreenshotForLabeling()
        }
        hotKeys.start()
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !controller.quitIsReady else { return .terminateNow }
        controller.quitWhenApplicationRequestsTermination {
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        // Closing the control panel must not quit the app — the menu bar /
        // Dock icon should keep it discoverable.
        false
    }

    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu()
        let show = NSMenuItem(title: "Show Control Panel",
                              action: #selector(showControlPanelFromDock),
                              keyEquivalent: "")
        show.target = self
        menu.addItem(show)
        return menu
    }

    @objc private func showControlPanelFromDock() {
        controller.showControlPanel()
    }
}
