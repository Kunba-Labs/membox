//! `membox-core` — the library shared by the desktop (Tauri) and iOS (UniFFI)
//! apps: SQLite store, capture, the enrichment queue, agent adapters and the
//! loopback MCP server. The host supplies a data dir and a [`browser::Browser`].
//!
//! One entry point does all the writing: [`Library::dispatch`]. Its action
//! vocabulary is exactly the desktop store's — so the webview calls
//! `invoke("dispatch", …)` and iOS calls `core.dispatch(…)` with the same names.

pub mod agent;
pub mod applog;
pub mod autotag;
pub mod browser;
pub mod capture;
pub mod db;
pub mod enrich;
pub mod mcp;
pub mod model;
pub mod sync;
pub mod triage;
pub mod ytdlp;

#[cfg(feature = "mobile-ffi")]
uniffi::setup_scaffolding!();
#[cfg(feature = "mobile-ffi")]
pub mod ffi;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde_json::{json, Value};

use browser::Browser;
use db::Db;
use model::{new_id, now, CaptureInput, Folder, Item, Settings, Snapshot};

pub type Notify = Box<dyn Fn() + Send + Sync>;

pub struct Library {
    pub db: Mutex<Db>,
    pub data_dir: PathBuf,
    pub blobs: PathBuf,
    pub browser: Arc<dyn Browser>,
    /// One browser, several things wanting it: the fetch stage holds this for a
    /// whole page, an agent's MCP call for the length of the call.
    pub browser_lock: Mutex<()>,
    pub mcp: OnceLock<Arc<mcp::McpServer>>,
    queue: Mutex<VecDeque<String>>,
    /// The agent's own queue — everything fetched, waiting its turn (§3.9).
    agent_queue: Mutex<VecDeque<String>>,
    /// Ids being worked on right now — one fetch, plus the agents in flight.
    active: Mutex<Vec<String>>,
    notify: Mutex<Option<Notify>>,
    pub sync_status: Mutex<sync::SyncStatus>,
    /// Set by `changed()`, cleared by the worker after it exports.
    dirty: std::sync::atomic::AtomicBool,
}

/// Kinds that are a *thing* rather than a page: enrich resolves a cover and a
/// synopsis for them (§3.4).
pub const SHELF: &[&str] = &["book", "movie", "tv", "game", "product"];

