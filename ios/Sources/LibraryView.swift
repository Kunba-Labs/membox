import SwiftUI
import UniformTypeIdentifiers

/// Root: the sidebar as a list (collections, folders), each pushing the grid.
struct LibraryView: View {
    enum Selection: Hashable {
        case all, uncategorized, untagged, trash, folder(String)
    }

    @EnvironmentObject var store: Store
    @State private var showSync = false
    @State private var showQueue = false

    var body: some View {
        NavigationStack {
            ZStack {
                Theme.backdrop
                if !store.ready {
                    VStack(spacing: 10) {
                        ProgressView()
                        Text("Opening your library…").font(.footnote).foregroundStyle(Theme.text3)
                    }
                }
                List {
                    Section {
                        row(.all, "All", "tray")
                        row(.uncategorized, "Uncategorized", "folder.badge.questionmark")
                        row(.untagged, "Untagged", "tag")
                        row(.trash, "Trash", "trash")
                    }
                    Section {
                        ForEach(store.folders.filter { $0.parentId == nil }) { g in
                            row(.folder(g.id), g.name, Self.symbol(g), folder: true, proposed: g.proposed)
                            ForEach(store.folders.filter { $0.parentId == g.id }) { c in
                                row(.folder(c.id), c.name, Self.symbol(c), indent: true, folder: true, proposed: c.proposed)
                            }
                        }
                    } header: { Label3("Folders") }
                }
                .scrollContentBackground(.hidden)
                .modifier(ListLook())
                .opacity(store.ready ? 1 : 0)
            }
            .navigationTitle("membox")
            .navigationDestination(for: Selection.self) { GridView(selection: $0) }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Toggle("Sync through iCloud", isOn: Binding(
                            get: { store.settings.syncEnabled },
                            set: { store.dispatch("settings", ["syncEnabled": $0]); if $0 { store.syncNow() } }))
                        Button("Sync now") { store.syncNow() }.disabled(!store.settings.syncEnabled)
                        Button("Sync status…") { showSync = true }
                        Divider()
                        Picker("Look", selection: Binding(
                            get: { store.settings.theme },
                            set: { store.dispatch("settings", ["theme": $0]) })) {
                            Text("Bauhaus").tag("bauhaus")
                            Text("Glass").tag("glass")
                        }
                    } label: { Label("More", systemImage: "ellipsis.circle") }
                    .tint(Theme.accent)
                }
                ToolbarItem(placement: .topBarLeading) {
                    if !store.waiting.isEmpty {
                        Button { showQueue = true } label: {
                            HStack(spacing: 4) {
                                TurningGlass()
                                Text("\(store.waiting.count)")
                            }
                            .foregroundStyle(Theme.accent).font(.caption)
                        }
                        .accessibilityLabel("\(store.waiting.count) waiting — show the queue")
                    }
                }
            }
            .tint(Theme.accent)
            .sheet(isPresented: $showSync) { SyncSheet().environmentObject(store) }
            .sheet(isPresented: $showQueue) { QueueSheet().environmentObject(store) }
            .captureButtons()
        }
    }

    /// SF Symbols for the seed tree, mirroring the desktop's own glyphs — an
    /// emoji is somebody else's font at somebody else's weight.
    static let folderIcons = [
        "f-seed-watching": "film", "f-seed-reading": "books.vertical",
        "f-seed-listening": "headphones", "f-seed-travel": "globe.europe.africa",
        "f-seed-build": "hammer", "f-seed-notes": "note.text",
    ]
    static let emojiIcons = [
        "🛒": "cart", "🛍": "cart", "🎬": "film", "📚": "books.vertical", "📖": "book",
        "🎧": "headphones", "🌍": "globe.europe.africa", "🛠": "hammer", "📝": "note.text",
        "🎮": "gamecontroller", "🎥": "film", "📺": "tv",
    ]
    static func symbol(_ f: Folder) -> String {
        folderIcons[f.id] ?? emojiIcons[(f.emoji ?? "").trimmingCharacters(in: .whitespaces)] ?? "folder"
    }

    private func row(_ sel: Selection, _ name: String, _ icon: String?, emoji: String? = nil, indent: Bool = false, folder: Bool = false, proposed: Bool = false) -> some View {
        NavigationLink(value: sel) {
            HStack(spacing: 10) {
                if let icon { Image(systemName: icon).foregroundStyle(folder ? Theme.folder : Theme.text2).frame(width: 20) }
                Text(name).foregroundStyle(proposed ? Theme.text2 : Theme.text).italic(proposed)
                if proposed {
                    Text("SUGGESTED").font(.system(size: 9, weight: .semibold)).tracking(1.2)
                        .foregroundStyle(Theme.onAccent)
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(Theme.accent)
                }
                Spacer()
                Text("\(store.count(sel))").foregroundStyle(Theme.text3).monospacedDigit().font(.callout)
            }
            .padding(.leading, indent ? 18 : 0)
        }
        .listRowBackground(Theme.panel)
    }
}


