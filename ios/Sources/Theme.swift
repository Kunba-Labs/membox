import SwiftUI

/// Which design. desktop/src/tokens.css holds the same two.
enum Look: String {
    case bauhaus, glass
}

/// desktop/src/tokens.css, mirrored — spec §6.9.
///
/// Two looks, and the difference is shape and softness as much as colour, so
/// radius, blur and the way a label is set are values here rather than
/// decisions taken in a view. `Store` sets `look` from settings.theme, and the
/// root view is keyed on it so the tree rebuilds when it changes.
enum Theme {
    static var look: Look = .bauhaus
    private static var glass: Bool { look == .glass }

    static var ground: Color { glass ? Color(hex: 0x141A26) : Color(hex: 0x0D0D10) }
    static var panel: Color { glass ? Color.white.opacity(0.045) : Color(hex: 0x17171C) }
    static var panel2: Color { glass ? Color.white.opacity(0.08) : Color(hex: 0x202027) }
    static var rule: Color { glass ? Color.white.opacity(0.08) : Color(hex: 0x2C2C34) }

    static var accent: Color { glass ? Color(hex: 0x4A9EFF) : Color(hex: 0x2D51E0) }
    static var folder: Color { glass ? Color(hex: 0xE8A0C8) : Color(hex: 0xD93B2B) }
    static var star: Color { glass ? Color(hex: 0xF5B942) : Color(hex: 0xF0B323) }
    static var onAccent: Color { glass ? Color(hex: 0x0D1117) : Color(hex: 0xF2F1EE) }

    static var danger: Color { glass ? Color(hex: 0xFF6B6B) : folder }
    static var warn: Color { glass ? Color(hex: 0xFFB86B) : star }

    static var text: Color { glass ? Color(hex: 0xE8EDF5) : Color(hex: 0xF2F1EE) }
    static var text2: Color { glass ? Color(hex: 0x8A93A3) : Color(hex: 0x98979F) }
    static var text3: Color { glass ? Color(hex: 0x5A6272) : Color(hex: 0x66656D) }

    /// Shape and softness — the other half of the difference.
    static var radius: CGFloat { glass ? 10 : 0 }
    static var chipRadius: CGFloat { glass ? 8 : 0 }
    static var tileBorder: CGFloat { glass ? 0 : 1 }
    static var captionRule: CGFloat { glass ? 0 : 1 }
    static var headingRule: CGFloat { glass ? 0 : 1 }
    /// A folder is a block of red under Bauhaus, a wash of pink under Glass.
    static var chipFolder: Color { glass ? folder.opacity(0.14) : folder }
    static var chipFolderInk: Color { glass ? text : onAccent }
    static var labelTracking: CGFloat { glass ? 0.2 : 1.2 }
    static var labelUppercased: Bool { !glass }
    static var badgeFill: AnyShapeStyle {
        glass ? AnyShapeStyle(.ultraThinMaterial) : AnyShapeStyle(ground)
    }
    /// A tile with nothing to show yet: its kind's colour, or the old slate.
    static func blank(_ kind: String) -> Color {
        guard !glass else { return Color(hex: 0x1B2330) }
        switch kind {
        case "youtube_video", "vimeo", "instagram": return accent
        case "image": return star
        case "youtube_music", "music": return folder
        default: return panel2
        }
    }

    /// The sticky pad — desktop/src/components/Note.jsx PADS. Same four slots
    /// in both looks, so a note keeps its colour across a switch.
    private static let pads: [Look: [String: (paper: Color, ink: Color)]] = [
        .bauhaus: [
            "yellow": (Color(hex: 0xF0B323), Color(hex: 0x17171C)),
            "red": (Color(hex: 0xD93B2B), Color(hex: 0xF2F1EE)),
            "paper": (Color(hex: 0xE8E6E1), Color(hex: 0x17171C)),
            "blue": (Color(hex: 0x2D51E0), Color(hex: 0xF2F1EE)),
        ],
        .glass: [
            "yellow": (Color(hex: 0xFBEAA0), Color(hex: 0x3D3617)),
            "red": (Color(hex: 0xFBC9D8), Color(hex: 0x43212C)),
            "paper": (Color(hex: 0xC4ECD3), Color(hex: 0x1F3A2A)),
            "blue": (Color(hex: 0xCBE4FB), Color(hex: 0x1D3245)),
        ],
    ]
    static var papers: [String: (paper: Color, ink: Color)] { pads[look] ?? pads[.bauhaus]! }
    static let paperOrder = ["yellow", "red", "paper", "blue"]
    /// Notes written before the palette changed still name their old stock.
    private static let legacy = ["pink": "red", "mint": "paper", "sky": "blue"]
    static func paper(_ item: Item) -> (paper: Color, ink: Color) {
        let c = item.meta?.color ?? "yellow"
        return papers[c] ?? papers[legacy[c] ?? ""] ?? papers["yellow"]!
    }

    @ViewBuilder static var backdrop: some View {
        if look == .glass {
            LinearGradient(colors: [Color(hex: 0x141A26), Color(hex: 0x1B2231)],
                           startPoint: .top, endPoint: .bottom).ignoresSafeArea()
        } else {
            ground.ignoresSafeArea()
        }
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255, blue: Double(hex & 0xff) / 255)
    }
    init(css: String) {
        var h = css.trimmingCharacters(in: .whitespaces)
        if h.hasPrefix("#") { h.removeFirst() }
        self.init(hex: UInt32(h, radix: 16) ?? 0x888888)
    }
}

/// A pane: an opaque ruled panel under Bauhaus, blurred glass under Glass.
struct Panel: ViewModifier {
    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
        return content
            .background {
                if Theme.look == .glass {
                    shape.fill(.ultraThinMaterial.opacity(0.6)).overlay(shape.fill(Theme.panel))
                } else {
                    shape.fill(Theme.panel)
                }
            }
            .overlay(shape.strokeBorder(Theme.rule, lineWidth: 1))
    }
}

extension View {
    func panel() -> some View { modifier(Panel()) }
}

/// Every heading and badge in the design is set the same way — tokens.css
/// --label-track and --label-case, in Swift.
struct Label3: View {
    let text: String
    var color: Color = Theme.text3
    init(_ text: String, color: Color = Theme.text3) { self.text = text; self.color = color }
    var body: some View {
        Text(Theme.labelUppercased ? text.uppercased() : text)
            .font(.system(size: Theme.labelUppercased ? 10 : 12, weight: .semibold))
            .tracking(Theme.labelTracking)
            .foregroundStyle(color)
    }
}

struct Badge: View {
    let text: String
    var body: some View {
        Text(Theme.labelUppercased ? text.uppercased() : text)
            .font(.system(size: 10, weight: .semibold))
            .tracking(Theme.labelTracking)
            .padding(.horizontal, 6).padding(.vertical, 2)
            .background(Theme.badgeFill, in: RoundedRectangle(cornerRadius: Theme.chipRadius))
            .foregroundStyle(Theme.text)
    }
}