impl Library {
    /// Open (or create) the library at `data_dir`, start the worker and the MCP
    /// server. `browser` is what enrichment drives; pass [`browser::NoBrowser`]
    /// on hosts without one.
    pub fn open(data_dir: &Path, browser: Arc<dyn Browser>) -> Result<Arc<Library>, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let blobs = data_dir.join("blobs");
        std::fs::create_dir_all(&blobs).map_err(|e| e.to_string())?;
        let db = Db::open(&data_dir.join("membox.db")).map_err(|e| e.to_string())?;
        let lib = Arc::new(Library {
            db: Mutex::new(db),
            data_dir: data_dir.to_path_buf(),
            blobs,
            browser,
            browser_lock: Mutex::new(()),
            mcp: OnceLock::new(),
            queue: Mutex::new(VecDeque::new()),
            agent_queue: Mutex::new(VecDeque::new()),
            active: Mutex::new(Vec::new()),
            notify: Mutex::new(None),
            sync_status: Mutex::new(sync::SyncStatus::default()),
            dirty: std::sync::atomic::AtomicBool::new(false),
        });
        if lib.browser.available() {
            match mcp::McpServer::spawn(lib.clone()) {
                Ok(s) => {
                    let _ = lib.mcp.set(s);
                }
                Err(e) => log::warn!("{e}"),
            }
        }
        // Anything left mid-flight by a previous run goes back on the queue.
        if let Ok(items) = lib.db.lock().all_items() {
            for i in items.iter().filter(|i| !i.trashed && matches!(i.status.as_str(), "fetching" | "enriching" | "pending")) {
                lib.queue.lock().push_back(i.id.clone());
            }
            for i in items.iter().filter(|i| !i.trashed && i.status == "queued") {
                lib.agent_queue.lock().push_back(i.id.clone());
            }
        }
        // Suggestions nobody took up, from runs before this one.
        lib.prune_proposed();
        lib.ensure_notes_folder();
        // Pull what other devices wrote while we were away.
        if let Err(e) = lib.sync_import() {
            lib.sync_status.lock().error = Some(e);
        }
        // ...and publish ours. Export only ever ran on a change, so a device
        // that was switched on and touched nothing never appeared to the
        // others — which is exactly what "sync isn't working" looks like.
        if lib.sync_dir().is_some() {
            lib.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
            log::info!("sync on: publishing this device's snapshot");
        }
        let worker = lib.clone();
        std::thread::Builder::new()
            .name("membox-enrich".into())
            .spawn(move || {
                let mut idle_ticks: u32 = 0;
                loop {
                    // Fetch everything first — one browser, one page at a time —
                    // then hand the agents out in parallel: they are separate
                    // processes doing their own thinking (§3.9).
                    // A host with no browser (the phone, §7.3) must not take
                    // fetch work. `enrich::run` would set the row "fetching",
                    // fail, and set it back to "pending" — a status flip
                    // carrying a fresh `updated_at`, which then wins LWW over
                    // the Mac that is actually doing the fetching. Leave it
                    // queued; it waits for a host that has a browser.
                    let fetch_next = worker.browser.available().then(|| worker.queue.lock().pop_front()).flatten();
                    if let Some(id) = fetch_next {
                        worker.active.lock().push(id.clone());
                        enrich::run(&worker, &id);
                        worker.active.lock().retain(|a| a != &id);
                        continue;
                    }
                    let limit = worker.db.lock().settings().map(|s| s.agent_concurrency).unwrap_or(3).clamp(1, 8) as usize;
                    let agent_next = (worker.active.lock().len() < limit).then(|| worker.agent_queue.lock().pop_front()).flatten();
                    match agent_next {
                        Some(id) => {
                            worker.active.lock().push(id.clone());
                            let w = worker.clone();
                            let _ = std::thread::Builder::new().name("membox-agent".into()).spawn(move || {
                                enrich::run_agent(&w, &id);
                                w.active.lock().retain(|a| a != &id);
                                w.changed();
                            });
                        }
                        None => {
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            idle_ticks += 1;
                            // Export ~1s after the last change; import every ~60s.
                            if worker.dirty.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(700));
                                if let Err(e) = worker.sync_export() {
                                    worker.sync_status.lock().error = Some(e);
                                }
                            }
                            if idle_ticks % 200 == 0 {
                                if let Err(e) = worker.sync_import() {
                                    worker.sync_status.lock().error = Some(e);
                                }
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(lib)
    }

    pub fn on_change(&self, f: Notify) {
        *self.notify.lock() = Some(f);
    }

    pub fn changed(&self) {
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(f) = self.notify.lock().as_ref() {
            f();
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let db = self.db.lock();
        let mut queue: Vec<String> = self.active.lock().iter().cloned().collect();
        queue.extend(self.queue.lock().iter().cloned());
        queue.extend(self.agent_queue.lock().iter().cloned());
        Ok(Snapshot {
            items: db.all_items().map_err(|e| e.to_string())?,
            folders: db.all_folders().map_err(|e| e.to_string())?,
            settings: db.settings().map_err(|e| e.to_string())?,
            queue,
        })
    }

    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut n = std::collections::HashMap::<String, usize>::new();
        if let Ok(items) = self.db.lock().all_items() {
            for i in items.iter().filter(|i| !i.trashed) {
                for t in &i.tags {
                    *n.entry(t.clone()).or_default() += 1;
                }
            }
        }
        let mut v: Vec<_> = n.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    // ---- blobs (§2.6): content-addressed under blobs/ab/<hash>.<ext> ----

    pub fn put_blob(&self, bytes: &[u8], ext: &str) -> Result<String, String> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let rel = format!("{}/{}.{}", &hash[..2], &hash[2..], ext);
        let path = self.blobs.join(&rel);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        Ok(rel)
    }



    /// The home for stickies (§9). Deterministic id like the rest of the seed
    /// tree, created once for libraries that predate notes — and left alone
    /// afterwards, tombstone included, so deleting it is allowed to stick.
    pub const NOTES_FOLDER: &str = "f-seed-notes";

    fn ensure_notes_folder(&self) {
        let known = self.db.lock().folder_exists(Self::NOTES_FOLDER).unwrap_or(true);
        // An unseeded library gets Notes from the seed tree; this is only the
        // migration for one that predates it.
        if known {
            self.file_loose_notes();
            return;
        }
        if self.db.lock().all_folders().map(|f| f.is_empty()).unwrap_or(true) {
            return;
        }
        let f = model::Folder {
            id: Self::NOTES_FOLDER.into(),
            name: "Notes".into(),
            emoji: Some("📝".into()),
            parent_id: None,
            proposed: false,
            position: 60,
            updated_at: now(),
            deleted_at: None,
        };
        if self.db.lock().insert_folder(&f).is_ok() {
            log::info!("added the Notes folder");
            self.changed();
        }
        self.file_loose_notes();
    }

    /// Stickies written before the folder existed (or restored from another
    /// device that hadn't made it yet) belong in it too.
    fn file_loose_notes(&self) {
        let items = self.db.lock().all_items().unwrap_or_default();
        for mut it in items.into_iter().filter(|i| i.kind == "note" && !i.trashed && i.folder_ids.is_empty()) {
            it.folder_ids.push(Self::NOTES_FOLDER.to_string());
            let _ = self.db.lock().save_item(&it);
            log::info!("filed the loose note {} into Notes", it.id);
        }
    }

    /// A suggested folder is a proposal, and a proposal nobody took up is
    /// clutter (§3.8). When the last item leaves one — dragged elsewhere,
    /// trashed — it goes, unless some item still points at it as a suggestion
    /// or it has children of its own.
    pub fn prune_proposed(&self) {
        let Ok(folders) = self.db.lock().all_folders() else { return };
        if !folders.iter().any(|f| f.proposed) {
            return;
        }
        let items = self.db.lock().all_items().unwrap_or_default();
        let mut gone = false;
        for f in folders.iter().filter(|f| f.proposed) {
            let used = items.iter().any(|i| {
                !i.trashed && (i.folder_ids.contains(&f.id) || i.meta["suggestedFolderId"].as_str() == Some(f.id.as_str()))
            });
            let parent_of = folders.iter().any(|c| c.parent_id.as_deref() == Some(f.id.as_str()));
            if !used && !parent_of {
                let _ = self.db.lock().delete_folder(&f.id);
                log::info!("dropped the empty suggestion {} ({})", f.name, f.id);
                gone = true;
            }
        }
        if gone {
            self.changed();
        }
    }

    /// Put an item on the fetch queue — a capture, a retry, or work another
    /// device left for whoever has a browser.
    pub(crate) fn queue_fetch(&self, id: &str) {
        self.queue.lock().push_back(id.to_string());
    }

    /// The fetch is done; the agent gets it when it gets to it.
    pub fn queue_agent(&self, id: &str) {
        self.agent_queue.lock().push_back(id.to_string());
    }

    pub fn blob_path(&self, rel: &str) -> PathBuf {
        self.blobs.join(rel)
    }

    // ---- capture (§1) ----

    /// One paste, however many things it holds. Several URLs become several
    /// items; the first id comes back. Prose around a single URL is kept as
    /// the person's note on it.
    pub fn capture(&self, input: CaptureInput) -> Result<String, String> {
        let urls = capture::extract_urls(&input.text, &input.html);
        if urls.len() > 1 && input.image_base64.is_none() && input.file_base64.is_none() {
            let mut first = None;
            for u in urls {
                // A guess keeps its bare form so the child capture knows it is
                // one (and so the fallback text reads `Cargo.site`, not a URL).
                let text = if capture::is_guess(&u) { u.trim_start_matches("https://").to_string() } else { u };
                let id = self.capture_one(CaptureInput { text, html: String::new(), source_app: input.source_app.clone(), ..Default::default() })?;
                first.get_or_insert(id);
            }
            return first.ok_or_else(|| "nothing to capture".into());
        }
        self.capture_one(input)
    }

    fn capture_one(&self, input: CaptureInput) -> Result<String, String> {
        let url = capture::extract_urls(&input.text, &input.html).into_iter().next();
        let canonical = url.as_deref().and_then(capture::canonical_url);
        let mut thumb = None;
        let mut size = String::new();
        let mut aspect = 4.0 / 3.0;
        let mut dims = "—".to_string();
        let mut file_blob: Option<String> = None;
        if let Some(b64) = &input.file_base64 {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64.split(',').last().unwrap_or("")).map_err(|e| e.to_string())?;
            let ext = input.file_name.as_deref().and_then(|n| n.rsplit('.').next()).map(|e| e.to_ascii_lowercase()).unwrap_or_else(|| "bin".into());
            size = enrich::human_size(bytes.len());
            file_blob = Some(self.put_blob(&bytes, &ext)?);
        }
        if let Some(b64) = &input.image_base64 {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64.split(',').last().unwrap_or("")).map_err(|e| e.to_string())?;
            let ext = enrich::image_ext(&bytes);
            if let Some((w, h)) = enrich::image_dims(&bytes) {
                aspect = w as f64 / h as f64;
                dims = format!("{w}×{h}");
            }
            size = enrich::human_size(bytes.len());
            thumb = Some(self.put_blob(&bytes, ext)?);
        }

        // A pasted screenshot carries no url and no text, so the old key was
        // the hash of "" for every one of them — the second screenshot ever
        // pasted found the first and became a `last_seen_at` bump. The blob's
        // own path is its content hash: the same image is the same item.
        let key = match thumb.as_ref().or(file_blob.as_ref()) {
            Some(blob) if canonical.is_none() => capture::dedupe_key(None, blob),
            _ => capture::dedupe_key(canonical.as_deref(), &input.text),
        };
        let existing = self.db.lock().find_by_dedupe(&key).map_err(|e| e.to_string())?;
        if let Some(existing) = existing {
            let mut e = existing;
            e.last_seen_at = now();
            self.db.lock().save_item(&e).map_err(|e| e.to_string())?;
            self.changed();
            return Ok(e.id);
        }

        let kind = match input.file_name.as_deref().and_then(|n| n.rsplit('.').next()).map(|e| e.to_ascii_lowercase()) {
            Some(ext) if file_blob.is_some() && ext == "pdf" => "pdf".to_string(),
            Some(ext) if file_blob.is_some() && ["png", "jpg", "jpeg", "gif", "webp", "heic"].contains(&ext.as_str()) => "image".to_string(),
            _ => capture::detect_kind(canonical.as_deref(), thumb.is_some(), file_blob.is_some()).to_string(),
        };
        // Triage said this line is a book / a film / a thing to buy. A URL of
        // its own still wins; a bare name gets the hinted kind and is looked up.
        let hint = input.kind.as_deref().filter(|k| SHELF.contains(k));
        let kind = match hint {
            Some(h) if kind == "snippet" || kind == "webpage" => h.to_string(),
            _ => kind,
        };
        let named = kind != "snippet" && canonical.is_none() && file_blob.is_none() && thumb.is_none();
        // Prose around the link is the person's own note on it.
        let notes = if let Some(n) = input.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(n.to_string())
        } else if canonical.is_some() {
            let rest: String = input.text.split_whitespace().filter(|t| !t.contains("://") && !capture::extract_urls(t, "").iter().any(|u| u.contains(t))).collect::<Vec<_>>().join(" ");
            let rest = rest.trim().trim_matches(|c: char| matches!(c, ',' | ':' | '-' | '—')).trim().to_string();
            (!rest.is_empty()).then_some(rest)
        } else {
            None
        };
        let is_code = kind == "snippet" && capture::looks_like_code(&input.text);
        // A bare `Cargo.site` we only guessed was a host — enrich puts it back
        // to text if the browser can't load it.
        let guess = url.as_deref().filter(|u| !input.text.contains("://") && capture::is_guess(u)).map(|u| u.trim_start_matches("https://").to_string());
        let id = new_id("i");
        let domain = canonical.as_ref().and_then(|c| url::Url::parse(c).ok()).and_then(|u| u.host_str().map(String::from)).or(input.source_app.clone());
        let text = input.text.trim().to_string();
        let item = Item {
            id: id.clone(),
            kind: kind.clone(),
            title: input.file_name.clone().unwrap_or_else(|| capture::draft_title(canonical.as_deref(), &text)),
            url: canonical.clone().or(url),
            domain,
            dedupe_key: key,
            thumb,
            aspect,
            status: if canonical.is_some() || file_blob.is_some() || named { "pending".into() } else { "ready".into() },
            body_html: if input.html.trim().is_empty() { None } else { Some(input.html.clone()) },
            body_text: if kind == "snippet" { Some(if is_code { text.clone() } else { strip_html_or(&input.html, &text) }) } else { None },
            summary: if kind == "snippet" { Some(if is_code { text.chars().take(600).collect() } else { strip_html_or(&input.html, &text).chars().take(600).collect() }) } else { None },
            notes,
            tags: if is_code { vec!["code".into()] } else { vec![] },
            auto_tags: if is_code { vec!["code".into()] } else { vec![] },
            added_at: now(),
            last_seen_at: now(),
            size: if size.is_empty() { enrich::human_size(text.len() + input.html.len()) } else { size },
            dimensions: dims,
            palette: vec!["#2c4a6e".into(), "#3f6d99".into(), "#7fa8cc".into(), "#c8dcea".into(), "#8a93a3".into(), "#5a6272".into()],
            meta: json!({ "file": file_blob, "fileName": input.file_name, "guess": guess,
                "query": named.then(|| text.clone()) }),
            ..Default::default()
        };
        self.db.lock().insert_item(&item).map_err(|e| e.to_string())?;
        if canonical.is_some() || file_blob.is_some() || named {
            self.queue.lock().push_back(id.clone());
        }
        self.changed();
        Ok(id)
    }

