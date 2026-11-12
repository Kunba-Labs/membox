import SwiftUI

/// The iPad, laid out like the Mac (desktop App.jsx): sidebar · grid ·
/// inspector, same widths as tokens.css. Still a viewer (§7.3) — what is
/// pasted here waits for the Mac exactly as it does on the phone.
struct DeskView: View {
    @EnvironmentObject var store: Store
    @State private var selection: LibraryView.Selection = .all
    @State private var focus: String?
    @State private var query = ""
    @State private var sidebar = true
    @State private var inspector = true
    @State private var showQueue = false
    @State private var confirmEmpty = false
    @AppStorage("desk.tile") private var size = 220.0
    @AppStorage("desk.sort") private var sort = "added"

    var body: some View {
        HStack(spacing: 0) {
            if sidebar {
                DeskSidebar(selection: $selection).frame(width: 224)
                Theme.rule.frame(width: 1)
            }
            NavigationStack { main }
            // Like the Mac's, but only once there is something to inspect: an
            // empty 268pt pane is a third of an iPad held upright.
            if inspector && focus != nil {
                Theme.rule.frame(width: 1)
                inspectorPane.frame(width: 268)
            }
        }
        .background(Theme.backdrop)
        .onChange(of: selection) { focus = nil; query = "" }
        // Trashed, deleted or filed elsewhere: the inspector lets go with it.
        .onChange(of: store.items) {
            if let f = focus, !store.items(in: selection, query: "").contains(where: { $0.id == f }) { focus = nil }
        }
        .sheet(isPresented: $showQueue) { QueueSheet().environmentObject(store) }
    }

    private var items: [Item] {
        let out = store.items(in: selection, query: query)
        switch sort {
        case "title": return out.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
        case "rating": return out.sorted { $0.rating > $1.rating }
        case "domain": return out.sorted { ($0.domain ?? "~") < ($1.domain ?? "~") }
        default: return out
        }
    }

    private var title: String {
        switch selection {
        case .all: return "All"
        case .uncategorized: return "Uncategorized"
        case .untagged: return "Untagged"
        case .trash: return "Trash"
        case .folder(let id): return store.folders.first { $0.id == id }?.name ?? "Folder"
        }
    }

