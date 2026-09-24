import SwiftUI
import WebKit

/// A sticky on the phone — §9. The same rich text the desktop writes, edited in
/// the same way: a contenteditable body in a WKWebView, which is the only rich
/// text editor iOS gives you for free. Swift owns the toolbar; the page owns
/// the caret.
struct NoteView: View {
    let id: String
    @EnvironmentObject var store: Store
    @State private var linking = false
    @State private var editor = NoteEditor()
    /// Set the moment the page reports real text. Kept beside the store's own
    /// copy because the editor saves on a 400ms debounce and on blur, either of
    /// which can still be in flight when the view goes away — and throwing away
    /// a note somebody just wrote is far worse than keeping an empty one.
    @State private var typed = false

    private var item: Item? { store.items.first { $0.id == id } }

    var body: some View {
        let it = item
        let p: (paper: Color, ink: Color) = it.map(Theme.paper) ?? Theme.papers["yellow"]!

        VStack(spacing: 0) {
            if let it {
                TextField("Untitled", text: Binding(
                    get: { it.title },
                    set: { store.dispatch("update", ["id": id, "patch": ["title": $0]]) }
                ))
                .font(.system(size: 20, weight: .bold))
                .foregroundStyle(p.ink)
                .padding(.horizontal, 18).padding(.top, 14).padding(.bottom, 6)

                HStack(spacing: 4) {
                    tool("bold", "B")
                    tool("italic", "I")
                    tool("insertUnorderedList", "list.bullet")
                    Spacer()
                    ForEach(Theme.paperOrder, id: \.self) { c in
                        Button {
                            store.dispatch("update", ["id": id, "patch": ["color": c]])
                        } label: {
                            Circle().fill(Theme.papers[c]!.paper)
                                .frame(width: 16, height: 16)
                                .overlay(Circle().strokeBorder(p.ink.opacity((it.meta?.color ?? "yellow") == c ? 0.9 : 0.2), lineWidth: 1.5))
                        }
                    }
                    Button { linking = true } label: { Image(systemName: "link") }
                        .padding(.leading, 8)
                }
                .foregroundStyle(p.ink.opacity(0.75))
                .padding(.horizontal, 18).padding(.bottom, 8)

                NoteBody(html: it.bodyHtml ?? "", paper: p, editor: editor) { html in
                    if !NoteView.isBlank(html) { typed = true }
                    store.dispatch("update", ["id": id, "patch": ["bodyHtml": html]])
                }
            }
        }
        .background(p.paper.ignoresSafeArea())
        .navigationTitle("")
        .navigationBarTitleDisplayMode(.inline)
        .onDisappear {
            // "New note" opens a real row so the editor has something to write
            // to. Leaving without typing anything should not leave a sticky
            // behind — and an untouched one is worth no tombstone either, so it
            // goes for good rather than into the Trash.
            guard !typed, let it = item, it.kind == "note", it.title == "New note",
                  NoteView.isBlank(it.bodyHtml ?? ""), (it.bodyText ?? "").isEmpty else { return }
            store.dispatch("deleteForever", ["ids": [id]])
        }
        .sheet(isPresented: $linking) {
            LinkPicker { picked in
                editor.insertChip(id: picked.id, title: picked.title)
                linking = false
            }
            .environmentObject(store)
        }
    }

    /// contenteditable never hands back "" — an empty body is `<br>`, a stray
    /// `<div></div>`, or a non-breaking space. Strip the markup and see.
    static func isBlank(_ html: String) -> Bool {
        html.replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
            .replacingOccurrences(of: "&nbsp;", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .isEmpty
    }

    private func tool(_ command: String, _ label: String) -> some View {
        Button {
            editor.run(command)
        } label: {
            if label.count == 1 { Text(label).font(.system(size: 15, weight: .semibold)).frame(width: 30) }
            else { Image(systemName: label).frame(width: 30) }
        }
    }
}

/// The bridge Swift holds onto: the toolbar talks to the page through it.
final class NoteEditor: ObservableObject {
    weak var web: WKWebView?

    func run(_ command: String) {
        web?.evaluateJavaScript("document.execCommand('\(command)');save()")
    }

