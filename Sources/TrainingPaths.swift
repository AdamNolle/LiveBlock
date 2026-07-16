import Foundation

/// File-system layout for the labeling pipeline.
///
///   ~/Library/Application Support/LiveBlock/training/
///     screenshots/    full-resolution PNGs captured for labeling
///     labels/         one JSON file per labeled screenshot (same stem)
///     exports/        YOLO-format datasets exported by tools/export_labels.py
///     trash/          screenshots the user marked unusable (kept locally for review)
enum TrainingPaths {

    static var root: URL {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory,
                                                  in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return appSupport
            .appendingPathComponent("LiveBlock", isDirectory: true)
            .appendingPathComponent("training", isDirectory: true)
    }

    static var screenshots: URL { root.appendingPathComponent("screenshots", isDirectory: true) }
    static var labels: URL      { root.appendingPathComponent("labels", isDirectory: true) }
    static var exports: URL     { root.appendingPathComponent("exports", isDirectory: true) }
    static var trash: URL       { root.appendingPathComponent("trash", isDirectory: true) }

    /// Ensure the labeling root and all subdirectories are owner-only. Existing
    /// directories are tightened on every call so older installs do not retain
    /// umask-derived group/world access to full-resolution screenshots.
    @discardableResult
    static func ensureDirectories(at rootURL: URL = root) -> Bool {
        let fm = FileManager.default
        let directories = [
            rootURL,
            rootURL.appendingPathComponent("screenshots", isDirectory: true),
            rootURL.appendingPathComponent("labels", isDirectory: true),
            rootURL.appendingPathComponent("exports", isDirectory: true),
            rootURL.appendingPathComponent("trash", isDirectory: true),
        ]
        do {
            for url in directories {
                try fm.createDirectory(at: url, withIntermediateDirectories: true)
                try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
            }
            return true
        } catch {
            NSLog("TrainingPaths: failed to create private directories: \(error.localizedDescription)")
            return false
        }
    }

    /// Generate a deterministic, sortable filename stem for a fresh capture.
    static func newScreenshotStem() -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.dateFormat = "yyyyMMdd-HHmmss-SSS"
        return formatter.string(from: Date()) + "Z"
    }

    static func labelURL(forScreenshot url: URL) -> URL {
        let stem = url.deletingPathExtension().lastPathComponent
        return labels.appendingPathComponent(stem + ".json")
    }
}
