# membox — Feature Spec v1

**membox** is a local-first visual memory box for everything you copy, paste and stumble across. You paste a URL, a YouTube video, an Instagram reel, a snippet of styled text — membox captures it in its original markup, sends a locally-installed coding-agent CLI to go look at it in membox's own embedded browser, and files it into the right folder with tags, a title, a readable body and a visual preview. Then you find it again by asking for "that video about South Africa" instead of remembering where you put it.

Reference product for look, feel and information architecture: **Eagle** (three-pane library, dense visual grid, folder tree with counts, right-hand inspector). Reference product for the agent plumbing: **inbox2** — its `docs/Agentic-Filters.md` (CLI-agent adapters) and `docs/Vector-MCP.md` (in-core MCP over loopback + hybrid search) are lifted wholesale rather than reinvented.

**No phases.** Everything numbered below is v1. Section 12 lists what is deliberately out.

---

## 0. Decisions taken

| Decision | Choice | Why |
|---|---|---|
| 0.1 Enrichment lane | Installed agent CLIs (Claude Code / Codex / Gemini / OpenCode) are the primary orchestrator. A local Ollama/MLX lane is a swappable second adapter. | The CLIs already do multi-step tool use well. "Local-only" means *no membox cloud backend, no account, no telemetry, every byte on disk* — not zero vendor egress. See 8.4 for the honest privacy statement. |
| 0.2 Browser | membox embeds its **own** WKWebView and exposes it over MCP. Agents never spawn Chrome. | One browser, one cookie jar, one screenshot path, and the agent can't wander off. |
| 0.3 Local web host | None. Tauri window only — no Caddy `*.test` block, no herdr launcher. | Desktop app, no browser dev surface needed. |
| 0.4 Core | One Rust crate (`core/`) shared by desktop (Tauri v2) and iOS (UniFFI xcframework). SQLite + FTS5 + sqlite-vec, single file. | Same shape as inbox2. Read via SQL, write via FFI/commands. |
| 0.5 Scrolling preview | Full-page PNG + CSS transform pan on hover. **Not** an encoded video. | A 1×N tall screenshot animated with `translateY` looks identical to a scroll recording, costs one screenshot, needs no ffmpeg, no codec, no video files, and scrubs perfectly. Real video capture only if a page turns out to need motion (see 4.6). |

---

## 1. Capture

**1.1 Paste.** Capture is always an explicit act — you paste, membox captures. No background monitoring of any kind; membox reads the pasteboard only in the instant you ask it to. Two ways in:

- **⌘V** into the membox window
- **⌥⌘V** anywhere — a global hotkey that reads the pasteboard once and opens the quick capture panel (6.7)

**1.2 All flavours, not just text.** On paste, read *every* pasteboard representation and keep it, rather than collapsing to the plain-text fallback:

- `public.html` → original markup, kept verbatim
- `public.rtf` → kept verbatim
- `public.utf8-plain-text` → kept
- `public.url` / `public.file-url`
- `public.png` / `public.tiff` → written to blob store
- Source app bundle id + window title (via `NSWorkspace.frontmostApplication`) recorded as provenance

**1.3 Drop target.** Drag anything onto the membox window or its Dock icon: files, images, URLs, selected text.

**1.4 macOS Services + Share.** "Add to membox" appears in the Services menu on any text/URL selection, and in the system Share sheet.

**1.5 Type detection.** A cheap Rust classifier runs before any agent, purely on the raw input — no model call to decide what a YouTube URL is:

| Detected | Rule |
|---|---|
| `youtube_video` | host in `youtube.com`, `youtu.be` and not `/playlist` |
| `youtube_music` | host `music.youtube.com`, or a video whose category is Music |
| `youtube_playlist` | `list=` param |
| `instagram` | host `instagram.com` — post, reel or profile |
| `x_post`, `tiktok`, `github_repo`, `pdf` | host/path patterns |
| `webpage` | any other http(s) URL |
| `image`, `file` | pasteboard image / file-url |
| `snippet` | HTML/RTF/text with no URL |