    private var main: some View {
        let items = items
        return ZStack {
            Theme.backdrop
            if !store.ready {
                VStack(spacing: 10) {
                    ProgressView()
                    Text("Opening your library…").font(.footnote).foregroundStyle(Theme.text3)
                }
            } else if items.isEmpty {
                Text("Nothing here yet — share a link to membox, or tap Paste.")
                    .foregroundStyle(Theme.text3).font(.footnote)
            } else {
                // One column is allowed here: an iPad held upright with both
                // panes open leaves the grid under 300pt.
                Masonry(items: items, column: size, minColumns: 1) { item in
                    Button { focus = item.id; inspector = true } label: {
                        Tile(item: item, active: focus == item.id)
                            // --accent selects, at full strength — Grid.module.css.
                            .overlay(RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                                .strokeBorder(Theme.accent, lineWidth: 2).opacity(focus == item.id ? 1 : 0))
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(item.kindLabel), \(item.title)")
                    .accessibilityAddTraits(focus == item.id ? .isSelected : [])
                    .contextMenu { TileMenu(item: item) }
                }
            }
        }
        .navigationTitle("\(title)  \(items.count)")
        .navigationBarTitleDisplayMode(.inline)
        .searchable(text: $query, prompt: "Search")
        .toolbar {
            ToolbarItemGroup(placement: .topBarLeading) {
                Button { withAnimation(.snappy) { sidebar.toggle() } } label: { Label("Sidebar", systemImage: "sidebar.left") }
                if !store.waiting.isEmpty {
                    Button { showQueue = true } label: {
                        HStack(spacing: 4) { TurningGlass(); Text("\(store.waiting.count)") }.font(.caption)
                    }
                    .accessibilityLabel("\(store.waiting.count) waiting — show the queue")
                }
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                Slider(value: $size, in: 150...420).frame(width: 120).accessibilityLabel("Tile size")
                if case .trash = selection, store.count(.trash) > 0 {
                    Button("Empty Trash", role: .destructive) { confirmEmpty = true }
                }
                Menu {
                    Picker("Sort", selection: $sort) {
                        Text("Date added").tag("added")
                        Text("Title").tag("title")
                        Text("Rating").tag("rating")
                        Text("Source").tag("domain")
                    }
                } label: { Label("Sort", systemImage: "arrow.up.arrow.down") }
                Button { withAnimation(.snappy) { inspector.toggle() } } label: { Label("Inspector", systemImage: "sidebar.right") }
                    .disabled(focus == nil)
            }
        }
        .captureButtons()
        .confirmationDialog("Empty the Trash?", isPresented: $confirmEmpty, titleVisibility: .visible) {
            Button("Delete \(store.count(.trash)) items forever", role: .destructive) { store.dispatch("emptyTrash") }
        } message: { Text("This cannot be undone.") }
    }

    @ViewBuilder private var inspectorPane: some View {
        if let focus { ItemDetailView(id: focus, wholePage: false).id(focus) }
    }
}

/// desktop Sidebar.jsx: the library mark, the collections, the folder tree,
/// ruled rows 27pt high, the current one in --row-current.
struct DeskSidebar: View {
    @Binding var selection: LibraryView.Selection
    @EnvironmentObject var store: Store

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image("LaunchMark").resizable().frame(width: 22, height: 22)
                    .clipShape(RoundedRectangle(cornerRadius: Theme.chipRadius))
                Text("membox").font(.system(size: 13, weight: .semibold)).foregroundStyle(Theme.text)
                Spacer()
                MoreMenu(icon: "gearshape").labelStyle(.iconOnly).foregroundStyle(Theme.text2)
            }
            .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    row(.all, "All", "tray")
                    row(.uncategorized, "Uncategorized", "folder.badge.questionmark")
                    row(.untagged, "Untagged", "tag")
                    row(.trash, "Trash", "trash")
                    Label3("Folders").padding(.horizontal, 8).padding(.top, 18).padding(.bottom, 6)
                    ForEach(store.folders.filter { $0.parentId == nil }) { g in
                        row(.folder(g.id), g.name, LibraryView.symbol(g), folder: true, proposed: g.proposed)
                        ForEach(store.folders.filter { $0.parentId == g.id }) { c in
                            row(.folder(c.id), c.name, LibraryView.symbol(c), indent: true, folder: true, proposed: c.proposed)
                        }
                    }
                }
                .padding(.horizontal, 8).padding(.bottom, 8)
            }
        }
        .background(Color.black.opacity(0.14))
    }

    private func row(_ sel: LibraryView.Selection, _ name: String, _ icon: String, indent: Bool = false, folder: Bool = false, proposed: Bool = false) -> some View {
        let on = selection == sel
        return Button { selection = sel } label: {
            HStack(spacing: 8) {
                Image(systemName: icon).font(.system(size: 13))
                    .foregroundStyle(on ? Theme.rowCurrentInk : folder ? Theme.folder : Theme.text2).frame(width: 18)
                Text(name).font(.system(size: 13)).lineLimit(1).italic(proposed)
                    .foregroundStyle(on ? Theme.rowCurrentInk : proposed ? Theme.text2 : Theme.text)
                if proposed { Label3("suggested", color: on ? Theme.rowCurrentInk : Theme.accent) }
                Spacer(minLength: 0)
                Text("\(store.count(sel))").font(.system(size: 12)).monospacedDigit()
                    .foregroundStyle(on ? Theme.star : Theme.text3)
            }
            .padding(.leading, indent ? 26 : 8).padding(.trailing, 8)
            .frame(height: 30)
            .background(on ? Theme.rowCurrent : .clear, in: RoundedRectangle(cornerRadius: Theme.chipRadius))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
