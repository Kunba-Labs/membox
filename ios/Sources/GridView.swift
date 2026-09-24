import SwiftUI

struct GridView: View {
    let selection: LibraryView.Selection
    @EnvironmentObject var store: Store
    @State private var query = ""
    @State private var confirmEmpty = false

    private var isTrash: Bool { if case .trash = selection { return true }; return false }

    var title: String {
        switch selection {
        case .all: return "All"
        case .uncategorized: return "Uncategorized"
        case .untagged: return "Untagged"
        case .trash: return "Trash"
        case .folder(let id): return store.folders.first { $0.id == id }?.name ?? "Folder"
        }
    }

    var body: some View {
        ZStack {
            Theme.backdrop
            let items = store.items(in: selection, query: query)
            if items.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "doc.on.clipboard").font(.title).foregroundStyle(Theme.text3)
                    Text("Nothing here yet — share a link to membox, or tap Paste.").foregroundStyle(Theme.text3).font(.footnote).multilineTextAlignment(.center)
                }.padding()
            } else {
                GeometryReader { geo in
                    // Round-robin columns, not a LazyVGrid: a grid row is as
                    // tall as its tallest tile, which squares every preview off
                    // and loses the desktop's waterfall — §6.3.
                    let cols = max(2, Int((geo.size.width - 24) / 190))
                    ScrollView {
                        HStack(alignment: .top, spacing: 10) {
                            ForEach(Array(0..<cols), id: \.self) { c in
                                LazyVStack(spacing: 10) {
                                    ForEach(items.enumerated().filter { $0.offset % cols == c }.map(\.element)) { item in
                                        NavigationLink(value: item) { Tile(item: item) }
                                            .buttonStyle(.plain)
                                            .contextMenu { menu(for: item) }
                                    }
                                }
                                .frame(maxWidth: .infinity)
                            }
                        }
                        .padding(12)
                    }
                }
            }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .searchable(text: $query, prompt: "Search")
        .navigationDestination(for: Item.self) { ItemDetailView(id: $0.id) }
        .captureButtons()
        .toolbar {
            if isTrash && !store.items(in: .trash, query: "").isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(role: .destructive) { confirmEmpty = true } label: { Label("Empty Trash", systemImage: "trash.slash") }
                }
            }
        }
        .confirmationDialog("Empty the Trash?", isPresented: $confirmEmpty, titleVisibility: .visible) {
            Button("Delete \(store.count(.trash)) items forever", role: .destructive) { store.dispatch("emptyTrash") }
        } message: {
            Text("This cannot be undone.")
        }
    }

    /// Long-press a tile to get rid of it — the one thing the phone could not
    /// do. Trash is reversible and syncs as a flag; forever is a tombstone the
    /// other devices honour (§7.4).
    @ViewBuilder private func menu(for item: Item) -> some View {
        if item.trashed {
            Button { store.dispatch("trash", ["ids": [item.id], "trashed": false]) } label: {
                Label("Put back", systemImage: "arrow.uturn.backward")
            }
            Button(role: .destructive) { store.dispatch("deleteForever", ["ids": [item.id]]) } label: {
                Label("Delete forever", systemImage: "xmark.bin")
            }
        } else {
            Button(role: .destructive) { store.dispatch("trash", ["ids": [item.id], "trashed": true]) } label: {
                Label("Move to Trash", systemImage: "trash")
            }
        }
    }
}

struct Tile: View {
    let item: Item
    @EnvironmentObject var store: Store

