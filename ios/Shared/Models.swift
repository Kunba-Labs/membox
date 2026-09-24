import Foundation

// The core's JSON shapes (core/src/model.rs), decoded as-is.

struct Item: Codable, Identifiable, Hashable {
    let id: String
    var kind: String
    var title: String
    var url: String?
    var domain: String?
    var thumb: String?
    var pageShot: String?
    var aspect: Double
    var tags: [String]
    var autoTags: [String]?
    var folderIds: [String]
    var rating: Int
    var duration: String?
    var status: String
    var error: String?
    var bodyText: String?
    var summary: String?
    var notes: String?
    var transcript: String?
    var agentReason: String?
    var confidence: Double?
    var addedAt: String
    var size: String
    var dimensions: String
    var palette: [String]
    var trashed: Bool
    var userEdited: Bool
    var updatedAt: String?
    var bodyHtml: String?
    var meta: Meta?

    var kindLabel: String {
        switch kind {
        case "webpage": return "WEB"
        case "youtube_video", "vimeo": return "VIDEO"
        case "youtube_music", "music": return "MUSIC"
        case "youtube_playlist": return "PLAYLIST"
        case "instagram": return "REEL"
        case "x_post": return "POST"
        case "github_repo": return "REPO"
        case "image": return "IMG"
        case "snippet": return "TEXT"
        case "note": return "NOTE"
        case "book": return "BOOK"
        case "movie": return "FILM"
        case "tv": return "SERIES"
        case "game": return "GAME"
        case "product": return "PRODUCT"
        case "hn": return "HN"
        default: return kind.uppercased()
        }
    }

    /// Working, or stopped? A spinner on something that gave up is a lie.
    var working: Bool { ["pending", "fetching", "queued", "enriching"].contains(status) }

    /// The thread behind a Hacker News or Reddit save (§3.5).
    var thread: Meta.Thread? { meta?.hn ?? meta?.reddit }
}

/// Only the parts of `meta` the phone reads; the rest stays on the item.
struct Meta: Codable, Hashable {
    var color: String?
    var links: [String]?
    var author: String?
    var developer: String?
    var query: String?
    var hn: Thread?
    var reddit: Thread?

    struct Thread: Codable, Hashable {
        var url: String?
        var points: Int?
        var comments: Int?
        var subreddit: String?
        var author: String?
    }
}

extension Item {
    static func == (a: Item, b: Item) -> Bool {
        a.id == b.id && a.status == b.status && a.title == b.title && a.rating == b.rating
            && a.thumb == b.thumb && a.bodyHtml == b.bodyHtml && a.meta?.color == b.meta?.color
    }
    func hash(into h: inout Hasher) { h.combine(id) }
}

struct Folder: Codable, Identifiable, Hashable {
    let id: String
    var name: String
    var emoji: String?
    var parentId: String?
    var proposed: Bool
}

struct Settings: Codable {
    var agent: String
    var localModel: String
    var autoFileThreshold: Double
    var syncEnabled: Bool = false
    var syncDir: String?
    /// "bauhaus" | "glass" — §6.9, the same setting the desktop writes.
    var theme: String = "bauhaus"
}

struct Snapshot: Codable {
    var items: [Item]
    var folders: [Folder]
    var settings: Settings
    var queue: [String]
}

/// The App Group container both the app and the share extension open —
/// one SQLite file, one blob store.
enum AppGroup {
    static let id = "group.com.membox"
    static var dataDir: URL {
        let base = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: id)
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("membox", isDirectory: true)
    }
}
