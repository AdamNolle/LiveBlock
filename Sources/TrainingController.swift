import Foundation
import AppKit
import Combine

/// Drives the export → train → install → rebuild pipeline as an in-app job.
///
/// Spawns `tools/export_labels.py` followed by `tools/auto.sh`, streams stdout
/// in real time, parses ultralytics' epoch lines into structured progress
/// updates, and exposes everything as `@Published` state for the dashboard.
@MainActor
final class TrainingController: ObservableObject {

    enum State: Equatable {
        case idle
        case exporting
        case training(progress: TrainingProgress)
        case installing
        case rebuilding
        case finished(success: Bool, message: String)
    }

    struct TrainingProgress: Equatable {
        var epoch: Int
        var totalEpochs: Int
        var box_loss: Double
        var cls_loss: Double
        var map50: Double?
        var map50_95: Double?
    }

    @Published private(set) var state: State = .idle
    @Published private(set) var logTail: [String] = []      // last N lines for the dashboard
    @Published private(set) var lastSuccessAt: Date? = nil
    @Published private(set) var lastError: String? = nil
    @Published private(set) var datasetPath: URL? = nil
    /// Mirror of `tools/.venv` presence — drives the dashboard's
    /// "Install training environment" precondition card.
    @Published private(set) var venvInstalled: Bool = false
    /// True while `tools/setup_env.sh` is running.
    @Published private(set) var isInstallingEnvironment: Bool = false

    /// Hook the running app's VisionProcessor sets so `reloadModel()` can
    /// be called from this controller after a successful install. Kept as
    /// a closure to avoid a hard cross-import.
    var onModelInstalled: (() -> Void)?

    private let logTailLimit = 200
    private var process: Process?
    private var stdoutPipe: Pipe?
    private var stderrPipe: Pipe?

    init() {
        recheckVenv()
    }

    var isBusy: Bool {
        switch state {
        case .idle, .finished: return false
        default: return true
        }
    }

    func recheckVenv() {
        venvInstalled = FileManager.default.fileExists(atPath: venvPython.path)
    }

    var progressFraction: Double {
        if case .training(let p) = state, p.totalEpochs > 0 {
            return Double(p.epoch) / Double(p.totalEpochs)
        }
        return 0
    }

    // MARK: - Public actions

    func startTraining(epochs: Int = 50, imgsz: Int = 640, batch: Int = 8) {
        guard !isBusy else { return }
        Task { await runFullPipeline(epochs: epochs, imgsz: imgsz, batch: batch) }
    }

