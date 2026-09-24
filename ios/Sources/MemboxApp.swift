import SwiftUI

@main
struct MemboxApp: App {
    @StateObject private var store = Store()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            LibraryView()
                .environmentObject(store)
                .id(store.settings.theme)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
        // Coming to the foreground is the moment to merge what the Mac wrote.
        .onChange(of: phase) { _, p in if p == .active { store.syncNow() } }
    }
}