    func insertChip(id: String, title: String) {
        let safe = title.replacingOccurrences(of: "'", with: "’").replacingOccurrences(of: "<", with: "‹")
        web?.evaluateJavaScript(
            "document.execCommand('insertHTML',false,'<a data-item=\"\(id)\" href=\"#\(id)\" class=\"chip\" contenteditable=\"false\">\(safe)</a>&nbsp;');save()"
        )
    }
}

private struct NoteBody: UIViewRepresentable {
    let html: String
    let paper: (paper: Color, ink: Color)
    let editor: NoteEditor
    let onChange: (String) -> Void

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.userContentController.add(context.coordinator, name: "note")
        let web = WKWebView(frame: .zero, configuration: cfg)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.loadHTMLString(page(), baseURL: nil)
        editor.web = web
        context.coordinator.loaded = html
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        // Only push text the page has not seen — reloading mid-sentence would
        // take the caret with it.
        if context.coordinator.loaded != html {
            context.coordinator.loaded = html
            web.evaluateJavaScript("document.body.innerHTML = \(quote(html))")
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(onChange: onChange) }

    final class Coordinator: NSObject, WKScriptMessageHandler {
        var loaded = ""
        let onChange: (String) -> Void
        init(onChange: @escaping (String) -> Void) { self.onChange = onChange }
        func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
            guard let html = m.body as? String else { return }
            loaded = html
            onChange(html)
        }
    }

    private func quote(_ s: String) -> String {
        String(data: try! JSONSerialization.data(withJSONObject: [s], options: .fragmentsAllowed), encoding: .utf8)
            .map { String($0.dropFirst().dropLast()) } ?? "\"\""
    }

    private func page() -> String {
        let ink = paper.ink.hexString
        let line = paper.ink.hexString + "33"
        return """
        <!doctype html><meta charset=utf-8>
        <meta name=viewport content="width=device-width,initial-scale=1,maximum-scale=1">
        <style>
          html,body{margin:0;background:transparent;color:\(ink);
            font:16px/1.6 -apple-system,system-ui;-webkit-text-size-adjust:100%}
          body{padding:4px 18px 40px;min-height:60vh;outline:none}
          h3{font-size:17px;margin:14px 0 4px}
          ul,ol{padding-left:22px;margin:6px 0}
          blockquote{margin:8px 0;padding-left:12px;border-left:2px solid \(line);opacity:.8}
          .chip{display:inline-block;padding:1px 8px;border-radius:999px;font-size:15px;
            color:\(ink);background:rgba(255,255,255,.55);border:1px solid \(line);text-decoration:none}
        </style>
        <body contenteditable="true">\(html)</body>
        <script>
          let t;
          function save(){clearTimeout(t);t=setTimeout(()=>window.webkit.messageHandlers.note.postMessage(document.body.innerHTML),400)}
          document.body.addEventListener('input', save);
          document.body.addEventListener('blur', () => window.webkit.messageHandlers.note.postMessage(document.body.innerHTML));
        </script>
        """
    }
}

/// Pick something in the library to point at.
private struct LinkPicker: View {
    let pick: (Item) -> Void
    @EnvironmentObject var store: Store
    @State private var q = ""

    var body: some View {
        NavigationStack {
            List(store.items(in: .all, query: q).filter { $0.kind != "note" }.prefix(40)) { it in
                Button { pick(it) } label: {
                    HStack(spacing: 10) {
                        if let url = store.blobURL(it.thumb), let ui = UIImage(contentsOfFile: url.path) {
                            Image(uiImage: ui).resizable().aspectRatio(contentMode: .fill)
                                .frame(width: 44, height: 32).clipShape(RoundedRectangle(cornerRadius: Theme.chipRadius))
                        }
                        Text(it.title).lineLimit(1)
                        Spacer()
                        Text(it.kindLabel).font(.system(size: 9.5)).foregroundStyle(Theme.text3)
                    }
                }
            }
            .searchable(text: $q, prompt: "Search the library")
            .navigationTitle("Link something")
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}

extension Color {
    /// `#rrggbb` for the editor's stylesheet.
    var hexString: String {
        let c = UIColor(self).cgColor.components ?? [0, 0, 0]
        let f = { (i: Int) in Int((c.count > i ? c[i] : 0) * 255) }
        return String(format: "#%02x%02x%02x", f(0), f(1), f(2))
    }
}
