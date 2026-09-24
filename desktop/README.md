# membox desktop

Tauri v2 shell + React 19 / Vite / CSS Modules frontend.

Port **5210** is pinned with `strictPort`, so a collision fails loudly
instead of silently hopping to another port. `beforeDevCommand` reuses an
already-running Vite rather than starting a second one.

```sh
bin/dev                # Tauri window (reuses or starts Vite on :5210)
cd desktop && yarn dev  # frontend only, in a browser
bin/mock-thumbs        # regenerate the offline fixture thumbnails
```

## State

`src/store.js` is the library. In the Tauri app every write is
`invoke("dispatch", {action, args})` into the Rust core (`core/`), and the
state is a snapshot refreshed on the core's `library-changed` event; blob
paths become `asset:` URLs via `convertFileSrc`. In a plain browser
(`yarn dev`) the same action names run against an in-memory copy in
`localStorage`, seeded from `src/mock.js`, with a faked enrichment timer.
Components can't tell the two apart.

`src-tauri/src/browser.rs` is the embedded browser: a hidden second
`WebviewWindow` driven through its WKWebView (`evaluateJavaScript`,
`takeSnapshot`) via objc2. `main.rs` opens the core with it, and the core
writes `<data dir>/mcp.json` so any other MCP client can drive the same
browser and library.

`src/capture.js` is the pure paste logic (§1.5 type detection, §1.7
canonicalisation + dedupe). It has no DOM dependency so it runs under
`node --test src/capture.test.js`, and it ports to Rust line for line.

## What works

- **Capture** — ⌘V anywhere in the window (not in a field), or drop a
  link/image/file/text on it. All pasteboard flavours are kept; HTML markup is
  stored verbatim and rendered inert in the detail view. Re-pasting a known URL
  selects the existing item instead of duplicating it.
- **Library** — every sidebar entry filters: system collections, smart folders,
  folder groups (roll up their children) and folders. Counts are live.
  All Tags is a cloud; clicking a tag searches for it.
- **Folders** — `+` creates, right-click renames/deletes/adds a subfolder,
  drag tiles onto a folder to file them. The footer field filters the tree.
- **Search** — substring over title/tags/domain/summary, plus `kind:` `tag:`
  `domain:` `rating:4` `is:untagged` prefixes; Esc clears. Sort menu in the
  toolbar.
- **Selection** — click, ⌘-click, ⇧-click; ⌘A; drag a multi-selection.
- **Inspector** — title and notes commit on blur; stars click; tag chips add
  (`+` → type → Enter) and remove; folder chips add (`+` → pick) and remove;
  Re-enrich and Trash buttons.
- **Context menu** — open, open/copy source, copy original markup, re-enrich,
  trash / put back / delete permanently.
- **Detail view** — double-click or Enter; Esc closes.
- **Trash** — soft delete, put back, empty.

Enrichment in the Tauri app is real (spec §3): fetch in the embedded
browser or via yt-dlp, then the agent lane chosen in Settings. In the
browser fallback it is faked with a timer.

## Keys

| | |
|---|---|
| ⌘V | capture the pasteboard |
| ← → ↑ ↓ | move focus in the grid (⇧ extends) |
| Enter / Space | open detail |
| 0–5 | rate the selection |
| ⌫ | trash the selection (delete permanently when in Trash) |
| ⌘A / Esc | select all / clear |
| ⌘F | focus search |
| ⌘I | toggle inspector |

Design tokens live in `src/tokens.css` (spec §6.9) and are the single source
of truth; the iOS app mirrors them as Swift constants.

The window is transparent with the macOS `hudWindow` vibrancy material behind
it, so the glass in the UI sits on real material rather than a painted
approximation (§6.1).
