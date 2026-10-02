import Foundation
import AppKit
import Combine

/// Invalidates stale asynchronous source-training completions. Cancellation
/// advances ownership before terminating subprocesses; shutdown is terminal.
struct TrainingOperationPolicy {
    private(set) var sequence: UInt64 = 0
    private(set) var activeOperation: UInt64?
    private(set) var shutdownRequested = false

    private mutating func next() -> UInt64 {
        sequence &+= 1
        if sequence == 0 { sequence = 1 }
        return sequence
    }

    mutating func begin() -> UInt64? {
        guard !shutdownRequested, activeOperation == nil else { return nil }
        let operation = next()
        activeOperation = operation
        return operation
    }

    mutating func finish(_ operation: UInt64) {
        if activeOperation == operation { activeOperation = nil }
    }

    mutating func cancel() {
        _ = next()
        activeOperation = nil
    }

    mutating func shutdown() {
        cancel()
        shutdownRequested = true
    }

    func owns(_ operation: UInt64) -> Bool {
        !shutdownRequested && operation != 0 && activeOperation == operation
    }
}

/// Drives the developer-only export → train → candidate pipeline.
///
/// Signed release builds are inference-only and never bootstrap Python or pip.
/// Debug/source builds spawn `tools/export_labels.py` and the candidate runner,
/// stream stdout
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
    /// Most recent exported candidate. Candidates are never auto-installed;
    /// schema-5 verification must produce a passing report first.
    @Published private(set) var candidateModelPath: URL? = nil
    /// Release binaries are inference-only. Training remains an explicit
    /// source/developer workflow until a separately signed companion exists.
    let trainingRuntimeAvailable: Bool

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
    private enum BackgroundOperationKind {
        case environmentSetup
        case sourceTraining
        case verifiedInstall
    }

    private var operationTask: Task<Void, Never>?
    private var activeOperationKind: BackgroundOperationKind?
    private var operationPolicy = TrainingOperationPolicy()

    init(bundle: Bundle = .main) {
        if let value = bundle.object(forInfoDictionaryKey: "LiveBlockTrainingRuntimeEnabled") as? String {
            trainingRuntimeAvailable = ["YES", "true", "1"].contains(value)
        } else if let value = bundle.object(forInfoDictionaryKey: "LiveBlockTrainingRuntimeEnabled") as? NSNumber {
            trainingRuntimeAvailable = value.boolValue
        } else {
#if DEBUG
            trainingRuntimeAvailable = true
#else
            trainingRuntimeAvailable = false
#endif
        }
        recheckVenv()
    }

    var isBusy: Bool {
        if isInstallingEnvironment { return true }
        switch state {
        case .idle, .finished: return false
        default: return true
        }
    }

    func recheckVenv() {
        venvInstalled = trainingRuntimeAvailable
            && FileManager.default.fileExists(atPath: venvPython.path)
    }

    var progressFraction: Double {
        if case .training(let p) = state, p.totalEpochs > 0 {
            return Double(p.epoch) / Double(p.totalEpochs)
        }
        return 0
    }

    // MARK: - Public actions

    func startTraining(epochs: Int = 50, imgsz: Int = 640, batch: Int = 8) {
        guard !operationPolicy.shutdownRequested else { return }
        guard trainingRuntimeAvailable else {
            failWith("Training is unavailable in signed release builds; use the source companion workflow.")
            return
        }
        guard !isBusy, process?.isRunning != true,
              let operation = operationPolicy.begin() else { return }
        activeOperationKind = .sourceTraining
        operationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.runFullPipeline(epochs: epochs, imgsz: imgsz, batch: batch,
                                       operation: operation)
            self.finishOperation(operation)
        }
    }

    func cancel() {
        guard isBusy || operationPolicy.activeOperation != nil || process?.isRunning == true else { return }
        guard activeOperationKind != .verifiedInstall else {
            lastError = "Verified installation cannot be cancelled safely; quit waits for its atomic transaction."
            return
        }
        cancelActiveOperation(message: "Cancelled by user.", immediateKill: false)
    }

    /// Installs the terminal ownership barrier synchronously and returns the
    /// invalidated task for quit to await. Source setup/training is killed;
    /// an authenticated install is allowed to finish its atomic transaction.
    func prepareForShutdown() -> Task<Void, Never>? {
        guard !operationPolicy.shutdownRequested else { return operationTask }
        let hadWork = isBusy || operationPolicy.activeOperation != nil || process?.isRunning == true
        let task = operationTask
        let operationKind = activeOperationKind
        operationPolicy.shutdown()
        activeOperationKind = nil
        operationTask = nil
        isInstallingEnvironment = false
        if hadWork {
            if operationKind != .verifiedInstall {
                task?.cancel()
                terminateActiveProcess(immediateKill: true)
                stopDetachedRunner()
            }
            let message = operationKind == .verifiedInstall
                ? "LiveBlock will quit after verified installation finishes."
                : "Cancelled because LiveBlock is quitting."
            state = .finished(success: false, message: message)
        }
        return task
    }

    func clearTerminalState() {
        guard !operationPolicy.shutdownRequested else { return }
        if case .finished = state {
            state = .idle
            lastError = nil
        }
    }

    /// Run `tools/setup_env.sh`. Streams output through the same logTail
    /// publisher the dashboard already shows.
    func installEnvironment() {
        guard !operationPolicy.shutdownRequested else { return }
        guard trainingRuntimeAvailable else {
            failWith("Release builds never download or install a Python training environment.")
            return
        }
        guard !isInstallingEnvironment, !isBusy, process?.isRunning != true,
              let operation = operationPolicy.begin() else { return }
        activeOperationKind = .environmentSetup
        isInstallingEnvironment = true
        appendLog("=== Installing training environment \(Date()) ===")
        let setup = repoRoot.appendingPathComponent("tools/setup_env.sh")
        guard FileManager.default.fileExists(atPath: setup.path) else {
            appendLog("FAILED: tools/setup_env.sh missing — repo path is wrong.")
            lastError = "tools/setup_env.sh missing at \(setup.path)."
            isInstallingEnvironment = false
            operationPolicy.finish(operation)
            activeOperationKind = nil
            return
        }
        operationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            defer { self.finishOperation(operation) }
            let result = await self.runProcessCapturingOutput(
                url: URL(fileURLWithPath: "/bin/bash"),
                args: [setup.path],
                operation: operation
            )
            guard self.operationIsCurrent(operation) else { return }
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

    private func runFullPipeline(epochs: Int, imgsz: Int, batch: Int,
                                 operation: UInt64) async {
        guard operationIsCurrent(operation) else { return }
        let pipelineStartedAt = Date()
        appendLog("=== Training pipeline started \(pipelineStartedAt) ===")

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
            args: [exportScript.path, "--include-empty", "--class-name", "Ad banner"],
            operation: operation
        )
        guard operationIsCurrent(operation) else { return }
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
                   "--batch", String(batch)],
            operation: operation
        )
        guard operationIsCurrent(operation) else { return }

        if trainResult.exitCode == 0 {
            guard let candidate = newestCoreMLCandidate(modifiedAfter: pipelineStartedAt) else {
                failWith("Training finished, but no exported CoreML candidate was found.")
                return
            }
            candidateModelPath = candidate
            lastSuccessAt = Date()
            state = .finished(
                success: true,
                message: "Candidate exported. It was not installed; schema-5 verification is required."
            )
            appendLog("Candidate only (not installed): \(candidate.path)")
            appendLog("=== Pipeline OK \(Date()) ===")
        } else {
            failWith("Training pipeline failed (exit \(trainResult.exitCode)). See log.")
        }
    }

    /// Install only through the fingerprint-bound verifier. A report picker can
    /// call this after an external/human-reviewed schema-5 run succeeds.
    func installVerifiedModel(reportURL: URL) {
        guard !isBusy, process?.isRunning != true,
              let operation = operationPolicy.begin() else { return }
        activeOperationKind = .verifiedInstall
        state = .installing
        operationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            defer { self.finishOperation(operation) }
            let installer = self.repoRoot.appendingPathComponent("tools/install_verified_model.py")
            let destination = self.runtimeModelDestination()
            do {
                try FileManager.default.createDirectory(
                    at: destination.deletingLastPathComponent(),
                    withIntermediateDirectories: true
                )
            } catch {
                self.failWith("Could not create the runtime model directory: \(error.localizedDescription)")
                return
            }
            let result = await self.runProcessCapturingOutput(
                url: self.venvPython,
                args: [installer.path, "--report", reportURL.path,
                       "--destination", destination.path],
                operation: operation
            )
            guard self.operationIsCurrent(operation) else { return }
            if result.exitCode == 0 {
                self.onModelInstalled?()
                self.state = .finished(success: true,
                                       message: "Verified model installed atomically.")
                self.appendLog("Verified model installed: \(destination.path)")
            } else {
                self.failWith("Verified installation failed (exit \(result.exitCode)).")
            }
        }
    }

    private func runtimeModelDestination() -> URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent("LiveBlock/models/liveblock-detector.mlpackage")
    }

    private func newestCoreMLCandidate(modifiedAfter cutoff: Date) -> URL? {
        let runs = repoRoot.appendingPathComponent("tools/runs", isDirectory: true)
        guard let enumerator = FileManager.default.enumerator(
            at: runs,
            includingPropertiesForKeys: [.contentModificationDateKey, .isDirectoryKey],
            options: [.skipsHiddenFiles]
        ) else { return nil }
        var candidates: [(URL, Date)] = []
        for case let url as URL in enumerator where url.pathExtension == "mlpackage" {
            let values = try? url.resourceValues(forKeys: [.contentModificationDateKey, .isDirectoryKey])
            if values?.isDirectory == true,
               let modified = values?.contentModificationDate,
               modified >= cutoff.addingTimeInterval(-1) {
                candidates.append((url, modified))
                enumerator.skipDescendants()
            }
        }
        return candidates.max(by: { $0.1 < $1.1 })?.0
    }

    private func failWith(_ message: String) {
        appendLog("FAILED: \(message)")
        lastError = message
        state = .finished(success: false, message: message)
    }

    // MARK: - Process plumbing

    private struct RunResult { let exitCode: Int32 }

    private func operationIsCurrent(_ operation: UInt64) -> Bool {
        !Task.isCancelled && operationPolicy.owns(operation)
    }

    private func finishOperation(_ operation: UInt64) {
        guard operationPolicy.owns(operation) else { return }
        operationPolicy.finish(operation)
        activeOperationKind = nil
        operationTask = nil
    }

    private func cancelActiveOperation(message: String, immediateKill: Bool) {
        operationPolicy.cancel()
        operationTask?.cancel()
        operationTask = nil
        activeOperationKind = nil
        isInstallingEnvironment = false
        terminateActiveProcess(immediateKill: immediateKill)
        stopDetachedRunner()
        state = .finished(success: false, message: message)
    }

    private func terminateActiveProcess(immediateKill: Bool) {
        guard let process, process.isRunning else { return }
        let pid = process.processIdentifier
        // The Process is bash (or python). Terminate its direct trainer child
        // before the parent so the child cannot continue after cancellation.
        runOneShot(launchPath: "/usr/bin/pkill", args: ["-TERM", "-P", "\(pid)"], onLine: { _ in })
        process.terminate()
        if immediateKill {
            runOneShot(launchPath: "/usr/bin/pkill", args: ["-KILL", "-P", "\(pid)"], onLine: { _ in })
            if process.isRunning { kill(pid, SIGKILL) }
            return
        }
        // Capture the exact Process. Never consult self.process here: a newer
        // workflow may own that slot by the time escalation runs.
        DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) { [process, pid] in
            Task { @MainActor [weak self] in
                guard process.isRunning else { return }
                self?.runOneShot(launchPath: "/usr/bin/pkill",
                                 args: ["-KILL", "-P", "\(pid)"],
                                 onLine: { _ in })
                if process.isRunning { kill(pid, SIGKILL) }
            }
        }
    }

    private func stopDetachedRunner() {
        // Belt-and-suspenders for a detached runner from a prior invocation.
        runOneShot(launchPath: "/bin/bash",
                   args: [repoRoot.appendingPathComponent("tools/auto.sh").path, "stop"],
                   onLine: { _ in })
    }

    private func runProcessCapturingOutput(url: URL, args: [String],
                                           operation: UInt64) async -> RunResult {
        guard operationIsCurrent(operation) else { return RunResult(exitCode: -1) }
        return await withCheckedContinuation { continuation in
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
                    guard let self, self.operationPolicy.owns(operation) else { return }
                    self.ingestOutput(str)
                }
            }
            errPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                guard !data.isEmpty,
                      let str = String(data: data, encoding: .utf8) else { return }
                Task { @MainActor [weak self] in
                    guard let self, self.operationPolicy.owns(operation) else { return }
                    self.ingestOutput(str)
                }
            }

            proc.terminationHandler = { p in
                outPipe.fileHandleForReading.readabilityHandler = nil
                errPipe.fileHandleForReading.readabilityHandler = nil
                Task { @MainActor [weak self] in
                    guard let self, self.process === proc else { return }
                    self.process = nil
                    self.stdoutPipe = nil
                    self.stderrPipe = nil
                }
                continuation.resume(returning: RunResult(exitCode: p.terminationStatus))
            }

            do {
                try proc.run()
            } catch {
                outPipe.fileHandleForReading.readabilityHandler = nil
                errPipe.fileHandleForReading.readabilityHandler = nil
                if self.process === proc {
                    self.process = nil
                    self.stdoutPipe = nil
                    self.stderrPipe = nil
                }
                continuation.resume(returning: RunResult(exitCode: -1))
                if operationPolicy.owns(operation) {
                    appendLog("Failed to launch \(url.path): \(error.localizedDescription)")
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
