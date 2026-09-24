import UIKit
import UniformTypeIdentifiers

/// "Add to membox" in the Share sheet — §7.2, the phone's primary capture path.
/// Opens the same library in the App Group container and captures directly;
/// the main app sees the new row on its next refresh. No browser here (an
/// extension is short-lived), so URLs wait for the app to open.
final class ShareViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = UIColor(red: 0.08, green: 0.10, blue: 0.15, alpha: 1)
        let label = UILabel()
        label.text = "Saving to membox…"
        label.textColor = .white
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([label.centerXAnchor.constraint(equalTo: view.centerXAnchor), label.centerYAnchor.constraint(equalTo: view.centerYAnchor)])
        Task { await capture() }
    }

    private func capture() async {
        // One capture per attachment: sharing four photos at once used to keep
        // only the last, because they all wrote the same key in one dictionary.
        var captures: [[String: Any]] = []
        for item in extensionContext?.inputItems as? [NSExtensionItem] ?? [] {
            for p in item.attachments ?? [] {
                if p.hasItemConformingToTypeIdentifier(UTType.url.identifier), let u = try? await p.loadItem(forTypeIdentifier: UTType.url.identifier) as? URL {
                    captures.append(["text": u.absoluteString])
                } else if p.hasItemConformingToTypeIdentifier(UTType.image.identifier), let data = try? await imageData(p) {
                    captures.append(["imageBase64": data.base64EncodedString()])
                } else if p.hasItemConformingToTypeIdentifier(UTType.plainText.identifier), let s = try? await p.loadItem(forTypeIdentifier: UTType.plainText.identifier) as? String {
                    captures.append(["text": s])
                }
            }
        }
        if captures.isEmpty { return done() }
        // Opening the library merges what the other devices left in iCloud,
        // which stats a blob path per item and blocks on the cloud. This is a
        // UIViewController, so `capture()` is main-actor bound — doing it here
        // hangs the share sheet the same way it used to hang the app. Off the
        // main thread; only `done()` comes back to it.
        let dir = AppGroup.dataDir
        let jsons = captures.compactMap { (try? JSONSerialization.data(withJSONObject: $0)).flatMap { String(data: $0, encoding: .utf8) } }
        await Task.detached(priority: .userInitiated) {
            do {
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                let core = try MemboxCore.open(dataDir: dir.path, browser: nil)
                for json in jsons {
                    _ = try core.dispatch(action: "capture", argsJson: json)
                }
                // The library's own worker exports ~1s after a change, and this
                // process is gone long before that: a link shared from Safari
                // sat in the phone's database until someone opened the app,
                // which is the one thing the share sheet exists to avoid. Push
                // it to iCloud here, while we are still alive, so the Mac can
                // pick the fetch up (§7.3).
                _ = try core.dispatch(action: "sync", argsJson: "{}")
            } catch {
                NSLog("membox share: \(error)")
            }
        }.value
        done()
    }

    private func imageData(_ p: NSItemProvider) async throws -> Data {
        let any = try await p.loadItem(forTypeIdentifier: UTType.image.identifier)
        if let url = any as? URL { return try Data(contentsOf: url) }
        if let img = any as? UIImage, let png = img.pngData() { return png }
        if let d = any as? Data { return d }
        throw NSError(domain: "membox", code: 1)
    }

    private func done() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            self.extensionContext?.completeRequest(returningItems: nil)
        }
    }
}
