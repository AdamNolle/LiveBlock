import Foundation
import AppKit
import Combine
import CoreML
import UniformTypeIdentifiers

/// Drives the export → train → install pipeline as an in-app job.
///
/// Runs the foreground training entry point, streams stdout
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
    @Published var selectedDataset: URL? = nil
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
    private var jobGeneration = UUID()
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

    func chooseDataset() {
        guard !isBusy else { return }
        let panel = NSOpenPanel()
        panel.title = "Choose a YOLO dataset configuration"
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = ["yaml", "yml"].compactMap { UTType(filenameExtension: $0) }
        if panel.runModal() == .OK { selectedDataset = panel.url }
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
        guard (1...1000).contains(epochs), (1...128).contains(batch),
              (320...2048).contains(imgsz), imgsz % 32 == 0 else {
            failWith("Use 1–1000 epochs, batch 1–128, and an image size from 320–2048 divisible by 32.")
            return
        }
        jobGeneration = UUID()
        let generation = jobGeneration
        state = .exporting
        lastError = nil
        Task { await runFullPipeline(epochs: epochs, imgsz: imgsz, batch: batch, generation: generation) }
    }

    func cancel() {
        guard isBusy else { return }
        jobGeneration = UUID()
        if let active = process, active.isRunning {
            // Kill only descendants of this foreground job.
            runOneShot(launchPath: "/usr/bin/pkill",
                       args: ["-TERM", "-P", "\(active.processIdentifier)"],
                       onLine: { _ in })
            active.terminate()
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                if active.isRunning { kill(active.processIdentifier, SIGKILL) }
            }
        }
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

    private func runFullPipeline(epochs: Int, imgsz: Int, batch: Int, generation: UUID) async {
        guard generation == jobGeneration else { return }
        appendLog("=== Training pipeline started \(Date()) ===")
        guard FileManager.default.fileExists(atPath: venvPython.path) else {
            failWith("tools/.venv missing — run tools/setup_env.sh first.")
            return
        }
        state = .training(progress: TrainingProgress(epoch: 0, totalEpochs: epochs, box_loss: 0, cls_loss: 0))
        var arguments: [String]
        if let selectedDataset {
            arguments = [repoRoot.appendingPathComponent("tools/train_logos.py").path,
                         "--data", selectedDataset.path, "--install"]
            datasetPath = selectedDataset
        } else {
            arguments = [repoRoot.appendingPathComponent("tools/train_from_labels.py").path,
                         "--root", TrainingPaths.root.path]
        }
        arguments += ["--epochs", String(epochs), "--imgsz", String(imgsz), "--batch", String(batch)]
        let trainResult = await runProcessCapturingOutput(url: venvPython, args: arguments)
        guard generation == jobGeneration else { return }
        if selectedDataset == nil { datasetPath = newestExportYaml(in: TrainingPaths.exports) }

        if trainResult.exitCode == 0 {
            // Copy the freshly trained model into Application Support so the
            // running app picks it up without a relaunch.
            do { try await installFreshlyTrainedModel(generation: generation) }
            catch {
                guard generation == jobGeneration else { return }
                failWith("Model installation failed: \(error.localizedDescription)")
                return
            }
            // Tell VisionProcessor to drop its cached model — next inference
            // will reload from the runtime dir we just populated.
            guard generation == jobGeneration else { return }
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
    private func installFreshlyTrainedModel(generation: UUID) async throws {
        let fm = FileManager.default
        let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support")
        let modelDir = support.appendingPathComponent("LiveBlock/models", isDirectory: true)
        try fm.createDirectory(at: modelDir, withIntermediateDirectories: true)

        // The trainer writes new weights into `Sources/yolov8n.mlpackage` in
        // the repo. Copy that into the runtime dir so SCStream-driven
        // inference picks it up immediately.
        let src = repoRoot.appendingPathComponent("Sources/yolov8n.mlpackage")
        guard fm.fileExists(atPath: src.path) else {
            throw NSError(domain: "LiveBlockTraining", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "Trained model missing at \(src.path)"
            ])
        }
        let compiled = try await Task.detached(priority: .utility) {
            try MLModel.compileModel(at: src)
        }.value
        defer { try? fm.removeItem(at: compiled) }
        guard generation == jobGeneration else { throw CancellationError() }
        let dst = modelDir.appendingPathComponent("yolov8n.mlmodelc")
        let staged = modelDir.appendingPathComponent("\(UUID().uuidString).mlmodelc")
        try fm.copyItem(at: compiled, to: staged)
        defer { try? fm.removeItem(at: staged) }
        if fm.fileExists(atPath: dst.path) {
            _ = try fm.replaceItemAt(dst, withItemAt: staged)
        } else {
            try fm.moveItem(at: staged, to: dst)
        }
        appendLog("Installed compiled model into \(dst.path)")
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
                Task { @MainActor [weak self] in
                    guard self?.process === proc else { return }
                    self?.ingestOutput(str)
                }
            }
            errPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                guard !data.isEmpty,
                      let str = String(data: data, encoding: .utf8) else { return }
                Task { @MainActor [weak self] in
                    guard self?.process === proc else { return }
                    self?.ingestOutput(str)
                }
            }

            proc.terminationHandler = { p in
                outPipe.fileHandleForReading.readabilityHandler = nil
                errPipe.fileHandleForReading.readabilityHandler = nil
                Task { @MainActor [weak self] in
                    if self?.process === p {
                        self?.process = nil
                        self?.stdoutPipe = nil
                        self?.stderrPipe = nil
                    }
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