    func cancel() {
        guard isBusy else { return }
        if let process = process, process.isRunning {
            // The Process is bash (or python). Bash spawns a python child
            // (ultralytics trainer). `pkill -P <bash-pid>` reaps the child;
            // process.terminate() reaps bash itself. Without the pkill the
            // python child keeps training even after the bash is gone.
            let pid = process.processIdentifier
            runOneShot(launchPath: "/usr/bin/pkill",
                       args: ["-TERM", "-P", "\(pid)"],
                       onLine: { _ in })
            process.terminate()
            // Escalate to SIGKILL on anything still alive after a beat.
            DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) { [pid] in
                Task { @MainActor in
                    self.runOneShot(launchPath: "/usr/bin/pkill",
                                    args: ["-KILL", "-P", "\(pid)"],
                                    onLine: { _ in })
                    if let p = self.process, p.isRunning {
                        kill(p.processIdentifier, SIGKILL)
                    }
                }
            }
        }
        // Belt-and-suspenders: also tell auto.sh's stop in case there's a
        // detached runner from a prior invocation.
        runOneShot(launchPath: "/bin/bash",
                   args: [repoRoot.appendingPathComponent("tools/auto.sh").path, "stop"],
                   onLine: { _ in })
        state = .finished(success: false, message: "Cancelled by user.")
    }

    func clearTerminalState() {
        if case .finished = state {
            state = .idle
            lastError = nil
        }
    }

    /// Run `tools/setup_env.sh`. Streams output through the same logTail
    /// publisher the dashboard already shows.
    func installEnvironment() {
        guard !isInstallingEnvironment, !isBusy else { return }
        isInstallingEnvironment = true
        appendLog("=== Installing training environment \(Date()) ===")
        let setup = repoRoot.appendingPathComponent("tools/setup_env.sh")
        guard FileManager.default.fileExists(atPath: setup.path) else {
            appendLog("FAILED: tools/setup_env.sh missing — repo path is wrong.")
            lastError = "tools/setup_env.sh missing at \(setup.path)."
            isInstallingEnvironment = false
            return
        }
        Task {
            let result = await runProcessCapturingOutput(
                url: URL(fileURLWithPath: "/bin/bash"),
                args: [setup.path]
            )
            self.isInstallingEnvironment = false
            self.recheckVenv()
            if result.exitCode == 0 {
                self.appendLog("=== Environment installed OK \(Date()) ===")
                self.lastError = nil
            } else {
                self.appendLog("FAILED: setup_env.sh exit \(result.exitCode)")
                self.lastError = "Setup failed (exit \(result.exitCode)). See log."
            }
        }
    }

    // MARK: - Pipeline

    /// Best-effort repo root resolution. LaunchServices doesn't propagate env
    /// variables so we use a config file written by `run.sh`, plus fallbacks:
    ///   1. `~/Library/Application Support/LiveBlock/repo_path.txt`
    ///   2. `LIVEBLOCK_REPO` environment variable (Xcode-launched runs)
    ///   3. Walk up from the bundle looking for `project.yml`
    ///   4. `~/Desktop/Code/LiveBlock` and `~/LiveBlock`
    private var repoRoot: URL {
        let fm = FileManager.default

        // 1. Config file written by run.sh
        let configFile = URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library/Application Support/LiveBlock/repo_path.txt")
        if let raw = try? String(contentsOf: configFile, encoding: .utf8) {
            let path = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            if !path.isEmpty {
                let url = URL(fileURLWithPath: path)
                if fm.fileExists(atPath: url.appendingPathComponent("project.yml").path) {
                    return url
                }
            }
        }

        // 2. Env var (Xcode debug runs preserve env)
        if let envPath = ProcessInfo.processInfo.environment["LIVEBLOCK_REPO"],
           !envPath.isEmpty {
            let url = URL(fileURLWithPath: envPath)
            if fm.fileExists(atPath: url.appendingPathComponent("project.yml").path) {
                return url
            }
        }

        // 3 + 4. Walk up from the bundle, then check default locations.
        var candidates: [URL] = []
        var bundle = URL(fileURLWithPath: Bundle.main.bundlePath)
        for _ in 0..<8 {
            bundle = bundle.deletingLastPathComponent()
            candidates.append(bundle)
        }
        candidates.append(URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Desktop/Code/LiveBlock"))
        candidates.append(URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("LiveBlock"))

        for candidate in candidates {
            if fm.fileExists(atPath: candidate.appendingPathComponent("project.yml").path) {
                return candidate
            }
        }
        // Last resort — cwd. Will fail downstream with a clear error.
        return URL(fileURLWithPath: fm.currentDirectoryPath)
    }

    private var venvPython: URL {
        repoRoot.appendingPathComponent("tools/.venv/bin/python")
    }

    private func runFullPipeline(epochs: Int, imgsz: Int, batch: Int) async {
        appendLog("=== Training pipeline started \(Date()) ===")

        guard FileManager.default.fileExists(atPath: venvPython.path) else {
            failWith("tools/.venv missing — run `tools/setup_env.sh` first.")
            return
        }

        // Step 1: Export labels to YOLO format.
        state = .exporting
        let exportScript = repoRoot.appendingPathComponent("tools/export_labels.py")
        let exportRoot = TrainingPaths.exports

        let exportResult = await runProcessCapturingOutput(
            url: venvPython,
            args: [exportScript.path]
        )
        guard exportResult.exitCode == 0 else {
            failWith("Export failed (exit \(exportResult.exitCode)). See log.")
            return
        }

        // Find the freshly exported data.yaml
        guard let dataYaml = newestExportYaml(in: exportRoot) else {
            failWith("Export script reported success but no data.yaml found.")
            return
        }
        datasetPath = dataYaml

        // Step 2: Run auto.sh in foreground (so we can stream output ourselves).
        // auto.sh's normal mode backgrounds itself; instead we directly run the
        // unattended runner, which is the inner workhorse.
        let runner = repoRoot.appendingPathComponent("tools/_unattended_runner.sh")
        guard FileManager.default.fileExists(atPath: runner.path) else {
            failWith("tools/_unattended_runner.sh is missing.")
            return
        }

        state = .training(progress: TrainingProgress(epoch: 0,
                                                     totalEpochs: epochs,
                                                     box_loss: 0,
                                                     cls_loss: 0))
        let trainResult = await runProcessCapturingOutput(
            url: URL(fileURLWithPath: "/bin/bash"),
            args: [runner.path,
                   repoRoot.path,
                   dataYaml.path,
                   "--epochs", String(epochs),
                   "--imgsz", String(imgsz),
                   "--batch", String(batch),
                   "--install"]
        )

        if trainResult.exitCode == 0 {
            // Copy the freshly trained model into Application Support so the
            // running app picks it up without a relaunch.
            installFreshlyTrainedModel()
            // Tell VisionProcessor to drop its cached model — next inference
            // will reload from the runtime dir we just populated.
            onModelInstalled?()
            lastSuccessAt = Date()
            state = .finished(success: true, message: "Trained model installed. Detection updated.")
            appendLog("=== Pipeline OK \(Date()) ===")
        } else {
            failWith("Training pipeline failed (exit \(trainResult.exitCode)). See log.")
        }
    }

    /// Copy the trained `.mlpackage` into Application Support so the running
    /// VisionProcessor finds it before falling back to the bundled model.
    private func installFreshlyTrainedModel() {
        let fm = FileManager.default
        let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support")
        let modelDir = support.appendingPathComponent("LiveBlock/models", isDirectory: true)
        try? fm.createDirectory(at: modelDir, withIntermediateDirectories: true)

        // The trainer writes new weights into `Sources/yolov8n.mlpackage` in
        // the repo. Copy that into the runtime dir so SCStream-driven
        // inference picks it up immediately.
        let src = repoRoot.appendingPathComponent("Sources/yolov8n.mlpackage")
        guard fm.fileExists(atPath: src.path) else {
            appendLog("Hot-reload skipped: \(src.path) not found.")
            return
        }
        let dst = modelDir.appendingPathComponent("yolov8n.mlpackage")
        try? fm.removeItem(at: dst)
        do {
            try fm.copyItem(at: src, to: dst)
            appendLog("Hot-reloaded model into \(dst.path)")
        } catch {
            appendLog("Hot-reload copy failed: \(error.localizedDescription)")
        }
    }

    private func failWith(_ message: String) {
        appendLog("FAILED: \(message)")
        lastError = message
        state = .finished(success: false, message: message)
    }

    // MARK: - Process plumbing

    private struct RunResult { let exitCode: Int32 }

    private func runProcessCapturingOutput(url: URL, args: [String]) async -> RunResult {
        await withCheckedContinuation { continuation in
            let proc = Process()
            proc.executableURL = url
            proc.arguments = args
            proc.currentDirectoryURL = repoRoot
            // Inherit env so PATH includes brew, etc.
            var env = ProcessInfo.processInfo.environment
            env["PYTHONUNBUFFERED"] = "1"
            proc.environment = env
            proc.qualityOfService = .userInitiated

            let outPipe = Pipe()
            let errPipe = Pipe()
            proc.standardOutput = outPipe
            proc.standardError = errPipe
            self.stdoutPipe = outPipe
            self.stderrPipe = errPipe
            self.process = proc

            outPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                guard !data.isEmpty,
                      let str = String(data: data, encoding: .utf8) else { return }
                Task { @MainActor [weak self] in self?.ingestOutput(str) }
            }
            errPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                guard !data.isEmpty,
                      let str = String(data: data, encoding: .utf8) else { return }
                Task { @MainActor [weak self] in self?.ingestOutput(str) }
            }

            proc.terminationHandler = { p in
                outPipe.fileHandleForReading.readabilityHandler = nil
                errPipe.fileHandleForReading.readabilityHandler = nil
                Task { @MainActor [weak self] in
                    self?.process = nil
                    self?.stdoutPipe = nil
                    self?.stderrPipe = nil
                }
                continuation.resume(returning: RunResult(exitCode: p.terminationStatus))
            }

            do {
                try proc.run()
            } catch {
                continuation.resume(returning: RunResult(exitCode: -1))
                Task { @MainActor [weak self] in
                    self?.appendLog("Failed to launch \(url.path): \(error.localizedDescription)")
                }
            }
        }
    }

    private func runOneShot(launchPath: String, args: [String], onLine: @escaping (String) -> Void) {
        let proc = Process()
        proc.launchPath = launchPath
        proc.arguments = args
        proc.standardOutput = Pipe()
        proc.standardError = Pipe()
        try? proc.run()
        proc.waitUntilExit()
    }

    // MARK: - Output parsing

    private func ingestOutput(_ chunk: String) {
        for raw in chunk.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            if line.isEmpty { continue }
            appendLog(line)
            updateProgress(from: line)
        }
    }

    private func appendLog(_ line: String) {
        logTail.append(line)
        if logTail.count > logTailLimit {
            logTail.removeFirst(logTail.count - logTailLimit)
        }
    }

    /// Parses ultralytics' per-epoch progress line, e.g.
    /// `      1/50      1.05G      1.234      1.567      0.987      4      640: 100%|...`
    /// or table rows showing mAP@50, mAP@50-95.
    private func updateProgress(from line: String) {
        guard case .training(var progress) = state else { return }

        let trimmed = line.trimmingCharacters(in: .whitespaces)
        let parts = trimmed.split(separator: " ", omittingEmptySubsequences: true).map(String.init)

        // Per-epoch progress line: starts with `<epoch>/<total>` followed by ≥3 numeric fields.
        if let first = parts.first,
           let slashIdx = first.firstIndex(of: "/"),
           let epoch = Int(first[..<slashIdx]),
           let total = Int(first[first.index(after: slashIdx)...]),
           parts.count >= 5 {
            // parts[1] is GPU memory (e.g. "1.05G"), parts[2..4] are losses.
            if let boxLoss = Double(parts[2]),
               let clsLoss = Double(parts[3]) {
                progress.epoch = epoch
                progress.totalEpochs = total
                progress.box_loss = boxLoss
                progress.cls_loss = clsLoss
                state = .training(progress: progress)
                return
            }
        }

        // mAP table row: "all <images> <instances> <P> <R> <mAP50> <mAP50-95>"
        if parts.first == "all", parts.count >= 7 {
            if let m50 = Double(parts[5]), let m95 = Double(parts[6]) {
                progress.map50 = m50
                progress.map50_95 = m95
                state = .training(progress: progress)
            }
        }
    }

    // MARK: - Helpers

    private func newestExportYaml(in root: URL) -> URL? {
        let fm = FileManager.default
        guard let dirs = try? fm.contentsOfDirectory(at: root,
                                                     includingPropertiesForKeys: [.contentModificationDateKey],
                                                     options: [.skipsHiddenFiles]) else { return nil }
        let yamls = dirs
            .map { $0.appendingPathComponent("data.yaml") }
            .filter { fm.fileExists(atPath: $0.path) }
            .sorted { lhs, rhs in
                let l = (try? lhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                let r = (try? rhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                return l > r
            }
        return yamls.first
    }
}