    /// §1.4 — what did the person paste? Rules cover links and links-with-a-note;
    /// a line that is a name (a product, a book, a series) is what the agent
    /// lane is for, and a single link never spends one.
    pub fn plan_paste(&self, text: &str, html: &str, note: &str) -> triage::Plan {
        let plan = triage::plan_rules(text, html);
        if !triage::needs_agent(&plan) {
            return plan;
        }
        let settings = self.db.lock().settings().unwrap_or_default();
        let Some(ad) = agent::by_key(&settings.agent).filter(|a| which::which(a.binary).is_ok()) else { return plan };
        let scratch = self.data_dir.join("scratch").join("paste");
        let _ = std::fs::remove_dir_all(&scratch);
        triage::plan_with_agent(text, note, ad, &settings.local_model, &scratch, std::time::Duration::from_secs(90)).unwrap_or(plan)
    }

    /// A crawl child (§3.7): captured like a paste, linked to its parent.
    pub fn capture_child(&self, url: &str, parent: &str) -> Result<String, String> {
        let id = self.capture(CaptureInput { text: url.to_string(), ..Default::default() })?;
        let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
        if let Some(mut it) = found {
            it.meta["parentId"] = json!(parent);
            self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
        }
        Ok(id)
    }

    /// Machine tags: added to both lists, never over a tag the person chose.
    pub fn apply_auto_tags(&self, item: &mut Item) {
        for t in autotag::auto_tags(item) {
            if !item.tags.contains(&t) {
                item.tags.push(t.clone());
                item.auto_tags.push(t);
            }
        }
    }

    pub fn set_status(&self, id: &str, status: &str, error: Option<&str>) {
        let found = self.db.lock().get_item(id);
        if let Ok(Some(mut it)) = found {
            it.status = status.into();
            it.error = error.map(String::from);
            let _ = self.db.lock().save_item(&it);
        }
        self.changed();
    }

