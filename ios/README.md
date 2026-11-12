# membox iOS

SwiftUI over the shared Rust core (UniFFI). Same SQLite, same `dispatch`
action names as the desktop.

```sh
./bootstrap.sh   # builds ../target/MemboxCore.xcframework, vendors the Swift
                 # binding + brand assets, runs xcodegen
./build.sh       # build for the simulator, install, launch
```

- `Sources/Store.swift` — the core behind an `ObservableObject`; the change
  listener refreshes a whole-library snapshot. The library opens on a detached
  task, so the UI says "Opening your library…" until it lands.
- `Sources/CloudSync.swift` — pulls the other devices' iCloud snapshots
  (materialising placeholders first) and pushes this one.
- `ShareExtension/` — "Add to membox" in the share sheet, into the same
  library in the App Group container.

- `Sources/DeskView.swift` — the iPad at full width: the Mac's sidebar, grid
  and inspector. A narrow Split View falls back to the phone's stack.

`../bin/ios-release --upload` puts a build on TestFlight (iPhone and iPad);
`./deploy-device.sh` is the quick lane to a paired device.

The phone (and the iPad) is a viewer: it opens the core with no browser, so a link captured
there waits for the Mac to fetch, screenshot and file it.

Signing uses `DEVELOPMENT_TEAM` in `project.yml` — set it to your own Apple
team id before `bootstrap.sh`.