/// Inset and rounded under Glass; full-bleed and ruled under Bauhaus.
struct ListLook: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if Theme.look == .glass {
            content.listStyle(.insetGrouped)
        } else {
            content.listStyle(.plain).listRowSeparatorTint(Theme.rule)
        }
    }
}

/// "Is it actually syncing?" — with an answer instead of a shrug.
struct SyncSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false

    var body: some View {
        NavigationStack {
            List {
                Section("This device") {
                    line("Sync", store.settings.syncEnabled ? "on" : "off")
                    line("Items here", "\(store.items.filter { !$0.trashed }.count)")
                    line("Published", when(store.syncStatus.lastExport))
                }
                Section("Other devices") {
                    line("Seen", "\(store.syncStatus.devices)")
                    line("Last merged", when(store.syncStatus.lastImport))
                    line("Merged rows", "\(store.syncStatus.imported)")
                }
                if let e = store.syncStatus.error {
                    Section("Problem") { Text(e).foregroundStyle(Theme.danger).font(.footnote) }
                }
                Section("Folder") {
                    Text(store.syncStatus.dir ?? store.settings.syncDir ?? "not configured")
                        .font(.system(size: 11, design: .monospaced)).foregroundStyle(Theme.text3)
                }
                Section {
                    Button {
                        busy = true
                        store.syncNow()
                        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { busy = false }
                    } label: {
                        HStack { Text("Sync now"); Spacer(); if busy { ProgressView().controlSize(.mini) } }
                    }
                }
            }
            .navigationTitle("Sync")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
            .onAppear { store.readSyncStatus() }
        }
    }

    private func line(_ k: String, _ v: String) -> some View {
        HStack { Text(k); Spacer(); Text(v).foregroundStyle(Theme.text2) }.font(.footnote)
    }

    private func when(_ iso: String?) -> String {
        guard let iso, let d = ISO8601DateFormatter().date(from: iso) else { return "never" }
        let f = RelativeDateTimeFormatter()
        return f.localizedString(for: d, relativeTo: Date())
    }
}

/// Paste and New note — §1.1. Line symbols at the same weight as the rest of
/// the bar: a filled tray sat next to `ellipsis.circle` like a different set
/// of icons, which is exactly how it read. The meaning still comes from the
/// sidebar's vocabulary — All is a `tray`, so putting something in the box is
/// an arrow into one — rather than from `doc.on.clipboard`, which named the
/// gesture and pointed the wrong way besides.
///
/// They used to sit only on the root, so walking
/// into All or into a folder was a dead end: nothing to tap, and the empty
/// state told you to tap a button that was no longer on screen. The same two
/// buttons everywhere instead.
struct CaptureButtons: ViewModifier {
    @EnvironmentObject var store: Store
    @State private var captured: (ids: [String], what: String)?
    @State private var said = ""
    @State private var note: String?