    pub fn create_folder(&self, name: &str, parent: Option<&str>, emoji: Option<&str>, proposed: bool) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("empty name".into());
        }
        let db = self.db.lock();
        if let Some(id) = db.find_folder_by_name(name, parent).map_err(|e| e.to_string())? {
            return Ok(id);
        }
        let f = Folder {
            id: new_id("f"),
            name: name.into(),
            emoji: emoji.map(String::from).or_else(|| if parent.is_none() { Some("📁".into()) } else { None }),
            parent_id: parent.map(String::from),
            proposed,
            position: db.all_folders().map(|v| v.len() as i64).unwrap_or(0),
            updated_at: now(),
            deleted_at: None,
        };
        db.insert_folder(&f).map_err(|e| e.to_string())?;
        drop(db);
        self.changed();
        Ok(f.id)
    }

    // ---- what the agent may write (§3.6/§3.8/§3.12) ----

    /// `set_item` over MCP: same rules as a result.json, partial.
    pub fn apply_agent_patch(&self, id: &str, a: &Value) -> Result<(), String> {
        let mut r = json!({});
        for k in ["title", "summary", "tags", "reason"] {
            if !a[k].is_null() {
                r[k] = a[k].clone();
            }
        }
        if let Some(f) = a["folderId"].as_str() {
            r["folder"] = json!({ "existingId": f });
        }
        self.apply_agent_result(id, &r)
    }

    pub fn apply_agent_result(&self, id: &str, r: &Value) -> Result<(), String> {
        let settings = self.db.lock().settings().unwrap_or_default();
        let Some(mut it) = self.db.lock().get_item(id).map_err(|e| e.to_string())? else { return Err("item gone".into()) };
        // §3.12: anything the person typed wins.
        if !it.user_edited {
            if let Some(t) = r["title"].as_str().filter(|t| !t.trim().is_empty()) {
                it.title = t.trim().to_string();
            }
        }
        if let Some(s) = r["summary"].as_str() {
            it.summary = Some(s.trim().to_string());
        }
        if let Some(tags) = r["tags"].as_array() {
            for t in tags.iter().filter_map(Value::as_str) {
                let t = t.trim().to_lowercase();
                if !t.is_empty() && !it.tags.contains(&t) {
                    it.tags.push(t.clone());
                    it.auto_tags.push(t); // the agent is a machine too
                }
            }
        }
        if let Some(ents) = r["entities"].as_array() {
            it.meta["entities"] = json!(ents);
        }
        it.confidence = r["confidence"].as_f64();
        it.agent_reason = r["reason"].as_str().map(String::from).or_else(|| r["folder"]["suggest"]["why"].as_str().map(String::from));

        // Folder: an existing id above the threshold applies; a suggestion becomes
        // a proposed folder the person accepts from the sidebar (§3.8).
        let conf = it.confidence.unwrap_or(1.0);
        if let Some(fid) = r["folder"]["existingId"].as_str() {
            let exists = self.db.lock().all_folders().map(|v| v.iter().any(|f| f.id == fid)).unwrap_or(false);
            if exists && conf >= settings.auto_file_threshold && it.folder_ids.is_empty() {
                it.folder_ids.push(fid.to_string());
            } else if exists {
                it.meta["suggestedFolderId"] = json!(fid);
            }
        } else if let Some(name) = r["folder"]["suggest"]["name"].as_str() {
            let parent = r["folder"]["suggest"]["parentId"].as_str().filter(|p| !p.is_empty() && *p != "null");
            let emoji = r["folder"]["suggest"]["emoji"].as_str();
            // The same name is the same folder, whoever asked for it.
            let same = |a: &str| a.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
            let existing = self.db.lock().all_folders().ok().and_then(|fs| fs.into_iter().find(|f| same(&f.name) == same(name)).map(|f| f.id));
            let fid = match existing {
                Some(id) => id,
                None => self.create_folder(name, parent, emoji, true)?,
            };
            it.meta["suggestedFolderId"] = json!(fid);
        }
        // The agent found the thing itself: a product page for a name, or the
        // right shelf for a snippet. Either one earns another fetch — for the
        // cover and the screenshot, not for another opinion.
        let mut again = false;
        if let Some(k) = r["kind"].as_str().filter(|k| SHELF.contains(k)) {
            if it.kind != k && (it.kind == "snippet" || SHELF.contains(&it.kind.as_str())) {
                it.kind = k.to_string();
                again = true;
            }
        }
        if it.url.is_none() {
            if let Some(u) = r["url"].as_str().and_then(capture::canonical_url) {
                it.domain = url::Url::parse(&u).ok().and_then(|p| p.host_str().map(String::from));
                it.url = Some(u);
                again = true;
            }
        }
        if again {
            it.status = "pending".into();
            it.meta["skipAgent"] = json!(true);
        }
        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
        if again {
            self.queue.lock().push_back(it.id.clone());
        }
        self.changed();
        Ok(())
    }

    // ---- the one write surface ----

    pub fn dispatch(&self, action: &str, a: Value) -> Result<Value, String> {
        let s = |k: &str| a[k].as_str().map(str::to_string);
        let ids = || -> Vec<String> { a["ids"].as_array().map(|v| v.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default() };
        let out = match action {
            "snapshot" => return serde_json::to_value(self.snapshot()?).map_err(|e| e.to_string()),
            "capture" => json!(self.capture(serde_json::from_value(a.clone()).map_err(|e| e.to_string())?)?),
            // §1.4 — read the paste before writing anything. Rules first; the
            // agent lane only when a line is a name rather than a link.
            "plan" => serde_json::to_value(self.plan_paste(&s("text").unwrap_or_default(), &s("html").unwrap_or_default(), &s("note").unwrap_or_default())).map_err(|e| e.to_string())?,
            // The plan, captured: one item per entry, each with its own note.
            "capturePlan" => {
                let plan: triage::Plan = serde_json::from_value(a["plan"].clone()).map_err(|e| e.to_string())?;
                let intent = s("note").or(plan.intent.clone()).filter(|n| !n.trim().is_empty());
                let mut out = Vec::new();
                for e in plan.entries {
                    let note = match (&e.note, &intent) {
                        (Some(n), Some(i)) => Some(format!("{i} — {n}")),
                        (Some(n), None) => Some(n.clone()),
                        (None, i) => i.clone(),
                    };
                    let id = self.capture_one(CaptureInput {
                        text: e.text.clone(),
                        source_app: s("sourceApp"),
                        note,
                        kind: SHELF.contains(&e.kind.as_str()).then(|| e.kind.clone()),
                        ..Default::default()
                    })?;
                    if let Some(t) = e.title.as_deref().filter(|t| !t.trim().is_empty()) {
                        // Bind first: a lock guard in the `if let` scrutinee
                        // lives for the whole block (CLAUDE.md gotcha #1).
                        let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
                        if let Some(mut it) = found {
                            if it.title == e.text {
                                it.title = t.to_string();
                                let _ = self.db.lock().save_item(&it);
                            }
                        }
                    }
                    out.push(id);
                }
                json!({ "ids": out })
            }
            // "these are all things I want to buy" — the person's word on a
            // batch, after the fact. Anything already finished goes round again
            // so the agent files it knowing what it is.
            "annotate" => {
                let note = s("note").unwrap_or_default().trim().to_string();
                // "these are books" — believe them. A line kept as text becomes
                // the thing it was named as, and goes looking for its cover.
                let said = note.to_lowercase();
                let shelf = ["book", "film", "movie", "series", "show", "tv", "game", "buy", "product", "shopping"]
                    .iter()
                    .find(|w| said.split(|c: char| !c.is_alphanumeric()).any(|t| t.trim_end_matches('s') == **w))
                    .map(|w| match *w {
                        "book" => "book",
                        "film" | "movie" => "movie",
                        "series" | "show" | "tv" => "tv",
                        "game" => "game",
                        _ => "product",
                    });
                for id in ids() {
                    let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
                    if let Some(mut it) = found {
                        if !note.is_empty() {
                            it.notes = Some(match it.notes.take() {
                                Some(old) if !old.contains(&note) => format!("{note} — {old}"),
                                Some(old) => old,
                                None => note.clone(),
                            });
                        }
                        if let Some(k) = shelf.filter(|_| it.kind == "snippet" && it.url.is_none()) {
                            it.kind = k.to_string();
                            it.meta["query"] = json!(it.title.clone());
                        }
                        let requeue = it.status == "ready" || it.status == "failed";
                        if requeue {
                            it.status = "pending".into();
                            it.error = None;
                        }
                        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                        if requeue {
                            self.queue.lock().push_back(id);
                        }
                    }
                }
                json!(true)
            }
            "search" => json!(self.db.lock().search(&s("query").unwrap_or_default()).map_err(|e| e.to_string())?),
            // A sticky (§9): a title, a rich-text body, and links to whatever
            // else in the library it is about. An item like any other — so it
            // is searched, tagged, filed, trashed and synced like any other.
            "newNote" => {
                let id = new_id("i");
                let now_ = now();
                let item = Item {
                    id: id.clone(),
                    kind: "note".into(),
                    title: s("title").unwrap_or_else(|| "New note".into()),
                    dedupe_key: id.clone(),
                    status: "ready".into(),
                    body_html: Some(s("bodyHtml").unwrap_or_default()),
                    aspect: 1.0,
                    added_at: now_.clone(),
                    last_seen_at: now_,
                    size: "—".into(),
                    dimensions: "—".into(),
                    palette: vec!["#f0b323".into(), "#d93b2b".into(), "#2d51e0".into(), "#e8e6e1".into(), "#202027".into(), "#98979f".into()],
                    user_edited: true,
                    folder_ids: self
                        .db
                        .lock()
                        .folder_exists(Self::NOTES_FOLDER)
                        .unwrap_or(false)
                        .then(|| vec![Self::NOTES_FOLDER.to_string()])
                        .unwrap_or_default(),
                    ..Default::default()
                };
                self.db.lock().insert_item(&item).map_err(|e| e.to_string())?;
                self.changed();
                json!(id)
            }
            "update" => {
                let id = s("id").ok_or("id")?;
                let mut it = self.db.lock().get_item(&id).map_err(|e| e.to_string())?.ok_or("not found")?;
                let p = &a["patch"];
                if let Some(v) = p["title"].as_str() { it.title = v.into(); it.user_edited = true; }
                if let Some(v) = p["notes"].as_str() { it.notes = Some(v.into()); it.user_edited = true; }
                if let Some(v) = p["summary"].as_str() { it.summary = Some(v.into()); it.user_edited = true; }
                // A sticky's body. The plain text goes to `body_text` so search
                // reads it, and the ids it links to are read back out of the
                // markup — the note itself is the record of what it points at.
                if let Some(v) = p["bodyHtml"].as_str() {
                    it.body_html = Some(v.into());
                    let text = strip_html_or(v, "");
                    it.summary = Some(text.chars().take(300).collect());
                    it.body_text = Some(text);
                    it.meta["links"] = json!(linked_ids(v));
                    it.user_edited = true;
                }
                // A sticky's paper colour (§9) — the person's choice, not a tag.
                if let Some(v) = p["color"].as_str() { it.meta["color"] = json!(v); it.user_edited = true; }
                if let Some(v) = p["rating"].as_i64() { it.rating = v; }
                if let Some(v) = p["status"].as_str() { it.status = v.into(); }
                self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                json!(true)
            }
            "setRating" => {
                let id = s("id").ok_or("id")?;
                let r = a["rating"].as_i64().unwrap_or(0);
                let mut it = self.db.lock().get_item(&id).map_err(|e| e.to_string())?.ok_or("not found")?;
                it.rating = if it.rating == r { 0 } else { r };
                self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                json!(it.rating)
            }
            "addTag" | "removeTag" => {
                let id = s("id").ok_or("id")?;
                let tag = s("tag").unwrap_or_default().trim().to_lowercase();
                let mut it = self.db.lock().get_item(&id).map_err(|e| e.to_string())?.ok_or("not found")?;
                if action == "addTag" {
                    // Typing (or re-adding) a tag makes it the person's — §2.5.
                    if !tag.is_empty() && !it.tags.contains(&tag) { it.tags.push(tag.clone()); }
                    it.auto_tags.retain(|t| t != &tag);
                } else {
                    it.tags.retain(|t| t != &tag);
                    it.auto_tags.retain(|t| t != &tag);
                }
                it.user_edited = true;
                self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                json!(true)
            }
            "addToFolder" | "removeFromFolder" => {
                let fid = s("folderId").ok_or("folderId")?;
                let list = if a["ids"].is_array() { ids() } else { vec![s("id").ok_or("id")?] };
                for id in list {
                    let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
                    if let Some(mut it) = found {
                        if action == "addToFolder" {
                            if !it.folder_ids.contains(&fid) {
                                it.folder_ids.push(fid.clone());
                            }
                            // Dragged out of the folder you were looking at: that
                            // is a move, not a copy — and it is what empties a
                            // suggestion so it can go.
                            if let Some(from) = s("from").filter(|f| f != &fid) {
                                it.folder_ids.retain(|f| f != &from);
                            }
                            // Filed by hand, so a pending suggestion is stale.
                            it.meta["suggestedFolderId"] = Value::Null;
                        } else {
                            it.folder_ids.retain(|f| f != &fid);
                        }
                        it.user_edited = true;
                        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                    }
                }
                // Filing into a proposed folder is accepting it.
                if action == "addToFolder" {
                    self.db.lock().update_folder(&fid, None, Some(false)).map_err(|e| e.to_string())?;
                }
                self.prune_proposed();
                json!(true)
            }
            "trash" => {
                let t = a["trashed"].as_bool().unwrap_or(true);
                for id in ids() {
                    let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
                    if let Some(mut it) = found {
                        it.trashed = t;
                        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                    }
                }
                self.prune_proposed();
                json!(true)
            }
            "deleteForever" => { self.db.lock().delete_items(&ids()).map_err(|e| e.to_string())?; self.prune_proposed(); json!(true) }
            "emptyTrash" => { let n = self.db.lock().delete_trashed().map_err(|e| e.to_string())?.len(); self.prune_proposed(); json!(n) }
            "createFolder" => json!(self.create_folder(&s("name").unwrap_or_default(), s("parentId").as_deref(), s("emoji").as_deref(), false)?),
            "renameFolder" => { self.db.lock().update_folder(&s("id").ok_or("id")?, s("name").as_deref(), None).map_err(|e| e.to_string())?; json!(true) }
            "acceptFolder" => { self.db.lock().update_folder(&s("id").ok_or("id")?, None, Some(false)).map_err(|e| e.to_string())?; json!(true) }
            "deleteFolder" => {
                let gone = self.db.lock().delete_folder(&s("id").ok_or("id")?).map_err(|e| e.to_string())?;
                let all = self.db.lock().all_items().map_err(|e| e.to_string())?;
                for mut it in all {
                    if it.folder_ids.iter().any(|f| gone.contains(f)) {
                        it.folder_ids.retain(|f| !gone.contains(f));
                        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                    }
                }
                json!(true)
            }
            "reenrich" | "refetch" => {
                // refetch: screenshot + readable text again, keep the agent out
                // of it (nothing to re-decide, nothing to re-spend).
                let list = if a["ids"].is_array() { ids() } else { vec![s("id").ok_or("id")?] };
                for id in list {
                    let found = self.db.lock().get_item(&id).map_err(|e| e.to_string())?;
                    if let Some(mut it) = found {
                        it.meta["skipAgent"] = json!(action == "refetch");
                        it.status = "pending".into();
                        it.error = None;
                        self.db.lock().save_item(&it).map_err(|e| e.to_string())?;
                    }
                    self.queue.lock().push_back(id);
                }
                json!(true)
            }
            "settings" => {
                let mut st = self.db.lock().settings().unwrap_or_default();
                if let Some(v) = s("agent") { st.agent = v; }
                if let Some(v) = s("localModel") { st.local_model = v; }
                if let Some(v) = a["autoFileThreshold"].as_f64() { st.auto_file_threshold = v; }
                if let Some(v) = a["agentConcurrency"].as_i64() { st.agent_concurrency = v.clamp(1, 8); }
                if let Some(v) = a["crawlDepth"].as_i64() { st.crawl_depth = v; }
                if let Some(v) = a["syncEnabled"].as_bool() { st.sync_enabled = v; }
                if let Some(v) = s("theme") { st.theme = v; }
                if !a["syncDir"].is_null() { st.sync_dir = a["syncDir"].as_str().filter(|d| !d.is_empty()).map(String::from); }
                self.db.lock().save_settings(&st).map_err(|e| e.to_string())?;
                serde_json::to_value(st).unwrap()
            }
            // Import then export, on demand (app focus, a "Sync now" button).
            // Where things live, for when something needs investigating.
            "paths" => json!({
                "data": self.data_dir.display().to_string(),
                "log": self.data_dir.join("membox.log").display().to_string(),
                "blobs": self.blobs.display().to_string(),
                "scratch": self.data_dir.join("scratch").display().to_string(),
            }),
            "sync" => {
                let imported = self.sync_import()?;
                self.sync_export()?;
                serde_json::to_value(self.sync_status.lock().clone()).unwrap()
                    .as_object().cloned().map(|mut m| { m.insert("importedNow".into(), json!(imported)); Value::Object(m) }).unwrap()
            }
            "syncStatus" => {
                let mut st = self.sync_status.lock().clone();
                if st.dir.is_none() { st.dir = self.settings().sync_dir.map(PathBuf::from).or_else(|| sync::default_dir(&self.data_dir)).map(|p| p.display().to_string()); }
                st.error = st.error.clone();
                serde_json::to_value(st).unwrap()
            }
            "agents" => serde_json::to_value(agent::detect()).unwrap(),
            "runs" => serde_json::to_value(self.db.lock().runs_for(&s("id").ok_or("id")?).map_err(|e| e.to_string())?).unwrap(),
            "audit" => serde_json::to_value(self.mcp.get().map(|m| m.audit.lock().clone()).unwrap_or_default()).unwrap(),
            "mcp" => json!(self.mcp.get().map(|m| json!({ "url": m.url(), "token": m.token }))),
            "reset" => {
                let db = self.db.lock();
                db.conn.execute_batch("DELETE FROM items; DELETE FROM items_fts; DELETE FROM folders; DELETE FROM agent_runs;").map_err(|e| e.to_string())?;
                drop(db);
                seed::seed(self)?;
                json!(true)
            }
            _ => return Err(format!("unknown action {action}")),
        };
        self.changed();
        Ok(out)
    }

    pub fn settings(&self) -> Settings {
        self.db.lock().settings().unwrap_or_default()
    }
}

