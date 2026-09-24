import SwiftUI

struct ItemDetailView: View {
    let id: String
    @EnvironmentObject var store: Store
    @State private var newTag = ""
    @Environment(\.dismiss) private var dismiss

    private var item: Item? { store.items.first { $0.id == id } }

    var body: some View {
        // A sticky opens as itself — the editor is the view (§9).
        if item?.kind == "note" {
            return AnyView(NoteView(id: id))
        }
        return AnyView(detail)
    }

    private var detail: some View {
        ZStack {
            Theme.backdrop
            if let item {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        if let url = store.blobURL(item.pageShot ?? item.thumb), let ui = UIImage(contentsOfFile: url.path) {
                            Image(uiImage: ui).resizable().scaledToFit()
                                .clipShape(RoundedRectangle(cornerRadius: Theme.radius, style: .continuous))
                        }
                        HStack(spacing: 6) {
                            ForEach(item.palette, id: \.self) { c in Rectangle().fill(Color(css: c)).frame(width: 14, height: 14) }
                        }.padding(8).panel()

                        TextField("Title", text: Binding(get: { item.title }, set: { store.dispatch("update", ["id": id, "patch": ["title": $0]]) }))
                            .font(.headline).foregroundStyle(Theme.text).padding(9).panel()

                        if let u = item.url, let link = URL(string: u) {
                            Link(destination: link) { Label(u, systemImage: "link").lineLimit(1).font(.footnote) }
                        }

                        section("Tags") {
                            FlowChips(values: item.tags, auto: Set(item.autoTags ?? []), tint: Theme.panel2,
                                      onKeep: { store.dispatch("addTag", ["id": id, "tag": $0]) }) { store.dispatch("removeTag", ["id": id, "tag": $0]) }
                            HStack {
                                TextField("add tag", text: $newTag).textInputAutocapitalization(.never).font(.footnote)
                                Button("Add") { store.dispatch("addTag", ["id": id, "tag": newTag]); newTag = "" }.disabled(newTag.isEmpty)
                            }.padding(8).panel()
                        }

                        section("Folders") {
                            FlowChips(values: item.folderIds.map { store.path(of: $0) }, tint: Theme.chipFolder, ink: Theme.chipFolderInk, onKeep: nil) { path in
                                if let f = item.folderIds.first(where: { store.path(of: $0) == path }) { store.dispatch("removeFromFolder", ["id": id, "folderId": f]) }
                            }
                            Menu("Add to folder…") {
                                ForEach(store.folders) { f in
                                    Button(store.path(of: f.id)) { store.dispatch("addToFolder", ["ids": [id], "folderId": f.id]) }
                                }
                            }.font(.footnote)
                        }

                        if let s = item.summary, !s.isEmpty {
                            section(item.status == "ready" ? "Summary" : item.status) { Text(s).font(.footnote).foregroundStyle(Theme.text2) }
                        }
                        if let r = item.agentReason {
                            section("Filed by agent" + (item.confidence.map { " · \(Int($0 * 100))%" } ?? "")) { Text(r).font(.footnote).foregroundStyle(Theme.text2) }
                        }
                        // The other half of a Hacker News or Reddit save (§3.5).
                        if let t = item.thread, let u = t.url, let link = URL(string: u) {
                            Link(destination: link) {
                                HStack(spacing: 8) {
                                    Image(systemName: "bubble.left.and.bubble.right.fill")
                                        .foregroundStyle(item.meta?.hn != nil ? Color(hex: 0xFF6600) : Color(hex: 0xFF4500))
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text("Discussion on " + (t.subreddit.map { "r/\($0)" } ?? "Hacker News")).font(.footnote)
                                        Text("\(t.points ?? 0) points · \(t.comments ?? 0) comments")
                                            .font(.system(size: 11)).foregroundStyle(Theme.text3).monospacedDigit()
                                    }
                                    Spacer()
                                }
                                .padding(9).panel()
                            }
                        }

                        if let e = item.error, !item.working {
                            HStack(spacing: 8) {
                                Image(systemName: "exclamationmark.triangle").foregroundStyle(Theme.danger)
                                Text(e).font(.footnote).foregroundStyle(Theme.danger)
                                Spacer()
                                Button("Retry") { store.dispatch("reenrich", ["ids": [id]]) }.font(.footnote)
                            }
                            .padding(9).panel()
                        }
                        if let t = item.transcript {
                            section("Transcript") { Text(t).font(.system(size: 11, design: .monospaced)).foregroundStyle(Theme.text2).textSelection(.enabled) }
                        }

                        section("Rating") {
                            HStack(spacing: 4) {
                                ForEach(1...5, id: \.self) { n in
                                    Image(systemName: n <= item.rating ? "star.fill" : "star")
                                        .foregroundStyle(n <= item.rating ? Theme.star : Theme.text3)
                                        .onTapGesture { store.dispatch("setRating", ["id": id, "rating": n]) }
                                }
                            }
                        }

                        HStack {
                            Button { store.dispatch("reenrich", ["ids": [id]]) } label: { Label("Re-enrich", systemImage: "bolt") }
                            Spacer()
                            Button(role: .destructive) { store.dispatch("trash", ["ids": [id], "trashed": !item.trashed]); dismiss() } label: {
                                Label(item.trashed ? "Put back" : "Trash", systemImage: "trash")
                            }
                        }.font(.footnote).padding(.top, 6)
                    }
                    .padding(16)
                }
            } else {
                Text("Gone").foregroundStyle(Theme.text3)
            }
        }
        .navigationBarTitleDisplayMode(.inline)
    }

    private func section<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Label3(title)
            Rectangle().fill(Theme.rule).frame(height: Theme.headingRule)
            content()
        }
    }
}

