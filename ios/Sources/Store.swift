import Foundation
import Combine

/// The library on the phone: the UniFFI core behind an `ObservableObject`.
/// Writes go through `dispatch` with the desktop's action names; the core's
/// change listener triggers a snapshot refresh.
@MainActor
final class Store: ObservableObject {
    @Published var items: [Item] = []
    @Published var folders: [Folder] = []
    @Published var settings = Settings(agent: "off", localModel: "", autoFileThreshold: 0.7, syncEnabled: false, syncDir: nil)
    @Published var queue: [String] = []
    @Published var error: String?

    /// Nil until the library is open — see `init`.
    private(set) var core: MemboxCore?
    @Published var ready = false

    /// Opening a library is not main-thread work: it seeds, prunes, and merges
    /// whatever the other devices left in iCloud, which on a real library is
    /// seconds. Doing that inside a SwiftUI StateObject initialiser is the
    /// "frozen right after start" you get for free. So the window paints first
    /// and the library arrives when it arrives.
    ///
    /// No browser is passed: the phone is a viewer (§7.3). Anything captured
    /// here waits for the Mac to fetch it, which is what NoBrowser means — and
    /// it takes the offscreen WKWebView, its main-queue hops and its crash out
    /// of the app entirely.
    init() {
        try? FileManager.default.createDirectory(at: AppGroup.dataDir, withIntermediateDirectories: true)
        let dir = AppGroup.dataDir.path
        Task.detached(priority: .userInitiated) {
            do {
                let core = try MemboxCore.open(dataDir: dir, browser: nil)
                await MainActor.run {
                    self.core = core
                    core.setListener(listener: Listener { [weak self] in Task { @MainActor in self?.refresh() } })
                    self.ready = true
                    self.refresh()
                    self.configureSync()
                }
            } catch {
                await MainActor.run { self.error = "\(error)" }
            }
        }
    }

    /// §7.4 — the iCloud container is where every device's snapshot lives.
    /// nil on the simulator / an unsigned build: sync silently stays off.
    private func configureSync() {
        DispatchQueue.global().async { [weak self] in
            guard let url = FileManager.default.url(forUbiquityContainerIdentifier: "iCloud.com.membox")?.appendingPathComponent("Documents") else { return }
            try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            Task { @MainActor in
                guard let self else { return }
                // First run on a phone that has the container: turn it on. The
                // whole reason the app is on the phone is the library being
                // there, and it is the person's own iCloud. Once syncDir is
                // set, the toggle in the menu is the only thing that decides.
                let first = (self.settings.syncDir ?? "").isEmpty
                self.dispatch("settings", first ? ["syncDir": url.path, "syncEnabled": true] : ["syncDir": url.path])
                self.syncNow()
            }
        }
    }

    @Published var syncStatus = SyncStatus()
    @Published var progress: SyncProgress?

    /// Pull the other devices' files down first — iCloud gives a phone
    /// placeholders until something asks — then merge, then fetch the thumbs
    /// in the background so tiles are not blank.
    ///
    /// All of that stays off the main thread. The merge stats a blob path in
    /// iCloud Drive for every item it sees, and a stat on a file iCloud has
    /// not materialised blocks on the cloud rather than on the CPU — the
    /// watchdog counted 10s of wall clock against 0.02s of app CPU. Hopping
    /// back into `Task { @MainActor }` to call it froze the app on every
    /// foreground and got it killed with 0x8BADF00D. Only the status read,
    /// which is a mutex clone, returns to main.
    ///
    /// Each stage says what it is doing through `progress` — "is it syncing?"
    /// used to have no answer on screen at all. Two guards, not one: a second
    /// merge of the same lists only races the first, but the picture download
    /// can run for minutes and must not hold off a foreground sync that has
    /// new lists to merge.
    @Published private(set) var merging = false
    private var fetchingPictures = false
    /// A sync asked for while one was merging: the running merge may already
    /// have passed the rows it would fill, so one more pass follows it.
    private var again = false

