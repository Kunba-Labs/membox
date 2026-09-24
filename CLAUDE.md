# membox

Local-first visual memory box (Eagle-style). Paste anything → it is fetched in
membox's own embedded browser, tagged, filed, and searchable. Spec:
`docs/Feature-Spec.md` (no phases — everything is v1). Sibling project
`~/Development/inbox2` is the pattern source (AgentAdapter, loopback MCP,
UniFFI iOS, dev/prod identities, local installer) — reuse before inventing.

## Layout

```
core/            Rust crate shared by desktop + iOS: SQLite+FTS5 store (db.rs), capture
                 (capture.rs), enrichment queue (enrich.rs), no-model tags/rules (autotag.rs),
                 yt-dlp (ytdlp.rs), agent adapters (agent.rs), loopback MCP (mcp.rs),
                 iCloud sync (sync.rs), UniFFI surface (ffi.rs). ONE write entry point:
                 Library::dispatch(action, json) — same action names in JS and Swift.
desktop/         Tauri v2 + React 19 + Vite + CSS Modules. src/store.js talks to the core
                 via invoke("dispatch") and keeps a localStorage fallback for plain-browser dev.
                 src-tauri/src/browser.rs = hidden WebviewWindow driven through its WKWebView (objc2).
ios/             SwiftUI over the core (UniFFI xcframework), share extension, WKWebView HostBrowser.
brand/           icon-master.png → bin/icons regenerates every derived set (mac/ios/tauri/tauri-dev).
```

## Commands

```sh
bin/dev                              # membox Dev (com.membox.desktop.dev) — the play identity
cd desktop && yarn install:app       # build, sign (Developer ID), install /Applications/membox.app — daily
cd desktop && yarn install:app:dev   # same for "membox Dev" (golden DEV badge)
cargo test -p membox-core            # core tests (the ones that matter)
cd desktop && node --test src/capture.test.js
ios/bootstrap.sh && ios/build.sh     # xcframework + xcodegen; simulator "iPhone 17 Pro"
ios/deploy-device.sh [--xcframework] # signed build → paired iPhone over the air (team KF53B4S5HD)
bin/backup [--dev]                    # snapshot library → membox-backups/<id>/<stamp>/, keep 10
bin/restore [--dev] [dir]            # put one back (app must be quit; old library kept aside)
bin/icons · bin/mock-thumbs
```

Data: `~/Library/Application Support/com.membox.desktop[.dev]/` — `membox.db`,
`blobs/`, `scratch/<item>/` (what each agent run saw/wrote), `mcp.json`
(loopback MCP url + bearer token for outside agents).

## Decisions that must hold

- **Capture is manual.** ⌘V / drop / Share / MCP `capture`. No clipboard watching, ever.
- **A bare `Cargo.site` is a guess, not text.** Two-label tokens with a plausible
  TLD are opened in the browser (`meta.guess`); one that won't load is demoted
  back to a snippet by enrich. `capture.rs` / `notes.md` never qualify.
- **A paste is read before it is written** (§1.4 `triage.rs`): rules split lines
  into entries and keep the words sharing a line with a link as that entry's
  note; only a line that is a *name* (product, book, film) spends an agent call.
  The paste sheet then asks one question — "anything about these?" — while the
  fetching runs. The answer lands in `notes` and outranks the page for the agent.
- **A Hacker News or Reddit link is two saves in one**: the story's own page (screenshot,
  title, summary) with the thread beside it — `meta.hn` for the discussion link,
  points and comment count, top comments in `transcript` so search reads them.
  A text post (Launch HN, a self post) lends the first link inside it. Reddit's
  JSON is shut to anonymous curl and open to a real browser — so ours fetches it.
- **Books, films, series and games are things, not pages**: named ones resolve a
  cover and a synopsis keylessly — Open Library for books, Steam's store API for
  games (600×900 box art), Wikipedia for the rest — and an agent may supply the
  canonical url for a product it recognises.
- **Agent lanes**: Claude Code / Codex / Gemini / OpenCode / Ollama, chosen in Settings with "sends to cloud" / "stays local" stated. "Local-first" = no membox server, account or telemetry — not zero vendor egress.
- **The browser is ours.** Agents drive membox's hidden WKWebView over MCP; never spawn Chrome. MCP writes are scoped to the item under enrichment; `capture` is the one unscoped tool.
- **User edits win.** `user_edited` rows are never overwritten by an agent. Machine tags live in `auto_tags`; typing or re-adding a tag makes it the person's.
- **Suggested folders are proposals** (`proposed=1`) until filed into — and a
  proposal nobody took up disappears: `prune_proposed()` drops an empty one on
  every membership change. Dropping an item into a folder while you are looking
  at another *moves* it, which is what empties them. The agent sees the existing
  suggestions in its prompt, so it reuses "Want to buy" instead of inventing
  "Shopping" beside it.
- **Sync never moves the SQLite file.** Per-device `devices/<id>.json` snapshots in iCloud Drive, LWW on `updated_at`, tombstones in `deleted_at`. Seed folder ids are deterministic (`f-seed-…`) so trees merge.
- **Two identities** like inbox2: `membox` (daily) and `membox Dev` (play) — separate libraries, MCP endpoints, iCloud folders (`membox` / `membox-dev`).
- **Two looks, one token file** (`settings.theme`, synced): **Bauhaus** is the
  default and **Glass** is the original design, kept. The difference is shape
  and softness as much as colour, so radius, blur, shadow, rule width and how a
  label is set are tokens too — a component asks for `--r-tile`, only
  `desktop/src/tokens.css` knows it is 0. `ios/Sources/Theme.swift` mirrors both
  (`Theme.look`, and the root view is keyed on it so a switch redraws).