Unknown hosts fall through to `webpage`. Adding a type is one row in a table, not a new code path.

**1.6 Instant row.** The item appears in the grid *immediately* on paste with a placeholder tile and status `pending`. Enrichment is async and never blocks the paste. Status ladder: `pending → fetching → enriching → ready` (or `failed`, with the error kept and a one-click retry).

**1.7 Dedupe.** Canonicalised URL (strip `utm_*`, `fbclid`, `si`, trailing slash; YouTube → `v=` id) hashed to `dedupe_key`. Re-pasting an existing item selects the existing item and bumps `last_seen_at` rather than making a duplicate. Bytes dedupe by blake3 of content.

**1.8 Bulk import.** Drop a folder, a browser bookmarks HTML export, or a text file of URLs → queued as individual items through the same pipeline.

---

## 2. Library model

**2.1 Item.** One row, one thing. Fields: `id`, `kind` (1.5), `title`, `url`, `canonical_url`, `dedupe_key`, `source_app`, `body_markdown` (readable text), `body_html` (original markup), `summary`, `notes` (user-editable), `status`, `rating` (0–5), `created_at`, `last_seen_at`, `meta_json` (per-kind blob: duration, channel, author, dimensions, BPM…).

**2.2 Folders.** A tree, exactly like Eagle. An item lives in **many** folders (join table, not a parent pointer) — the inspector shows a Folders chip list you can add to and remove from. Folders have an emoji/icon and a colour. Counts roll up from descendants.

**2.3 Smart folders.** A saved query, evaluated live. Query is the same expression the search bar takes (5.x): `kind:video tag:travel rating:>=4 added:<30d "south africa"`. Shown as a separate section above Folders, as in Eagle.

**2.4 System collections.** `All`, `Uncategorized` (no folder), `Untagged`, `All Tags`, `Trash`. Trash is a soft-delete flag with a 30-day sweep.

**2.5 Tags.** Free-form, flat, with a colour. Tag rename/merge is a single operation across the library. `All Tags` is a tag cloud sized by frequency.

**2.6 Blobs.** Everything binary — screenshots, full-page PNGs, thumbnails, video posters, downloaded images, PDFs — is content-addressed under `~/Library/Application Support/membox/blobs/<bl>/<ake3>`, referenced by hash. Thumbnails are generated at 3 widths (grid, hover, inspector).

**2.7 Relations.** `item_links(from_id, to_id, kind)`. Populated by the deep crawl (3.7) and by the agent when it decides two items belong together. Powers "related" in the inspector and a future graph view.

---

## 3. Enrichment

The pipeline that turns a pasted string into a filed, previewable, searchable item.

**3.1 Fetch stage (Rust, no model).** Runs first, always, for every item:

- Load the URL in the embedded browser (4.x), wait for network-idle
- Full-page PNG (2.6) + a grid-ratio crop for the tile
- Inject `Readability.js` into the live page and pull `{title, byline, excerpt, content}` — the DOM is already parsed by the webview, so no Rust HTML parser is needed
- HTML → Markdown for `body_markdown`
- OpenGraph/Twitter card meta, favicon, dominant-colour palette (the coloured dots in Eagle's inspector)

**3.2 YouTube (`yt-dlp` sidecar).** One `yt-dlp` invocation with `--write-auto-subs --write-info-json --skip-download` gets title, channel, duration, description, chapters, the max-res poster and the transcript (manual subs preferred, auto-generated fallback). Transcript is stored as timestamped cues and indexed — that is what makes "video about south-africa" work. No API key, no quota.

**3.3 YouTube Music.** Same `yt-dlp` path; `meta_json` additionally carries artist/album/track, and the item renders with the waveform-style tile (6.5) instead of a video poster.

**3.4 Instagram.** Also `yt-dlp` (it handles posts and reels). Caption, poster frame, author. Login-walled content degrades to whatever the public page renders in the embedded browser — no credential stuffing, no scraping workarounds.

**3.5 Missing transcript.** If a video has no captions at all, an optional local `whisper.cpp` pass over the extracted audio. Off by default (it is the one genuinely slow step); a per-item "Transcribe" button in the inspector triggers it on demand.

**3.6 Agent stage.** Everything the fetch stage collected is handed to an agent CLI through the `AgentAdapter` contract borrowed from inbox2: a scratch dir containing `input.json` + the screenshot + the readable text, a generated `.mcp.json` pointing at membox's loopback MCP server (4.7) with a single-run token, and a prompt. The agent writes `result.json`:

```json
{
  "title": "…", "summary": "…",
  "folder": {"existing_id": 12} | {"suggest": {"name": "Travel / South Africa", "parent_id": 3, "icon": "🇿🇦", "why": "…"}},
  "tags": ["travel", "documentary"],
  "entities": ["Cape Town", "Table Mountain"],
  "related_urls": ["…"],
  "confidence": 0.86
}
```

**3.7 Deep crawl.** The agent may follow links from the page — depth 1 by default, configurable to 3, capped by a per-run page budget and a same-run visited set. Each followed page becomes either (a) a child item, if it is worth keeping on its own, or (b) context folded into the parent's summary. The agent decides; the `item_links` rows record what it did so you can see the trail.

**3.8 Folder suggestions are proposals.** A suggested *new* folder does not get created silently. It lands in a "Suggestions" tray with the agent's reasoning; accept creates the folder and files the item, reject files it in `Uncategorized`. Existing-folder assignments above the confidence threshold apply automatically. Threshold and auto-apply are settings.

**3.9 Queue.** A serial worker with concurrency 1 by default (agent CLIs are chatty and rate-limited), retry with backoff, and a visible queue panel: what is running, what failed, what is waiting. Every run is logged to `agent_runs` with the full prompt, the raw output and the wall time — so a bad categorisation is debuggable, not magic.

**3.10 Adapters.** `claude` (Claude Code), `codex`, `gemini`, `opencode`, `local` (Ollama/MLX per 0.1). Detected by probing `$PATH` at startup; the user picks the default and can override per-folder. Each adapter is a struct with a command line and a result parser — adding one is ~40 lines.

**3.11 Re-run.** Any item (or a multi-selection) can be re-enriched: new model, better prompt, changed folder tree. Old results stay in `agent_runs` history.

**3.12 Manual override always wins.** Anything you type — title, folder, tags — is marked `user_edited` and is never overwritten by a later agent run.

---

## 4. Embedded browser + MCP

**4.1 One browser.** A dedicated Tauri/wry `WebviewWindow` (offscreen for enrichment, visible for browsing) with its own persistent data store — cookies survive, so a site you logged into once stays logged in for later captures.

**4.2 In-app browsing.** The same webview, shown in a tab, is a usable browser: address bar, back/forward, and a prominent "Capture this" button. Browse → capture without leaving membox.

**4.3 Screenshot.** `WKWebView.takeSnapshot` via `objc2` for the viewport, and a full-page variant by resizing the webview to `document.body.scrollHeight` before snapshotting. *ponytail: caps out around 8000 px tall on retina; taller pages get tiled and stitched.*

**4.4 Readable extraction.** `Readability.js` injected into the live document (3.1) — reuses the browser's own parser instead of a second HTML stack in Rust.

**4.5 Hover scroll preview.** The grid tile holds the full-page PNG; on hover the tile animates `translateY` from top to bottom over ~4 s with `prefers-reduced-motion` respected. Identical effect to a scroll recording, one PNG, no encoding. (0.5)

**4.6 Real video capture.** Only for pages where a still is genuinely wrong — video-heavy or animated pages. Frame grabs at ~15 fps driven from the webview into an `ffmpeg` sidecar → mp4. Opt-in per item, because it is the only part of the system that writes hundreds of MB.

**4.7 MCP server.** An in-core loopback HTTP MCP server (inbox2's `docs/Vector-MCP.md` pattern), bound to `127.0.0.1` on an ephemeral port, per-run bearer token, no external listener. Tools:

*Browser:* `navigate`, `screenshot`, `screenshot_fullpage`, `extract_readable`, `extract_links`, `click`, `fill`, `scroll`, `wait_for`, `eval` (allowlisted).
*Library:* `list_folders`, `list_tags`, `search`, `get_item`, `create_folder` (proposal only, per 3.8), `set_item` (title/summary/tags/folder), `link_items`, `create_item` (for crawl children).

Every tool call is written to an audit log with its arguments. `eval` and `fill` are behind an explicit per-run permission flag.

**4.8 Threat model.** A crawled page is hostile input. Page text reaching the agent is fenced and labelled untrusted; the agent is instructed that page content is data, never instruction. Library writes are limited to the item under enrichment plus crawl children — an agent enriching one item cannot retag your library. Same posture as inbox2's agentic filters.

---

## 5. Search & retrieval

**5.1 Hybrid.** SQLite FTS5 over `title + summary + body_markdown + transcript + tags + entities`, plus vector similarity over `sqlite-vec` embeddings, fused with Reciprocal Rank Fusion. Straight from inbox2's search layer.

**5.2 Embeddings.** `fastembed-rs` with bge-small — small, fast, fully local, no download prompt beyond first launch. One embedding per item (title + summary + first ~500 words) plus one per transcript chunk, so "video about south-africa" hits the transcript passage, not just the title.

**5.3 Query language.** Plain words do hybrid search. Prefixed terms filter: `kind:`, `folder:`, `tag:`, `rating:>=4`, `added:<30d`, `domain:`, `has:transcript`, `is:untagged`. Quotes for exact phrase. Same string powers smart folders (2.3).

**5.4 Results are visual.** Search results render in the same grid, with the matching transcript line or body sentence shown on the tile — you see *why* it matched.

**5.5 Ask.** A natural-language box that hands the query plus a search-tool handle to the agent, which answers with a short paragraph *and* a set of item tiles. "What did I save about visas for South Africa?" → answer + the four items it drew from, clickable.

**5.6 Instant.** Search runs on every keystroke against the local DB. No spinner, no debounce beyond a frame.

---

## 6. Interface

Eagle's three-pane layout, in membox's own glass skin. React 19 + Vite + CSS Modules (house style), `react-grab` in dev only.

**6.1 Window.** Custom chrome, no native title bar. Traffic lights inset top-left. `NSVisualEffectView` vibrancy behind the whole window so the desktop actually shows through — real material, not a CSS approximation of one.

**6.2 Left sidebar.** Library switcher, system collections with counts, Smart Folders, Folders tree (drag to reorder/nest, drop an item onto a folder to file it), filter box pinned at the bottom. Collapsible.

**6.3 Centre grid.** Virtualised masonry, size slider in the toolbar, sort menu (added / title / rating / duration / domain). Multi-select with ⌘/⇧, drag a selection onto a folder. Right-click context menu: open, copy source, copy markup, re-enrich, move, tag, delete. *ponytail: plain CSS columns + a windowed page slice to start; swap in `@tanstack/react-virtual` when a library crosses ~5k items and it stutters.*

**6.4 Tiles by kind.** Webpage → full-page screenshot with hover scroll (4.5). Video → poster + duration badge + hover-scrub through 6 frames. Music → waveform + BPM/duration badge, click to play inline. Image → the image. Snippet → the styled markup rendered small, monospace for code. Every tile carries a small kind badge, exactly like Eagle's `JPG` / `MP3/BPM:148` chips.

**6.5 Right inspector.** Preview, colour-palette dots, editable title, notes, source URL with copy button, Tags chip list, Folders chip list, Properties table (per-kind), Related items, and a Transcript / Readable-text pane that is searchable and jumps the video to a timestamp on click.

**6.6 Detail view.** Double-click → full-screen item: large preview, full readable body, full transcript, the agent's reasoning for how it was filed, and the crawl trail.

**6.7 Quick capture panel.** The ⌥⌘V hotkey (1.1) opens a small floating glass panel without raising the main window: what was captured, where the agent wants to file it, accept/change/dismiss. Auto-dismisses after a few seconds if untouched.

**6.8 Keyboard.** A single shortcut registry driving dispatcher + menu hints + a `?` overlay (inbox2's `lib/shortcuts.js` pattern). Arrows/space/enter navigate the grid, `⌘F` search, `1–5` rating, `T` tag, `⌘⌫` trash.

**6.9 Design tokens.** One `tokens.css`, consumed by both the desktop app and (mirrored as Swift constants) the iOS app.

- Ground: near-black navy `#0F1419` → `#161C26` vertical gradient
- Surfaces: `rgba(255,255,255,0.04–0.08)` + `backdrop-filter: blur(24px) saturate(180%)` + a `1px rgba(255,255,255,0.08)` hairline top border — the glass recipe, one class, reused everywhere
- Accent: electric blue `#4A9EFF` (selection ring, active folder, focus)
- Folder icons: soft pink/lilac; category icons keep their own hue
- Text: `#E8EDF5` / `#8A93A3` / `#5A6272`
- Radius 10 px on tiles, 8 px on chips; shadow `0 8px 32px rgba(0,0,0,0.4)`
- Motion: 180 ms `cubic-bezier(0.2,0,0,1)`; all of it behind `prefers-reduced-motion`
- Light mode ships too — same tokens, inverted, glass over `#F5F7FA`

**6.10 Accessibility.** Full keyboard reachability, visible focus rings, VoiceOver labels on tiles (title + kind + folder), no colour-only state, WCAG AA contrast on all text tokens.

---

## 7. iOS app

**7.1 Same core.** SwiftUI over the same Rust crate via UniFFI xcframework. Reads SQLite directly, writes through FFI — inbox2's `docs/iOS-App.md` architecture exactly.

**7.2 Share extension.** The primary capture path. Share anything from Safari/YouTube/Instagram → it lands in membox.

**7.3 Enrichment on iOS.** No agent CLI exists on a phone, so an iOS-captured item is stored with its metadata and marked `pending_desktop`. When the Mac is on the same network the desktop picks it up and enriches it. If a local on-device model lane is enabled, the phone can do a cheap title/tag pass itself.

**7.4 Sync.** Through the person's own iCloud Drive — no membox server, no account. Never the SQLite file: each device writes its own `devices/<id>.json` snapshot (tombstones included) into the shared iCloud folder and merges everyone else's, last-writer-wins per row. No file is written by two devices, so there is nothing for iCloud to conflict on. Thumbnails travel content-addressed; page shots and scratch stay local. Off until switched on in Settings.

**7.5 Screens.** Grid, folder tree, item detail, search, share-target confirm. Same tokens as 6.9.

---

## 8. Storage, privacy, operations

**8.1 Layout.** `~/Library/Application Support/membox/` — `membox.db` (SQLite, WAL), `blobs/`, `logs/`, `prompts.toml`.

**8.2 Portable library.** The whole directory is the library. Point membox at a different one (Dropbox, an external disk) from the library switcher.

**8.3 Backup + export.** Nightly `VACUUM INTO` snapshot, keep 7. Export a folder or a selection as JSON + blobs, or as a static HTML page.

**8.4 Privacy, stated honestly.** No membox server exists. No account, no telemetry, no analytics — the app makes zero outbound requests of its own, and never reads the pasteboard except in the instant you paste (1.1). The two things that *do* leave the machine, both plainly labelled in Settings:

1. Fetching a URL you pasted (that is the point).
2. The enrichment agent. Choosing `claude`/`codex`/`gemini` sends the page text and screenshot to that vendor under their terms. Choosing `local` (Ollama/MLX) sends nothing anywhere. The active lane is shown in the status bar at all times, and a per-folder "never send to a cloud agent" flag routes sensitive folders to the local lane regardless of the global setting.

**8.5 Secrets hygiene.** Captured content is scanned for obvious credential shapes (API keys, bearer tokens, private keys) before an agent run; matches are redacted from what the agent sees and flagged on the item.

**8.6 Logs.** Structured, on-disk, rotated. The agent queue panel (3.9) reads them.

---

## 9. Brand, icons, launch screens

**9.1 Icon concept.** A flat isometric cube — the box, empty. Three uniform colour faces (pale blue top, slate left, deep navy right) with a thin lighter edge line, on a deep navy squircle. No gradients, no glow, no contents, no rendered glass: flat geometry survives downscaling, and the mark is still legible at 16 px where a soft-lit render turns to mush. The app's glassmorphism lives in the UI (6.9), not in the icon.

**9.2 macOS.** `AppIcon.icns` at 16/32/64/128/256/512/1024 with @2x, generated from a 1024 master via `sips` + `iconutil`.

**9.3 iOS.** Full `AppIcon.appiconset` from the same master, plus tinted/dark variants for iOS 18 icon theming.

**9.4 iOS launch screen.** The `UILaunchScreen` Info.plist dict — a flat ground colour plus the centred mark, no storyboard, no spinner, no text — so it dissolves into the first frame of the real UI.

**9.5 macOS has no launch screen.** Instead: a glass "About membox" window and a first-run window with the same mark, used for library setup and agent-lane selection.

**9.6 Assets.** Master art generated locally and committed under `brand/`, with a `bin/icons` script that regenerates every derived size from the master — so a redesign is one file swap plus one command.

---

## 10. Repository

```
membox/
  core/           — Rust: SQLite store, capture, fetch pipeline, agent adapters, MCP server, search
  desktop/        — Tauri v2 app (React 19 + Vite + CSS Modules)
  ios/            — SwiftUI app over core via UniFFI
  brand/          — icon master art + generated sets
  bin/            — dev, build, icons, release scripts
  docs/           — this spec + per-subsystem docs
```

No Caddy host, no herdr launcher (0.3). `bin/dev` runs the Tauri dev window.

---

## 11. Verification

Non-negotiable checks, kept small:

- **11.1** Rust unit tests on the pure logic that will silently rot: URL canonicalisation + dedupe (1.7), type detection (1.5), the query-expression parser (5.3), `result.json` parsing per adapter (3.6).
- **11.2** One end-to-end fixture run: paste a known URL → assert an item reaches `ready` with a screenshot blob, non-empty readable text, and a folder assignment. Uses a stub agent that returns canned `result.json`, so it needs no CLI installed and no network beyond a local fixture server.
- **11.3** An MCP smoke test that starts the server, calls `navigate` + `extract_readable` against the fixture server, and asserts the token gate rejects an unauthenticated call.

No test frameworks beyond `cargo test` and `vitest`.

---

## 12. Deliberately not in v1

- Multi-user, sharing, publishing, any cloud sync (7.4 is LAN-only by design)
- Browser extension — the ⌥⌘V hotkey, the drop target and the Share sheet cover the same ground with no second codebase to ship and review
- Clipboard monitoring in any form — capture is always something you do on purpose (1.1)
- Windows/Linux desktop
- Graph view over `item_links` — the data is recorded (2.7), the view waits until there is enough of it to be worth looking at
- Editing captured content (annotations, cropping, markup)
- Full-text OCR of screenshots — revisit if readable-text extraction turns out to miss too much
- Plugin API