    /// A link whose screenshot hasn't landed yet — desktop Grid.jsx.
    private var isPlaceholder: Bool {
        !["snippet", "note"].contains(item.kind) && item.thumb == nil && item.pageShot == nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ZStack(alignment: .topLeading) {
                media
                if item.kind != "note" {
                    Badge(text: item.kindLabel).padding(7)
                }
                if let d = item.duration {
                    VStack { Spacer(); HStack { Spacer(); Badge(text: d).padding(7) } }
                } else if let dom = item.domain, showsDomain {
                    VStack { Spacer(); HStack { Badge(text: dom).padding(7); Spacer() } }
                }
            }
            Rectangle().fill(Theme.rule).frame(height: Theme.captionRule)
            HStack(spacing: 6) {
                Text(item.title).font(.system(size: 11.5)).lineLimit(1).foregroundStyle(Theme.text)
                Spacer(minLength: 0)
                if item.rating > 0 { Text("★\(item.rating)").font(.system(size: 10)).foregroundStyle(Theme.star) }
                if !isPlaceholder && item.working {
                    ProgressView().controlSize(.mini)
                } else if !isPlaceholder, item.error != nil {
                    Image(systemName: "exclamationmark.triangle").font(.system(size: 10)).foregroundStyle(Theme.danger)
                }
            }
            .padding(.horizontal, 9).padding(.vertical, 7)
        }
        .panel()
        .clipShape(RoundedRectangle(cornerRadius: Theme.radius, style: .continuous))
    }

    /// The 4:5 page frame — §4.5. Posters and stills keep their own shape.
    private var pageFrame: Bool {
        ["webpage", "hn", "reddit"].contains(item.kind) && item.pageShot != nil
    }

    /// Only where the desktop shows it: on a page shot and on a tile still
    /// waiting for one. A poster doesn't need the site it came from stamped on it.
    private var showsDomain: Bool { item.domain != nil && (pageFrame || isPlaceholder) }

    /// Kind is a colour before it is a word — but only where there is nothing
    /// on top of it: a waveform or a line of text needs the neutral panel
    /// behind it, or the colour eats the words. Desktop Grid.module.css.
    private var pending: Color { Theme.blank(item.kind) }

    /// The shot, cropped from the top into a frame of the given ratio — the
    /// desktop's `object-fit: cover; object-position: top`.
    private func shot(_ ui: UIImage, ratio: Double) -> some View {
        Color.clear
            .aspectRatio(ratio, contentMode: .fit)
            .overlay(alignment: .top) {
                Image(uiImage: ui).resizable().aspectRatio(contentMode: .fill)
            }
            .clipped()
    }

    private func image(_ rel: String?) -> UIImage? {
        guard let url = store.blobURL(rel) else { return nil }
        return UIImage(contentsOfFile: url.path)
    }

    @ViewBuilder private var media: some View {
        if isPlaceholder {
            // The 4:5 frame it will become, with the fetch state in the middle,
            // so the column doesn't jump when the shot lands — §4.5.
            Color.clear.aspectRatio(4.0 / 5.0, contentMode: .fit)
                .overlay {
                    if item.working {
                        VStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text(item.status).font(.system(size: 10.5)).foregroundStyle(Theme.text3)
                        }
                    } else if let e = item.error {
                        VStack(spacing: 6) {
                            Image(systemName: "exclamationmark.triangle").foregroundStyle(Theme.danger)
                            Text(e).font(.system(size: 10.5)).foregroundStyle(Theme.danger).multilineTextAlignment(.center)
                        }.padding(12)
                    } else {
                        Text("no preview").font(.system(size: 11)).foregroundStyle(Theme.text3)
                    }
                }
                .background(item.working || item.error != nil ? Theme.panel2 : pending)
        } else if item.kind == "note" {
            // A sticky looks like one on the phone too (§9).
            let p = Theme.paper(item)
            let links = item.meta?.links?.count ?? 0
            Text(item.bodyText ?? "")
                .font(.system(size: 11.5)).foregroundStyle(p.ink).lineLimit(9)
                .frame(maxWidth: .infinity, minHeight: 96, alignment: .topLeading)
                .padding(.top, 28).padding(.horizontal, 12).padding(.bottom, 12)
                .background(p.paper)
                .overlay(alignment: .bottomTrailing) {
                    if links > 0 {
                        Text("\(links) linked")
                            .font(.system(size: 10)).foregroundStyle(p.ink)
                            .padding(.horizontal, 7).padding(.vertical, 1)
                            .background(p.ink.opacity(0.12))
                            .padding(7)
                    }
                }
        } else if item.kind == "youtube_music" || item.kind == "music" {
            // A waveform instead of a poster — §6.4.
            Color.clear.aspectRatio(3.0 / 2.0, contentMode: .fit)
                .overlay {
                    GeometryReader { g in
                        HStack(spacing: 2) {
                            ForEach(Array(Tile.bars(item.id).enumerated()), id: \.offset) { _, v in
                                Rectangle().fill(Theme.text2.opacity(0.5))
                                    .frame(maxWidth: .infinity, minHeight: 1)
                                    .frame(height: g.size.height * v)
                            }
                        }
                        .frame(width: g.size.width, height: g.size.height)
                    }
                    .padding(.horizontal, 12)
                }
                .background(Theme.panel2)
        } else if item.kind == "snippet" {
            // Text: the readable body, small, faded out at the bottom.
            Text(item.summary ?? item.url ?? item.title)
                .font(item.tags.contains("code") ? .system(size: 10.5, design: .monospaced) : .system(size: 11))
                .foregroundStyle(Theme.text2).lineLimit(10)
                .frame(maxWidth: .infinity, alignment: .topLeading)
                .padding(.top, 30).padding(.horizontal, 12).padding(.bottom, 16)
                .background(Theme.panel2)
                .mask(LinearGradient(colors: [.black, .black, .clear], startPoint: .top, endPoint: .bottom))
        } else if pageFrame, let ui = image(item.thumb ?? item.pageShot) {
            // One fixed 4:5 frame for pages, like the desktop's — §4.5. The
            // phone has no hover, so it stays on the viewport shot.
            shot(ui, ratio: 4.0 / 5.0)
        } else if let ui = image(item.thumb ?? item.pageShot) {
            // Everything else keeps its own shape: posters stay tall, video
            // stills stay wide. That variety is what makes the column read.
            shot(ui, ratio: max(0.4, min(2.5, item.aspect)))
                .overlay {
                    if item.kind == "youtube_video" || item.kind == "vimeo" || item.kind == "instagram" {
                        Image(systemName: "play.circle.fill")
                            .font(.system(size: 34)).foregroundStyle(.white.opacity(0.9))
                    }
                }
        } else {
            Text(item.summary ?? item.url ?? item.title)
                .font(.system(size: 11)).foregroundStyle(Theme.text2).lineLimit(8)
                .frame(maxWidth: .infinity, alignment: .topLeading)
                .padding(.top, 30).padding(.horizontal, 12).padding(.bottom, 12)
                .background(Theme.panel2)
        }
    }

    /// Deterministic pseudo-waveform, same shape as the desktop's — a tile that
    /// reshuffled on every render would be noise.
    static func bars(_ id: String, n: Int = 28) -> [Double] {
        let seed = Double(id.unicodeScalars.reduce(0) { $0 &+ Int($1.value) } % 97)
        return (0..<n).map { i in
            let v = sin(seed * 3.7 + Double(i) * 0.8) * cos(Double(i) * 0.31)
            return 0.18 + abs(v) * 0.78
        }
    }
}
