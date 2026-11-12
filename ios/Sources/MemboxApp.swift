import SwiftUI

@main
struct MemboxApp: App {
    @StateObject private var store = Store()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Root()
                .environmentObject(store)
                .id(store.settings.theme)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
        // Coming to the foreground is the moment to merge what the Mac wrote.
        .onChange(of: phase) { _, p in if p == .active { store.syncNow() } }
    }
}

/// The iPad gets the Mac's three panes; an iPad squeezed into a narrow Split
/// View (compact width) gets the phone's stack, which is what fits there.
private struct Root: View {
    @Environment(\.horizontalSizeClass) private var width

    var body: some View {
        if UIDevice.current.userInterfaceIdiom == .pad && width == .regular {
            DeskView()
        } else {
            LibraryView()
        }
    }
}
