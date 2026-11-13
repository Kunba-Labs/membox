# Membox review memory

- Desktop design tokens live in `desktop/src/tokens.css`. `ios/Sources/Theme.swift` mirrors their colours, shapes, and label styling. The two looks are Bauhaus (default) and Glass; changes to shared design values should be checked in both files.
- Desktop component styles live in `desktop/src/components/*.module.css`, with matching JSX components nearby. `Grid.module.css` defines tile, selection, and masonry behaviour; `Sidebar.module.css` defines sidebar rows and selected states.
- Desktop layout tokens set sidebar width to 224px and inspector width to 268px. The grid uses 12px outer padding, 10px column gaps, and round-robin item placement.
- The selected sidebar row uses `--row-current-bg` and `--row-current-fg`; its count uses `--star` in both looks. The selected grid tile uses a 2px `--accent` ring.
- iOS UI lives in `ios/Sources`. `LibraryView` is the phone collection stack, `GridView` renders tiles, `ItemDetailView` renders details, and `NoteView` edits notes. `Theme.swift` provides shared SwiftUI tokens and components such as `Panel`, `Label3`, and `Badge`.
- On regular-width iPad, `MemboxApp` presents `DeskView`; compact width uses `LibraryView`.
- iOS project settings are generated from `ios/project.yml` by XcodeGen. It targets iOS 17 and both iPhone and iPad; generated project and plist files are not the source of truth.
- The iOS `Store` exposes `items(in:query:)` and `count(_:)`. The former filters by collection and query; `count(_:)` uses an empty query. The iOS app opens the core without a browser, so captured links wait for the Mac to process them.
- Sync writes one JSON snapshot per device. It exports thumbnails to iCloud; full-page screenshots remain local. iOS `CloudSync` requests iCloud placeholders before the core imports snapshots.