/// The item ids a sticky's markup points at — `data-item="i-…"`.
pub(crate) fn linked_ids(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("data-item=\"") {
        rest = &rest[i + 11..];
        let end = rest.find('"').unwrap_or(rest.len());
        let id = &rest[..end];
        if id.starts_with("i-") && !out.contains(&id.to_string()) {
            out.push(id.to_string());
        }
        rest = &rest[end..];
    }
    out
}

pub(crate) fn strip_html_or(html: &str, text: &str) -> String {
    if html.trim().is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => { in_tag = false; out.push(' '); }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() { text.to_string() } else { collapsed }
}

/// The starter folder tree a fresh library gets, so the agent has somewhere
/// to file things on day one. Empty of items — the person's own pastes fill it.
///
/// Ids are deterministic (`f-seed-watching-talks`), not random: every device
/// seeds the same tree, and iCloud sync (§7.4) must recognise them as the
/// same folders rather than merging two copies of "Watching".
pub mod seed {
    use super::*;

    fn slug(s: &str) -> String {
        s.chars().filter_map(|c| if c.is_ascii_alphanumeric() { Some(c.to_ascii_lowercase()) } else if c == ' ' { Some('-') } else { None }).collect()
    }

    pub fn seed(lib: &Library) -> Result<(), String> {
        if !lib.db.lock().all_folders().map_err(|e| e.to_string())?.is_empty() {
            return Ok(());
        }
        let mut position = 0;
        for (emoji, name, kids) in [
            ("🎬", "Watching", &["Documentaries", "Talks", "Tutorials", "Film"][..]),
            ("📚", "Reading", &["Longform", "Docs & Refs", "Newsletters"][..]),
            ("🎧", "Listening", &["Sets", "Albums"][..]),
            ("🌍", "Travel", &[][..]),
            ("🛠", "Build", &["Code", "Design"][..]),
            ("📝", "Notes", &[][..]),
        ] {
            let pid = format!("f-seed-{}", slug(name));
            let mut insert = |id: String, name: &str, parent: Option<&str>, emoji: Option<&str>| -> Result<(), String> {
                position += 1;
                lib.db.lock().insert_folder(&Folder { id, name: name.into(), emoji: emoji.map(String::from), parent_id: parent.map(String::from), proposed: false, position, updated_at: now(), deleted_at: None }).map_err(|e| e.to_string())
            };
            insert(pid.clone(), name, None, Some(emoji))?;
            for k in kids {
                insert(format!("{pid}-{}", slug(k)), k, Some(&pid), None)?;
            }
        }
        lib.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Arc<Library> {
        let dir = tempfile::tempdir().unwrap();
        // So `cargo test -- --nocapture` shows the same trail the app writes.
        applog::init(dir.path(), log::LevelFilter::Info);
        let l = Library::open(dir.path(), Arc::new(browser::NoBrowser)).unwrap();
        std::mem::forget(dir);
        l
    }

    #[test]
    fn capture_dedupes_and_snippets_are_ready() {
        let l = lib();
        let a = l.capture(CaptureInput { text: "see https://youtu.be/abc123?si=1".into(), ..Default::default() }).unwrap();
        let b = l.capture(CaptureInput { text: "https://www.youtube.com/watch?v=abc123".into(), ..Default::default() }).unwrap();
        assert_eq!(a, b);
        let s = l.capture(CaptureInput { text: "just a note".into(), html: "<b>just</b> a note".into(), ..Default::default() }).unwrap();
        let it = l.db.lock().get_item(&s).unwrap().unwrap();
        assert_eq!(it.kind, "snippet");
        assert_eq!(it.status, "ready");
        assert_eq!(it.body_text.as_deref(), Some("just a note"));
        assert_eq!(l.snapshot().unwrap().items.len(), 2);
    }

    /// A host that never resolves — every guess falls back through here.
    struct DeadBrowser;
    impl browser::Browser for DeadBrowser {
        fn navigate(&self, url: &str) -> browser::Result<()> {
            Err(browser::BrowserError(format!("could not load {url}")))
        }
        fn eval(&self, _: &str) -> browser::Result<String> {
            Err(browser::BrowserError("no page".into()))
        }
        fn snapshot_png(&self, _: bool) -> browser::Result<Vec<u8>> {
            Err(browser::BrowserError("no page".into()))
        }
    }

    /// Every page lands on the same article, the way a short link does.
    struct Redirects;
    impl browser::Browser for Redirects {
        fn navigate(&self, _: &str) -> browser::Result<()> {
            Ok(())
        }
        fn eval(&self, _: &str) -> browser::Result<String> {
            Ok("\"https://www.example.com/post/1?utm_source=x\"".into())
        }
        fn snapshot_png(&self, _: bool) -> browser::Result<Vec<u8>> {
            Err(browser::BrowserError("no pixels".into()))
        }
    }

    #[test]
    fn a_short_link_to_a_saved_page_merges_into_it() {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::open(dir.path(), Arc::new(Redirects)).unwrap();
        let first = l.capture_one(CaptureInput { text: "https://example.com/post/1".into(), ..Default::default() }).unwrap();
        let short = l.capture_one(CaptureInput { text: "https://bit.ly/abc".into(), ..Default::default() }).unwrap();
        assert_ne!(first, short);
        enrich::run(&l, &short);
        let items = l.snapshot().unwrap().items;
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec![first.as_str()]);
        // The saved page still dedupes its own url, trailing slash and all.
        assert_eq!(l.capture_one(CaptureInput { text: "https://example.com/post/1/".into(), ..Default::default() }).unwrap(), first);
    }

