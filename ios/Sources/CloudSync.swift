import Foundation

/// iCloud Drive hands a phone *placeholders*, not files.
///
/// The Mac writes `devices/d-….json` into the container; on the phone that
/// arrives as a zero-byte stub called `.d-….json.icloud` until something asks
/// for it. The core reads the directory and sees no `.json` at all — which is
/// exactly what "my phone shows nothing" looks like. So before every sync we
/// ask iCloud to materialise what is there, and wait a moment for it.
enum CloudSync {
    /// Start downloading every placeholder under `dir`. Returns how many are
    /// still not local.
    @discardableResult
    static func materialise(_ dir: URL, deep: Bool = false) -> Int {
        let fm = FileManager.default
        let keys: [URLResourceKey] = [.ubiquitousItemDownloadingStatusKey, .isDirectoryKey]
        guard let items = fm.enumerator(at: dir, includingPropertiesForKeys: keys,
                                        options: deep ? [] : [.skipsSubdirectoryDescendants]) else { return 0 }
        var pending = 0
        for case let url as URL in items {
            let values = try? url.resourceValues(forKeys: Set(keys))
            if values?.isDirectory == true { continue }
            let status = values?.ubiquitousItemDownloadingStatus
            if status == .current { continue }
            pending += 1
            try? fm.startDownloadingUbiquitousItem(at: url)
        }
        return pending
    }

    /// Materialise, then wait (briefly) for the files to land. Called on the
    /// sync path, off the main thread.
    static func pull(_ dir: URL, timeout: TimeInterval = 12) {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if materialise(dir) == 0 { return }
            Thread.sleep(forTimeInterval: 0.5)
        }
    }
}

/// What `dispatch("syncStatus")` returns — shown in the app so "is it syncing?"
/// has an answer that isn't a shrug.
struct SyncStatus: Codable {
    var dir: String?
    var lastExport: String?
    var lastImport: String?
    var devices: Int = 0
    var imported: Int = 0
    var error: String?
}
