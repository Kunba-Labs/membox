# membox

A local-first visual memory box for macOS (and a companion iPhone viewer).
Paste a link, a video, a reel, a list of books, some styled text — membox
opens it in its own embedded browser, screenshots it, hands it to the coding
agent you already have installed to tag and file, and lets you find it again
by searching or asking.

Think [Eagle](https://eagle.cool), but you never file anything yourself.

![membox in the default Bauhaus look: a grid of saved pages, a video and a repo, with the inspector showing tags, folder and summary](docs/screenshots/bauhaus.jpg)

<details>
<summary>The same library in the Glass look</summary>

![membox in the Glass look](docs/screenshots/glass.jpg)
</details>

- **Capture is manual.** ⌘V, drag and drop, the iOS share sheet, or MCP. No clipboard watching.
- **Everything stays on your Mac.** SQLite + FTS5 in your Library folder. No membox server, account or telemetry.
- **Bring your own agent.** Claude Code, Codex, Gemini CLI, OpenCode or a local Ollama model — or none.
- **Your edits win.** Anything you changed by hand is never overwritten by an agent.

## Install

Download the `.dmg` from the [latest release](../../releases/latest) and drag
membox to Applications. Releases are Developer ID signed and notarized, and
every push to `main` is one: the app checks for a new version every half hour
and offers **Update** in the sidebar.

Optional, for richer captures:

```sh
brew install yt-dlp        # YouTube / Instagram posters, metadata, transcripts
```

…and any one of the agent CLIs in [Agent lanes](#agent-lanes), signed in.

## Build from source

Requires macOS 13+, Rust (stable), Node 22 + Yarn 1, and Xcode for iOS.

```sh
cd desktop && yarn install
bin/dev                          # "membox Dev" window (starts Vite on :5210)
cd desktop && yarn dev           # UI only, in a browser, over a localStorage stand-in
cd desktop && yarn install:app   # build, sign with your keychain identity, install to /Applications
cargo test -p membox-core        # core tests
cd desktop && node --test src/capture.test.js
ios/bootstrap.sh && ios/build.sh # xcframework + Xcode project → simulator (see ios/README.md)
```

`yarn install:app` signs with a Developer ID or Apple Development identity
from your keychain; `APPLE_SIGNING_IDENTITY=-` signs ad-hoc.

### Layout

```
core/      Rust — SQLite + FTS5 store, capture, enrichment queue, agent adapters,
           loopback MCP server, iCloud sync. Shared by desktop and iOS.
desktop/   Tauri v2 + React 19. A hidden second window is the embedded browser.
ios/       SwiftUI over the core via UniFFI, share extension.
brand/     Icon master + bin/icons → every derived set.
bin/       dev · backup · restore · icons · mock-thumbs · version · release-secrets
docs/      Feature-Spec.md — the full spec.
```

### Two identities

| | identity | library |
|---|---|---|
| **membox** | `com.membox.desktop` | `~/Library/Application Support/com.membox.desktop/` |
| **membox Dev** | `com.membox.desktop.dev` (golden DEV badge) | `…/com.membox.desktop.dev/` |

Separate Dock icons, libraries, MCP endpoints and iCloud folders (`membox` vs
`membox-dev`); both can run at once, so you can break the Dev one freely.
`bin/dev` and `yarn install:app:dev` share the Dev library.

Each library folder holds `membox.db` (SQLite, WAL), `blobs/`
(content-addressed screenshots), `scratch/<item>/` (what each agent run saw
and wrote), `membox.log` and `mcp.json`. `bin/backup` / `bin/restore`
snapshot and put back a library.

### Releases

Push a `v*` tag and `.github/workflows/release.yml` builds a universal
`.dmg` into a draft GitHub release. It is signed and notarized when the
repository has these secrets, ad-hoc signed when it doesn't:
`APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific), `APPLE_TEAM_ID`.

## How a paste becomes a filed item

Paste anything. A link, a list of links (each becomes its own item), a
bare `youtube.com/watch?v=…`, a share dialog's embed HTML, an image, a
PDF or any file, a block of code, a note. The core decides what each thing
is; you never say.

1. **Capture** (`core/src/capture.rs`) — every URL in the paste, canonical
   form, dedupe key, kind (video · music · reel · repo · post · image · pdf ·
   page · code · text). Every pasteboard flavour is kept; HTML markup
   verbatim; the prose around a single link becomes your note on it.
2. **Fetch** (`core/src/enrich.rs`) — no model. YouTube/Instagram go through
   `yt-dlp` (poster, metadata, transcript). Everything else loads in the
   embedded WKWebView: readable text, viewport + full-page PNG.
   Then, with no model at all (`core/src/autotag.rs`): tags from the kind,
   the site, the channel and the page's own keywords, and a rule files it
   (videos → Watching, music → Listening, repos → Build › Code, pages →
   Reading). Machine tags are marked — dashed, ✓ to keep — until you make
   them yours.
3. **Agent** (`core/src/agent.rs`) — a scratch dir with `input.json`,
   `readable.md`, `screenshot.png`, a per-agent MCP config pointing at
   membox's loopback server, and `prompt.md`. The CLI runs unattended with a
   hard timeout and writes `result.json`. Existing folder above the confidence
   threshold → filed; a suggested new folder → appears as *suggested* in the
   sidebar until you accept it. Anything you edited by hand is never
   overwritten.
4. **MCP** (`core/src/mcp.rs`) — `navigate`, `screenshot`, `extract_readable`,
   `extract_links`, `click`, `scroll`, `search`, `get_item`, `set_item`,
   `create_item`, `create_folder`. Loopback only, bearer token per process,
   origin/host checks, every call audited, writes scoped to the item under
   enrichment.

## iCloud sync

Settings › *Keep this library in sync across my devices* (desktop) or the
⋯ menu (iOS). The SQLite file never leaves the machine — file-level sync of
a live database corrupts it. Instead each device writes its own
`devices/<device-id>.json` snapshot into the iCloud folder and merges the
others', last-writer-wins per row on `updated_at`, deletions as tombstones.
No file is ever written by two devices, so iCloud has nothing to conflict
on. Items, folders, tags and thumbnails travel; full-page shots, transcripts'
scratch dirs and agent runs stay local.

Folder: the app's iCloud container (`iCloud.com.membox`, shown as "membox"
in iCloud Drive) when the signed iOS app has created it, else
`~/Library/Mobile Documents/com~apple~CloudDocs/membox` on the Mac. Export
runs ~1 s after any change, import on open, on app focus (iOS) and every
minute. `core/src/sync.rs`.

## Agent lanes

Chosen in Settings, shown with what leaves the machine:

| lane | binary | sends content to |
|---|---|---|
| `claude` | Claude Code | Anthropic |
| `codex` | Codex | OpenAI |
| `gemini` | Gemini CLI | Google |
| `opencode` | OpenCode | its configured provider |
| `local` | Ollama | nothing — stays on this machine |
| `off` | — | nothing; capture + screenshot only |

## Privacy

There is no membox server, no account and no telemetry. What leaves your
machine is exactly: the pages you paste (fetched by the embedded browser),
the item you are filing (sent to the agent lane you picked — none, if it's
Ollama or off), and your iCloud Drive if you turn sync on.

## License

[GPL-3.0-or-later](LICENSE). Use it, change it, ship it — as long as what you
ship is open source too. `core/vendor/autoconsent` is DuckDuckGo's
autoconsent, MPL-2.0, unmodified.