    func body(content: Content) -> some View {
        content
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        if let id = store.newNote() { note = id }
                    } label: { Label("New note", systemImage: "doc.badge.plus") }
                    .tint(Theme.accent)
                }
            }
            // The system control, not a Button that reads the pasteboard: the
            // tap is the permission, so iOS stops asking "Allow paste from…?"
            // every time (§1.1). It lives alone at the bottom, at its own
            // size: UIPasteControl asks after all when it is squeezed or drawn
            // against a neighbour (Apple, forums thread 714296), which the
            // toolbar's glass group did to it — and the thumb is there anyway.
            .safeAreaInset(edge: .bottom) {
                PasteButton(supportedContentTypes: [.image, .url, .plainText]) { providers in
                    store.capture(providers) { ids, what in captured = (ids, what) }
                }
                .buttonBorderShape(.capsule)
                .controlSize(.large)
                .tint(Theme.accent)
                .padding(.bottom, 6)
            }
            .navigationDestination(item: $note) { NoteView(id: $0) }
            // The phone fetches nothing itself (§7.3), so say where the work
            // happens — and take the one question the desktop sheet asks,
            // "anything about these?", while it is fresh.
            .alert(captured?.ids.isEmpty == false ? "Saved \(captured?.what ?? "")" : "Nothing to save",
                   isPresented: .init(get: { captured != nil }, set: { _ in captured = nil; said = "" })) {
                if let ids = captured?.ids, !ids.isEmpty {
                    TextField("Anything about this?", text: $said)
                    Button("Add note") {
                        let n = said.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !n.isEmpty { store.annotate(ids, n) }
                    }
                    Button("Skip", role: .cancel) {}
                } else {
                    Button("OK", role: .cancel) {}
                }
            } message: {
                if captured?.ids.isEmpty == false {
                    Text("Your Mac fetches, screenshots and files it the next time membox is open there. A note now tells it what this is.")
                }
            }
    }
}

extension View {
    func captureButtons() -> some View { modifier(CaptureButtons()) }
}

/// The hourglass turns itself over. A static one next to a number reads as a
/// label — a count of things that are stuck. Half a turn every second and a
/// bit says the queue is being worked, which is the whole question somebody
/// asks when they look at it. (Half, not a full spin: an hourglass at 180° is
/// very nearly the same shape, so there is no visible snap back.)
struct TurningGlass: View {
    @State private var turned = false

    var body: some View {
        Image(systemName: "hourglass")
            .rotationEffect(.degrees(turned ? 180 : 0))
            .animation(.easeInOut(duration: 1.1).repeatForever(autoreverses: false), value: turned)
            .onAppear { turned = true }
    }
}

/// What the badge is counting — §3.9. The phone fetches nothing itself (§7.3),
/// so nearly everything in here is waiting on the Mac; the sheet says so
/// rather than leaving a row spinning with no explanation, and offers the one
/// button that can hurry it along.
struct QueueSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(store.waiting) { it in
                        HStack(spacing: 10) {
                            if let url = store.blobURL(it.thumb), let ui = UIImage(contentsOfFile: url.path) {
                                Image(uiImage: ui).resizable().aspectRatio(contentMode: .fill)
                                    .frame(width: 42, height: 42).clipShape(RoundedRectangle(cornerRadius: Theme.chipRadius))
                            } else {
                                RoundedRectangle(cornerRadius: Theme.chipRadius).fill(Theme.text3.opacity(0.15))
                                    .frame(width: 42, height: 42)
                                    .overlay { Text(it.kindLabel).font(.system(size: 8)).foregroundStyle(Theme.text3) }
                            }
                            VStack(alignment: .leading, spacing: 2) {
                                Text(it.title).lineLimit(1).font(.footnote)
                                Text(Self.explain(it)).font(.caption2).foregroundStyle(Theme.text3)
                            }
                            Spacer(minLength: 0)
                        }
                    }
                } footer: {
                    Text("This phone saves; your Mac fetches, reads and files. These land here on their own once it has been on.")
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.ground)
            .overlay {
                if store.waiting.isEmpty {
                    VStack(spacing: 8) {
                        Image(systemName: "checkmark.circle").font(.title2).foregroundStyle(Theme.text3)
                        Text("Nothing waiting.").font(.footnote).foregroundStyle(Theme.text3)
                    }
                }
            }
            .navigationTitle("In the queue")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Sync now") { store.syncNow() }.disabled(!store.settings.syncEnabled)
                }
                ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } }
            }
        }
    }

    /// Why this row is still here, in the words of what is actually happening.
    static func explain(_ it: Item) -> String {
        switch it.status {
        case "fetching": return "fetching…"
        case "enriching": return "reading it…"
        case "queued": return "queued for the agent"
        default: return it.error ?? "waiting for your Mac"
        }
    }
}