struct FlowChips: View {
    let values: [String]
    var auto: Set<String> = []
    let tint: Color
    var ink: Color = Theme.text
    var onKeep: ((String) -> Void)? = nil
    let onRemove: (String) -> Void
    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(values, id: \.self) { v in
                let isAuto = auto.contains(v)
                HStack(spacing: 4) {
                    if isAuto { Image(systemName: "bolt.fill").font(.system(size: 8)).foregroundStyle(Theme.accent) }
                    Text(v).font(.footnote).foregroundStyle(isAuto ? Theme.text2 : ink)
                    if isAuto, let onKeep { Image(systemName: "checkmark").font(.system(size: 9)).foregroundStyle(Theme.accent).onTapGesture { onKeep(v) } }
                    Image(systemName: "xmark").font(.system(size: 9)).foregroundStyle(Theme.text3).onTapGesture { onRemove(v) }
                }
                .padding(.horizontal, 9).padding(.vertical, 5)
                .background(isAuto ? Color.clear : tint, in: RoundedRectangle(cornerRadius: Theme.chipRadius))
                .overlay(RoundedRectangle(cornerRadius: Theme.chipRadius).strokeBorder(Theme.rule, style: StrokeStyle(lineWidth: 1, dash: isAuto ? [3, 3] : [])))
            }
        }
    }
}

/// Chips wrap like words, not like grid cells — a chip never breaks mid-word.
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, rowH: CGFloat = 0
        for v in subviews {
            let sz = v.sizeThatFits(.unspecified)
            if x > 0 && x + sz.width > width { x = 0; y += rowH + spacing; rowH = 0 }
            x += sz.width + spacing
            rowH = max(rowH, sz.height)
        }
        return CGSize(width: width == .infinity ? x : width, height: y + rowH)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, rowH: CGFloat = 0
        for v in subviews {
            let sz = v.sizeThatFits(.unspecified)
            if x > bounds.minX && x + sz.width > bounds.maxX { x = bounds.minX; y += rowH + spacing; rowH = 0 }
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(sz))
            x += sz.width + spacing
            rowH = max(rowH, sz.height)
        }
    }
}