    #[test]
    fn bare_hosts_are_tried_then_fall_back_to_text() {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::open(dir.path(), Arc::new(DeadBrowser)).unwrap();
        // "Death to stock" is prose; the other two are guesses worth opening.
        l.capture(CaptureInput { text: "Intangible.ai\nDeath to stock\nCargo.site".into(), ..Default::default() }).unwrap();
        let items = l.snapshot().unwrap().items;
        assert_eq!(items.len(), 2);
        let id = items.iter().find(|i| i.title.contains("intangible")).unwrap().id.clone();
        assert_eq!(l.db.lock().get_item(&id).unwrap().unwrap().kind, "webpage");

        enrich::run(&l, &id);
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.kind, "snippet");
        assert_eq!(it.url, None);
        assert_eq!(it.title, "Intangible.ai");
        assert_eq!(it.body_text.as_deref(), Some("Intangible.ai"));
        assert_eq!(it.status, "ready");
        assert_eq!(it.error, None);
    }

    #[test]
    fn a_pasted_list_becomes_one_item_per_line_with_its_own_note() {
        let l = lib();
        let plan = l.plan_paste("books to read:\nhttps://example.com/lamp — the small one\nThe Dispossessed", "", "");
        assert_eq!(plan.intent.as_deref(), Some("books to read"));
        let ids: Vec<String> = serde_json::from_value(
            l.dispatch("capturePlan", json!({ "plan": { "entries": [
                { "kind": "url", "text": "https://example.com/lamp", "note": "the small one" },
                { "kind": "book", "text": "The Dispossessed", "title": "The Dispossessed" }
            ] }, "note": "books to read" })).unwrap()["ids"].clone(),
        )
        .unwrap();
        assert_eq!(ids.len(), 2);
        let a = l.db.lock().get_item(&ids[0]).unwrap().unwrap();
        assert_eq!(a.kind, "webpage");
        assert_eq!(a.notes.as_deref(), Some("books to read — the small one"));
        let b = l.db.lock().get_item(&ids[1]).unwrap().unwrap();
        assert_eq!(b.kind, "book");
        assert_eq!(b.title, "The Dispossessed");
        assert_eq!(b.notes.as_deref(), Some("books to read"));
        assert_eq!(b.status, "pending", "a named book is queued for its cover");

        // Saying what a batch is, after the fact.
        l.dispatch("annotate", json!({ "ids": ids, "note": "things I want to buy" })).unwrap();
        let a = l.db.lock().get_item(&ids[0]).unwrap().unwrap();
        assert!(a.notes.as_deref().unwrap().starts_with("things I want to buy"));
    }

    /// Real network, so it stays out of the normal run:
    /// `cargo test -p membox-core -- --ignored covers_come_back`
    /// The whole chain on a scratch library, with the real CLI and the real
    /// network: `cargo test -p membox-core -- --ignored a_list_of_books`
    #[test]
    #[ignore]
    fn a_list_of_books_becomes_books_with_covers() {
        let l = lib();
        l.dispatch("settings", json!({ "agent": "claude" })).unwrap();
        let text = "The Formation of the Secular Mind\nFormations of the Secular\nPolitics of Piety";
        let plan = l.plan_paste(text, "", "these are books I want to read");
        assert_eq!(plan.entries.len(), 3, "{plan:?}");
        assert!(plan.entries.iter().all(|e| e.kind == "book"), "{plan:?}");

        let ids: Vec<String> = serde_json::from_value(
            l.dispatch("capturePlan", json!({ "plan": plan, "note": "these are books I want to read" })).unwrap()["ids"].clone(),
        )
        .unwrap();
        assert_eq!(ids.len(), 3);
        for id in &ids {
            let mut it = l.db.lock().get_item(id).unwrap().unwrap();
            assert_eq!(it.kind, "book", "{}", it.title);
            let scratch = l.data_dir.join("scratch").join(id);
            std::fs::create_dir_all(&scratch).unwrap();
            let found = enrich::fetch_for_test(&l, &mut it, &scratch, &mut None);
            println!("{} → {:?} cover={} {:?}", it.title, it.url, it.thumb.is_some(), it.meta["author"]);
            match found {
                // Identified: a real link and a real cover.
                Ok(()) => assert!(it.url.as_deref().unwrap_or("").contains("openlibrary.org"), "{}", it.title),
                // Not identified: say so. A stranger's cover would be worse
                // than an empty one ("The Formation of the Secular Mind" is
                // not in Open Library, and its top hit is a different book).
                Err(e) => {
                    assert!(e.contains("no confident match") || e.contains("no book found"), "{e}");
                    assert!(it.thumb.is_none() && it.url.is_none(), "{}", it.title);
                }
            }
        }
    }

    /// Runs the real Claude Code CLI:
    /// `cargo test -p membox-core -- --ignored triage_reads`
    #[test]
    #[ignore]
    fn triage_reads_three_titles_as_books() {
        let l = lib();
        l.dispatch("settings", json!({ "agent": "claude" })).unwrap();
        let p = l.plan_paste("Formulations of the secular\nPolitics of piety\nIslamic secularism", "", "stuff I want to read");
        assert_eq!(p.source, "claude", "the agent lane never ran");
        assert_eq!(p.entries.len(), 3, "{p:?}");
        assert!(p.entries.iter().all(|e| e.kind == "book"), "{p:?}");

        // The same lines are perfumes when the person says they are.
        let p = l.plan_paste("Alfonso Mocha\nGuerlain tobacco honey\nMaison Margiela Replica By The Fireplace", "", "stuff i want to buy");
        assert_eq!(p.entries.len(), 3, "{p:?}");
        assert!(p.entries.iter().all(|e| e.kind == "product"), "{p:?}");
    }

    /// Real network. `cargo test -p membox-core -- --ignored hacker_news`
    #[test]
    #[ignore]
    fn hacker_news_keeps_the_thing_and_the_thread() {
        let l = lib();
        let id = l.capture(CaptureInput { text: "https://news.ycombinator.com/item?id=49426564".into(), ..Default::default() }).unwrap();
        // A Launch HN has no url of its own; the pitch's first link is the product.
        let launch = l.capture(CaptureInput { text: "https://news.ycombinator.com/item?id=41236273".into(), ..Default::default() }).unwrap();
        let mut lit = l.db.lock().get_item(&launch).unwrap().unwrap();
        let lscratch = l.data_dir.join("scratch").join(&launch);
        std::fs::create_dir_all(&lscratch).unwrap();
        let _ = enrich::fetch_for_test(&l, &mut lit, &lscratch, &mut None);
        assert_eq!(lit.url.as_deref(), Some("https://runtrellis.com/"), "{:?}", lit.url);
        assert!(lit.transcript.as_deref().unwrap().contains("ETL"));

        let mut it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.kind, "hn");
        let scratch = l.data_dir.join("scratch").join(&id);
        std::fs::create_dir_all(&scratch).unwrap();
        // NoBrowser, so the page half fails — the thread half must not.
        let _ = enrich::fetch_for_test(&l, &mut it, &scratch, &mut None);
        assert!(it.url.as_deref().unwrap().contains("apple.com"), "{:?}", it.url);
        assert_eq!(it.meta["hn"]["url"], json!("https://news.ycombinator.com/item?id=49426564"));
        assert!(it.meta["hn"]["comments"].as_i64().unwrap() > 10);
        assert!(it.transcript.as_deref().unwrap().len() > 200);
        assert!(!it.title.contains("item"), "{}", it.title);
    }

    #[test]
    #[ignore]
    fn covers_come_back_from_open_library_and_wikipedia() {
        let l = lib();
        for (kind, name, want) in [
            ("book", "The Dispossessed", "Le Guin"),
            ("tv", "Breaking Bad", "Gilligan"),
            ("movie", "Dune Part Two", "Villeneuve"),
            ("game", "Hades II", "Supergiant"),
        ] {
            let id = l
                .dispatch("capturePlan", json!({ "plan": { "entries": [{ "kind": kind, "text": name }] } }))
                .unwrap()["ids"][0]
                .as_str()
                .unwrap()
                .to_string();
            let mut it = l.db.lock().get_item(&id).unwrap().unwrap();
            let scratch = l.data_dir.join("scratch").join(&id);
            std::fs::create_dir_all(&scratch).unwrap();
            let mut shot = None;
            enrich::fetch_for_test(&l, &mut it, &scratch, &mut shot).unwrap();
            assert!(it.thumb.is_some(), "{kind}: no cover");
            let url = it.url.clone().unwrap_or_default();
            assert!(!url.is_empty(), "{kind}: no page");
            // A film goes to IMDb, not to the encyclopedia that identified it.
            if matches!(kind, "movie" | "tv") {
                assert!(url.contains("imdb.com/title/tt"), "{kind}: {url}");
                assert!(it.meta["wikipedia"].is_string(), "{kind}: lost the article");
            }
            let text = format!("{} {}", it.summary.clone().unwrap_or_default(), it.meta);
            assert!(text.contains(want), "{kind}: {text}");
        }
    }

    #[test]
    fn saying_they_are_books_makes_them_books() {
        let l = lib();
        let ids: Vec<String> = serde_json::from_value(
            l.dispatch("capturePlan", json!({ "plan": { "entries": [
                { "kind": "snippet", "text": "Formulations of the Secular" },
                { "kind": "snippet", "text": "Politics of Piety" }
            ] } })).unwrap()["ids"].clone(),
        )
        .unwrap();
        assert_eq!(l.db.lock().get_item(&ids[0]).unwrap().unwrap().kind, "snippet");
        l.dispatch("annotate", json!({ "ids": ids, "note": "these are all books I want to read" })).unwrap();
        let it = l.db.lock().get_item(&ids[0]).unwrap().unwrap();
        assert_eq!(it.kind, "book");
        assert_eq!(it.status, "pending");
        assert_eq!(it.meta["query"], json!("Formulations of the Secular"));
    }

    #[test]
    fn a_sticky_is_an_item_that_points_at_other_items() {
        let l = lib();
        seed::seed(&l).unwrap();
        let film = l.capture(CaptureInput { text: "https://example.com/film".into(), ..Default::default() }).unwrap();
        let id = l.dispatch("newNote", json!({ "title": "Karoo trip" })).unwrap().as_str().unwrap().to_string();
        let html = format!("<p>Watch this first: <a data-item=\"{film}\" href=\"#{film}\">The Gorge</a></p><ul><li>pack the tripod</li></ul>");
        l.dispatch("update", json!({ "id": id, "patch": { "bodyHtml": html } })).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.kind, "note");
        assert_eq!(it.folder_ids, vec![Library::NOTES_FOLDER], "a sticky lands in Notes");
        assert_eq!(it.meta["links"], json!([film]));
        assert_eq!(it.body_text.as_deref(), Some("Watch this first: The Gorge pack the tripod"));
        // and it is searchable like anything else
        assert!(l.db.lock().search("tripod").unwrap().contains(&id));
    }

    #[test]
    fn an_emptied_suggestion_removes_itself() {
        let l = lib();
        seed::seed(&l).unwrap();
        let id = l.capture(CaptureInput { text: "https://example.com/x".into(), ..Default::default() }).unwrap();
        l.apply_agent_result(&id, &json!({ "folder": { "suggest": { "name": "Want to buy", "emoji": "🛒" } }, "confidence": 0.9 })).unwrap();
        let sug = l.db.lock().all_folders().unwrap().into_iter().find(|f| f.name == "Want to buy").unwrap();
        assert!(sug.proposed);

        // A suggestion nobody has taken up survives; the item still points at it.
        l.prune_proposed();
        assert!(l.db.lock().all_folders().unwrap().iter().any(|f| f.id == sug.id));

        // Filed into it, then dragged out into another folder: nothing is left.
        l.dispatch("addToFolder", json!({ "id": id, "folderId": sug.id })).unwrap();
        let travel = l.db.lock().all_folders().unwrap().into_iter().find(|f| f.name == "Travel").unwrap().id;
        l.dispatch("addToFolder", json!({ "id": id, "folderId": travel, "from": sug.id })).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.folder_ids, vec![travel.clone()]);
        // ...but "Want to buy" was accepted when it was filed into, so it stays.
        assert!(l.db.lock().all_folders().unwrap().iter().any(|f| f.id == sug.id));

        // One that was only ever suggested goes when the item is filed by hand.
        let id2 = l.capture(CaptureInput { text: "https://example.com/y".into(), ..Default::default() }).unwrap();
        l.apply_agent_result(&id2, &json!({ "folder": { "suggest": { "name": "Shopping" } }, "confidence": 0.4 })).unwrap();
        let shop = l.db.lock().all_folders().unwrap().into_iter().find(|f| f.name == "Shopping").unwrap().id;
        l.dispatch("addToFolder", json!({ "id": id2, "folderId": travel })).unwrap();
        assert!(!l.db.lock().all_folders().unwrap().iter().any(|f| f.id == shop), "the stale suggestion should be gone");
    }

    #[test]
    fn agent_result_respects_user_edits_and_threshold() {
        let l = lib();
        seed::seed(&l).unwrap();
        let id = l.capture(CaptureInput { text: "https://example.com/x".into(), ..Default::default() }).unwrap();
        l.dispatch("update", json!({ "id": id, "patch": { "title": "Mine" } })).unwrap();
        let folders = l.db.lock().all_folders().unwrap();
        let travel = folders.iter().find(|f| f.name == "Travel").unwrap().id.clone();
        l.apply_agent_result(&id, &json!({ "title": "Agent title", "tags": ["Travel", "za"], "folder": { "existingId": travel }, "confidence": 0.9, "reason": "r" })).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.title, "Mine");
        assert_eq!(it.tags, vec!["travel", "za"]);
        assert_eq!(it.folder_ids, vec![travel.clone()]);

        // Low confidence → suggestion, not filing.
        let id2 = l.capture(CaptureInput { text: "https://example.com/y".into(), ..Default::default() }).unwrap();
        l.apply_agent_result(&id2, &json!({ "folder": { "existingId": travel }, "confidence": 0.3 })).unwrap();
        let it2 = l.db.lock().get_item(&id2).unwrap().unwrap();
        assert!(it2.folder_ids.is_empty());
        assert_eq!(it2.meta["suggestedFolderId"], json!(travel));

        // A suggested new folder is created as proposed; filing into it accepts it.
        l.apply_agent_result(&id2, &json!({ "folder": { "suggest": { "name": "South Africa", "parentId": travel, "emoji": "🇿🇦" } } })).unwrap();
        let f = l.db.lock().all_folders().unwrap();
        let za = f.iter().find(|f| f.name == "South Africa").unwrap();
        assert!(za.proposed);
        l.dispatch("addToFolder", json!({ "id": id2, "folderId": za.id })).unwrap();
        assert!(!l.db.lock().all_folders().unwrap().iter().find(|f| f.name == "South Africa").unwrap().proposed);
    }

    #[test]
    fn paste_anything() {
        let l = lib();
        // Two links in one paste → two items, first id back.
        let first = l.capture(CaptureInput { text: "see youtube.com/watch?v=aaa and https://github.com/a/b".into(), ..Default::default() }).unwrap();
        let items = l.snapshot().unwrap().items;
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.id == first && i.kind == "youtube_video"));
        assert!(items.iter().any(|i| i.kind == "github_repo"));
        // Prose around one link becomes the note.
        let id = l.capture(CaptureInput { text: "great talk on procrastination: https://youtu.be/arj7oStGLkU".into(), ..Default::default() }).unwrap();
        assert_eq!(l.db.lock().get_item(&id).unwrap().unwrap().notes.as_deref(), Some("great talk on procrastination"));
        // A share dialog's iframe counts as the link.
        let id = l.capture(CaptureInput { html: r#"<iframe src="https://www.youtube.com/embed/zzz"></iframe>"#.into(), ..Default::default() }).unwrap();
        assert_eq!(l.db.lock().get_item(&id).unwrap().unwrap().kind, "youtube_video");
        // Code is a snippet tagged code, untouched by HTML stripping.
        let id = l.capture(CaptureInput { text: "fn main() {\n    println!(\"hi\");\n}".into(), ..Default::default() }).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.auto_tags, vec!["code"]);
        assert!(it.body_text.unwrap().contains("println!"));
        // A file keeps its bytes and its kind.
        use base64::Engine;
        let pdf = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 fake");
        let id = l.capture(CaptureInput { file_base64: Some(pdf), file_name: Some("Paper.PDF".into()), ..Default::default() }).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.kind, "pdf");
        assert!(l.blob_path(it.meta["file"].as_str().unwrap()).exists());
    }

    #[test]
    fn two_screenshots_are_two_items() {
        let l = lib();
        use base64::Engine;
        let png = |b: &[u8]| Some(base64::engine::general_purpose::STANDARD.encode(b));
        let a = l.capture(CaptureInput { image_base64: png(b"\x89PNG\r\n\x1a\n one"), ..Default::default() }).unwrap();
        let b = l.capture(CaptureInput { image_base64: png(b"\x89PNG\r\n\x1a\n two"), ..Default::default() }).unwrap();
        assert_ne!(a, b, "two different screenshots must not collide on the empty-text key");
        // The same image twice is still one item.
        let again = l.capture(CaptureInput { image_base64: png(b"\x89PNG\r\n\x1a\n one"), ..Default::default() }).unwrap();
        assert_eq!(a, again);
    }

    /// The phone is a viewer (§7.3) — but a photo is the one thing it can
    /// finish on its own: the bytes are already here, so the tile knows its
    /// shape and size without waiting for the Mac. A JPEG used to come out
    /// `4:3` and "—", because only PNG headers were ever read.
    #[test]
    fn a_photo_is_finished_on_the_device_that_pasted_it() {
        let l = lib();
        use base64::Engine;
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        jpg.extend_from_slice(&3024u16.to_be_bytes());
        jpg.extend_from_slice(&4032u16.to_be_bytes());
        jpg.extend_from_slice(&[0; 8]);
        let id = l
            .capture(CaptureInput { image_base64: Some(base64::engine::general_purpose::STANDARD.encode(&jpg)), ..Default::default() })
            .unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.kind, "image");
        assert_eq!(it.dimensions, "4032×3024");
        assert!((it.aspect - 4032.0 / 3024.0).abs() < 1e-9, "aspect was {}", it.aspect);
        assert!(it.thumb.as_deref().unwrap().ends_with(".jpg"), "kept as a jpeg, not renamed png");
        // Nothing to fetch: no browser is ever needed for this one.
        assert_eq!(it.status, "ready");
    }

    #[test]
    fn tag_provenance() {
        let l = lib();
        let id = l.capture(CaptureInput { text: "https://en.wikipedia.org/wiki/Karoo".into(), ..Default::default() }).unwrap();
        let mut it = l.db.lock().get_item(&id).unwrap().unwrap();
        l.apply_auto_tags(&mut it);
        l.db.lock().save_item(&it).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.tags, vec!["article", "wikipedia"]);
        assert_eq!(it.auto_tags, it.tags);
        // Re-adding a machine tag makes it the person's; a new tag is theirs from the start.
        l.dispatch("addTag", json!({ "id": id, "tag": "wikipedia" })).unwrap();
        l.dispatch("addTag", json!({ "id": id, "tag": "Desert" })).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert_eq!(it.tags, vec!["article", "wikipedia", "desert"]);
        assert_eq!(it.auto_tags, vec!["article"]);
        // Removing drops it from both lists.
        l.dispatch("removeTag", json!({ "id": id, "tag": "article" })).unwrap();
        let it = l.db.lock().get_item(&id).unwrap().unwrap();
        assert!(it.auto_tags.is_empty());
        // The agent's tags count as machine tags too.
        l.apply_agent_result(&id, &json!({ "tags": ["semi-arid"] })).unwrap();
        assert_eq!(l.db.lock().get_item(&id).unwrap().unwrap().auto_tags, vec!["semi-arid"]);
    }

    #[test]
    fn search_hits_transcript() {
        let l = lib();
        let id = l.capture(CaptureInput { text: "https://example.com/v".into(), ..Default::default() }).unwrap();
        let mut it = l.db.lock().get_item(&id).unwrap().unwrap();
        it.transcript = Some("[0:01] we drove down to Cape Town at dawn".into());
        l.db.lock().save_item(&it).unwrap();
        assert_eq!(l.db.lock().search("cape town").unwrap(), vec![id.clone()]);
        assert_eq!(l.db.lock().search("durban").unwrap(), Vec::<String>::new());
    }
}