    /// `pictures: false` is the pass after the download: it only collects
    /// what arrived, so a picture that never comes cannot loop the wait.
    func syncNow(pictures: Bool = true) {
        guard let core else { return }
        guard !merging else { again = true; return }
        merging = true
        let dir = settings.syncDir.map { URL(fileURLWithPath: $0) }
        progress = SyncProgress(phase: "Checking iCloud…")
        let say: @Sendable (String, Double?) -> Void = { phase, f in
            Task { @MainActor [weak self] in self?.progress = SyncProgress(phase: phase, fraction: f) }
        }
        // The merge counts its rows in the core; read them while it runs.
        let poll = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { _ in
            Task { @MainActor [weak self] in
                guard let self, self.merging else { return }
                self.readSyncStatus()
                let s = self.syncStatus
                if s.total > 0 { self.progress = SyncProgress(phase: "Merging \(s.done) of \(s.total)", fraction: Double(s.done) / Double(s.total)) }
            }
        }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            if let dir {
                CloudSync.pull(dir.appendingPathComponent("devices")) { say("Downloading the other devices' lists — \($0) left", nil) }
            }
            _ = try? core.dispatch(action: "sync", argsJson: "{}")
            Task { @MainActor in
                poll.invalidate()
                guard let self else { return }
                self.merging = false
                self.progress = nil
                self.readSyncStatus()
                if pictures, let dir { self.fetchPictures(dir.appendingPathComponent("blobs")) }
                if self.again { self.again = false; self.syncNow(pictures: false) }
            }
        }
    }

    /// Pictures come after the rows, so the library is there to look at while
    /// they download; then one more merge puts them on their tiles. Runs at
    /// most once at a time, beside any number of list merges. Its bar only
    /// shows while no merge is using it.
    /// ponytail: re-walks blobs/ every 2s; fine for thousands of files, a
    /// metadata query if it gets to be millions.
    private func fetchPictures(_ blobs: URL) {
        guard !fetchingPictures else { return }
        fetchingPictures = true
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var left = CloudSync.materialise(blobs, deep: true)
            let start = left
            let deadline = Date().addingTimeInterval(600)
            while left > 0 && Date() < deadline {
                let (n, f) = (left, Double(start - left) / Double(start))
                Task { @MainActor [weak self] in
                    guard let self, !self.merging else { return }
                    self.progress = SyncProgress(phase: "Downloading pictures — \(n) left", fraction: f)
                }
                Thread.sleep(forTimeInterval: 2)
                left = CloudSync.materialise(blobs, deep: true)
            }
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.fetchingPictures = false
                if !self.merging { self.progress = nil }
                // The merge skipped every thumb that was not here yet.
                if start > 0 { self.syncNow(pictures: false) }
            }
        }
    }

    func readSyncStatus() {
        guard let json = dispatch("syncStatus"),
              let s = try? JSONDecoder().decode(SyncStatus.self, from: Data(json.utf8)) else { return }
        syncStatus = s
    }

    func refresh() {
        guard let core else { return }
        do {
            let data = Data(try core.snapshotJson().utf8)
            let s = try JSONDecoder().decode(Snapshot.self, from: data)
            items = s.items
            folders = s.folders
            settings = s.settings
            Theme.look = Look(rawValue: s.settings.theme) ?? .bauhaus
            queue = s.queue
        } catch {
            self.error = "\(error)"
        }
    }

    @discardableResult
    func dispatch(_ action: String, _ args: [String: Any] = [:]) -> String? {
        guard let core else { return nil }
        do {
            let json = String(data: try JSONSerialization.data(withJSONObject: args), encoding: .utf8) ?? "{}"
            return try core.dispatch(action: action, argsJson: json)
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    // §1.1 on the phone: the system paste control hands over item providers,
    // and the tap on it *is* the consent — so no "Allow paste from…?" sheet,
    // which reading `UIPasteboard.general` from an ordinary button earned on
    // every tap. Text goes through the same triage the desktop uses (§1.4) —
    // rules only here, since there is no agent lane on a phone — so a list of
    // links becomes a list of items rather than one blob. `done` gets the ids,
    // so the person can still say what these are (`annotate`).
    func capture(_ providers: [NSItemProvider], done: @escaping (_ ids: [String], _ what: String) -> Void) {
        // The provider's own bytes, not a re-encoded UIImage: a photo is
        // already HEIC or JPEG, and `pngData()` turns 2MB into 12 — which is
        // then what every device drags out of iCloud, since the picture *is*
        // the thumb. The core reads the size out of whichever header it gets.
        let imageTypes: [UTType] = [.png, .jpeg, .heic, .heif, .gif, .webP]
        // Slots, not appends: the loads finish on their own queues and in any
        // order, and three copied links should stay three links in that order.
        var images = [Data?](repeating: nil, count: providers.count)
        var lines = [String?](repeating: nil, count: providers.count)
        let group = DispatchGroup()
        for (i, p) in providers.enumerated() {
            group.enter()
            let put: (Data?, String?) -> Void = { d, s in
                DispatchQueue.main.async { images[i] = d; lines[i] = s; group.leave() }
            }
            if let t = imageTypes.first(where: { p.hasItemConformingToTypeIdentifier($0.identifier) }) {
                p.loadDataRepresentation(forTypeIdentifier: t.identifier) { d, _ in put(d, nil) }
            } else if p.canLoadObject(ofClass: URL.self) {
                _ = p.loadObject(ofClass: URL.self) { u, _ in put(nil, u?.absoluteString) }
            } else if p.canLoadObject(ofClass: String.self) {
                _ = p.loadObject(ofClass: String.self) { s, _ in put(nil, s) }
            } else {
                put(nil, nil)
            }
        }
        group.notify(queue: .main) { [self] in
            var ids: [String] = []
            let imgs = images.compactMap { $0 }
            for d in imgs {
                if let id = dispatch("capture", ["imageBase64": d.base64EncodedString()]) {
                    ids.append(id.trimmingCharacters(in: CharacterSet(charactersIn: "\"")))
                }
            }
            var what = imgs.count == 1 ? "1 image" : "\(imgs.count) images"
            let text = lines.compactMap { $0 }
                .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                .joined(separator: "\n")
            if !text.isEmpty,
               let planJson = dispatch("plan", ["text": text, "html": "", "note": ""]),
               let plan = try? JSONSerialization.jsonObject(with: Data(planJson.utf8)) as? [String: Any],
               let entries = plan["entries"] as? [[String: Any]], !entries.isEmpty,
               let out = dispatch("capturePlan", ["plan": plan, "note": ""]),
               let o = try? JSONSerialization.jsonObject(with: Data(out.utf8)) as? [String: Any] {
                ids += (o["ids"] as? [String]) ?? []
                what = entries.count == 1 ? (entries[0]["text"] as? String ?? "1 thing") : "\(entries.count) things"
            }
            done(ids, what)
        }
    }

    /// "these are books I want to read" — the person's word on what they just
    /// pasted (§1.4). The core keeps it in `notes`, where it outranks the page
    /// for the agent and is read by search.
    func annotate(_ ids: [String], _ note: String) {
        dispatch("annotate", ["ids": ids, "note": note])
    }

    /// A sticky (§9), created and opened straight away.
    func newNote() -> String? {
        dispatch("newNote", ["title": "New note"])?
            .trimmingCharacters(in: CharacterSet(charactersIn: "\""))
    }

    func blobURL(_ rel: String?) -> URL? {
        guard let rel, let core else { return nil }
        return URL(fileURLWithPath: core.blobPath(rel: rel))
    }

    func path(of folderId: String) -> String {
        guard let f = folders.first(where: { $0.id == folderId }) else { return "" }
        if let p = f.parentId, let parent = folders.first(where: { $0.id == p }) { return "\(parent.name) › \(f.name)" }
        return f.name
    }

    func items(in view: LibraryView.Selection, query: String) -> [Item] {
        let live = items.filter { !$0.trashed }
        var out: [Item]
        switch view {
        case .all: out = live
        case .uncategorized: out = live.filter { $0.folderIds.isEmpty }
        case .untagged: out = live.filter { $0.tags.isEmpty }
        case .trash: out = items.filter { $0.trashed }
        case .folder(let id):
            let scope = Set([id] + folders.filter { $0.parentId == id }.map(\.id))
            out = live.filter { !Set($0.folderIds).isDisjoint(with: scope) }
        }
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        if !q.isEmpty {
            out = out.filter { i in
                i.title.lowercased().contains(q) || i.tags.contains { $0.contains(q) }
                    || (i.summary ?? "").lowercased().contains(q) || (i.transcript ?? "").lowercased().contains(q)
                    || (i.bodyText ?? "").lowercased().contains(q)
            }
        }
        return out
    }

    func count(_ view: LibraryView.Selection) -> Int { items(in: view, query: "").count }

    /// What is still owed work, newest first — §3.9.
    ///
    /// Not `snapshot.queue`: that is the core's in-memory list of ids, and on a
    /// phone nothing ever drains it. There is no browser here, so the fetch
    /// queue only grows, and an item the Mac finished and synced back still
    /// sits in it — the badge would climb all day and never reach zero. The
    /// rows themselves say what is outstanding, they are what sync corrects,
    /// and they are what the queue sheet can actually show.
    var waiting: [Item] {
        items.filter { !$0.trashed && $0.working }
            .sorted { $0.addedAt > $1.addedAt }
    }
}

private final class Listener: ChangeListener {
    let f: () -> Void
    init(_ f: @escaping () -> Void) { self.f = f }
    func onChange() { f() }
}

import UIKit
import UniformTypeIdentifiers