- **Icon is flat**: isometric cube, no glow, no contents — and so is Bauhaus:
  Bauhaus, ink panels on one flat ground ruled apart in `--rule`, three
  unmixed primaries (`--accent` selects, `--folder` files, `--star` marks) used
  at full strength or not at all. No gradient, blur, shadow or radius anywhere,
  which is why there are no glass or radius tokens left in
  `desktop/src/tokens.css`; `ios/Sources/Theme.swift` mirrors it.

- **A sticky is an item** (`kind: "note"`), not a second kind of record: it gets
  search, tags, folders, trash and sync for free. The rich text lives in
  `body_html`, its plain text in `body_text` so FTS reads it, and the items it
  points at are read back out of the markup (`data-item="i-…"` → `meta.links`)
  rather than tracked beside it. ⌘⇧N, or "New note" in the paste sheet. Stickies
  land in the seed folder `f-seed-notes` (📝 Notes), added to an older library
  once at startup — never re-added, tombstone respected, so deleting it sticks.

## Gotchas (each cost real time)

- parking_lot `Mutex`: `if let Some(x) = self.db.lock().get()… { self.db.lock()… }` deadlocks — temporaries live for the whole block. Bind the result first.
- yt-dlp: `--dump-single-json` implies `--simulate` (no subs written) → `--no-simulate`; `--quiet` turns the JSON into `null`; `--sub-langs en.*` requests every auto-translation and gets 429'd → `en-orig,en-en,en`; parse the `{` line from stdout regardless of exit code.
- WebKit only composites views in a *visible* window, and pauses rAF/WebGL in one it considers hidden: hidden windows capture DOM chrome but empty canvases; alpha-0/offscreen windows on iOS snapshot black. iOS: the WKWebView sits at index 0 of the key window. macOS: the browser window is on screen at the bottom of the stack with NSWindow alpha 0, ignoring the mouse. Full-page: resize, sweep the page, settle, snapshot with `afterScreenUpdates`.
- `dispatch_sync` onto main from main (iOS) hangs and the app is killed at launch.
- iCloud gives a phone *placeholders*: the Mac's `devices/d-….json` arrives as a
  zero-byte `.d-….json.icloud` until something asks for it, so the core's
  `read_dir` sees no `.json` and imports nothing. `CloudSync.pull` calls
  `startDownloadingUbiquitousItem` and waits before every sync; thumbs are
  materialised after, in the background.
- **The phone is a viewer** (§7.3): `MemboxCore.open(browser: nil)`. It had an
  offscreen WKWebView; building it in `Store.init` trapped —
  `EXC_BREAKPOINT __DISPATCH_WAIT_FOR_QUEUE__` — because a `@MainActor`
  initialiser can run on a cooperative thread that already owns the main queue
  while `Thread.isMainThread` is false, so the "safe" hop was a `dispatch_sync`
  onto the current queue. Gone entirely; capture on the phone waits for the Mac.
- Opening the library is not main-thread work — it seeds, prunes and merges the
  other devices' snapshots. `Store.init` opens it on a detached task and the UI
  says "Opening your library…" until it lands.
- Tauri's `dragDropEnabled` (default true) swallows HTML5 drag events inside the
  webview: dragging a tile onto a folder never fired, and a file from Finder
  reached neither the JS handler nor any listener. It is `false` in
  tauri.conf.json — WebKit does its own drops. `tauri.conf.json` takes no
  comments, so this is the note.
- Cookie banners are clicked away before the shot by DuckDuckGo's autoconsent
  (`core/vendor/autoconsent`, MPL-2.0, unmodified; `browser.rs` is its host).
  It is a start-of-document script *in every frame* — a Sourcepoint banner is
  a cross-origin iframe the top frame cannot see into. A page it misses is a
  stale upstream rule (nu.nl's DPG gate, 2026-09): refresh the two vendored
  files, don't hand-write selectors.
- A new app icon does not reach the Dock: it keeps its own tile cache,
  `$(getconf DARWIN_USER_CACHE_DIR)/com.apple.dock.iconcache`, that survives
  `killall Dock`, an iconservices wipe and `lsregister -f` (Finder shows the
  new icon all along). `rm` that file, then `killall Dock`.
- Tauri `generate_context!` rejects non-RGBA icon PNGs. Extension bundle ids must nest under the app id or the simulator refuses the install.
- `app.css` loads after CSS Modules; a global `.glass` rule can override a module's background.
- Writing JSON files from Python heredocs: `"\\n"` inside a quoted heredoc is a literal backslash-n.

## Verifying

`~/Library/Application Support/com.membox.desktop[.dev]/membox.log` is the trail:
every fetch and agent run with timings, one rotation at 8MB (`membox.log.1`).
Settings › Library shows the path. Two queues: fetching is serial (one browser,
one page, `browser_lock`), agents run `settings.agent_concurrency` at a time
(default 3, Settings › Agent). Every run gets its own MCP token — `mcp.runs`
maps token → the one item it may write — because a single global scope would
let one item's agent write another's.

Drive the app without the GUI: read `mcp.json`, `curl` the MCP (`capture`, `navigate`,
`screenshot`, `get_item`), then inspect `membox.db` with `sqlite3`. For a run that must
not touch the daily library, launch `target/debug/membox` with `HOME=<scratch>`.
