//! iCloud sync — spec §7.4, done through iCloud Drive instead of the LAN.
//!
//! The SQLite file never leaves the machine: file-level sync of a live WAL
//! database corrupts it. Instead every device writes its *own* file,
//! `devices/<device-id>.json`, with every row it knows (tombstones included),
//! and reads everyone else's. No file is ever written by two devices, so
//! iCloud never has to resolve a conflict; membox does the merge itself,
//! last-writer-wins per row on `updated_at`.
//!
//! Thumbs are copied into `blobs/` content-addressed (a tile without its
//! thumb is blank); full-page shots and scratch dirs stay local.
//!
//! ponytail: each export rewrites the whole snapshot. Fine for thousands of
//! rows; switch to an append-only op log when a library makes the file slow.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Folder, Item};
use crate::Library;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFile {
    pub device: String,
    pub exported_at: String,
    pub items: Vec<Item>,
    pub folders: Vec<Folder>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub dir: Option<String>,
    pub last_export: Option<String>,
    pub last_import: Option<String>,
    pub devices: usize,
    pub imported: usize,
    pub error: Option<String>,
}

/// Where the Mac's copy lives. Prefer the app's own iCloud container (what
/// the signed iOS app uses), else a plain folder in iCloud Drive. The dev
/// identity (data dir ending in `.dev`) gets its own folder so playing
/// around never touches the real library.
#[cfg(target_os = "macos")]
pub fn default_dir(data_dir: &Path) -> Option<PathBuf> {
    let dev = data_dir.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(".dev")).unwrap_or(false);
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let mobile = home.join("Library/Mobile Documents");
    if !mobile.exists() {
        return None;
    }
    let container = mobile.join("iCloud~com~membox/Documents");
    if container.exists() {
        return Some(if dev { container.join("dev") } else { container });
    }
    let drive = mobile.join("com~apple~CloudDocs");
    drive.exists().then(|| drive.join(if dev { "membox-dev" } else { "membox" }))
}

#[cfg(not(target_os = "macos"))]
pub fn default_dir(_: &Path) -> Option<PathBuf> {
    None
}

impl Library {
    pub fn device_id(&self) -> String {
        let p = self.data_dir.join("device-id");
        if let Ok(s) = std::fs::read_to_string(&p) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        let id = crate::model::new_id("d");
        let _ = std::fs::write(&p, &id);
        id
    }

    pub(crate) fn sync_dir(&self) -> Option<PathBuf> {
        let s = self.settings();
        if !s.sync_enabled {
            return None;
        }
        s.sync_dir.map(PathBuf::from).or_else(|| default_dir(&self.data_dir))
    }

    /// Write this device's snapshot + missing thumbs. Cheap enough to run on
    /// every change (debounced by the caller).
    pub fn sync_export(&self) -> Result<(), String> {
        let Some(dir) = self.sync_dir() else { return Ok(()) };
        let devices = dir.join("devices");
        std::fs::create_dir_all(&devices).map_err(|e| format!("{}: {e}", devices.display()))?;
        let (items, folders) = {
            let db = self.db.lock();
            (db.all_items_for_sync().map_err(|e| e.to_string())?, db.all_folders_for_sync().map_err(|e| e.to_string())?)
        };
        // Thumbs only; content-addressed so a re-copy is a no-op.
        let blobs = dir.join("blobs");
        for rel in items.iter().filter_map(|i| i.thumb.as_deref()) {
            let dst = blobs.join(rel);
            if !dst.exists() {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(self.blob_path(rel), &dst);
            }
        }
        let file = DeviceFile { device: self.device_id(), exported_at: crate::model::now(), items, folders };
        let path = devices.join(format!("{}.json", file.device));
        let tmp = devices.join(format!("{}.json.tmp", file.device));
        std::fs::write(&tmp, serde_json::to_vec(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        let mut st = self.sync_status.lock();
        st.dir = Some(dir.display().to_string());
        st.last_export = Some(file.exported_at);
        st.error = None;
        Ok(())
    }

    /// Merge every other device's snapshot. Returns how many rows changed.
    pub fn sync_import(&self) -> Result<usize, String> {
        let Some(dir) = self.sync_dir() else { return Ok(0) };
        let devices = dir.join("devices");
        let me = self.device_id();
        let mut changed = 0;
        let mut seen = 0;
        let entries = match std::fs::read_dir(&devices) {
            Ok(e) => e,
            Err(_) => return Ok(0), // nothing synced yet
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|x| x != "json").unwrap_or(true) || path.file_stem().map(|s| s == me.as_str()).unwrap_or(false) {
                continue;
            }
            let Ok(raw) = std::fs::read(&path) else { continue };
            let Ok(file) = serde_json::from_slice::<DeviceFile>(&raw) else { continue };
            seen += 1;
            changed += self.merge(&file, &dir.join("blobs"))?;
        }
        let mut st = self.sync_status.lock();
        st.dir = Some(dir.display().to_string());
        st.last_import = Some(crate::model::now());
        st.devices = seen;
        st.imported += changed;
        drop(st);
        if changed > 0 {
            self.changed();
        }
        Ok(changed)
    }

    fn merge(&self, file: &DeviceFile, blobs: &Path) -> Result<usize, String> {
        let mut n = 0;
        for f in &file.folders {
            let db = self.db.lock();
            let local = db.get_folder_for_sync(&f.id).map_err(|e| e.to_string())?;
            if local.map(|l| l.updated_at < f.updated_at).unwrap_or(true) {
                db.upsert_folder_for_sync(f).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
        for it in &file.items {
            let local = self.db.lock().get_item_for_sync(&it.id).map_err(|e| e.to_string())?;
            // Whether the bytes are here is not a function of the row's
            // version. iCloud hands a phone the snapshot first and the blob
            // whenever the download finishes, so the copy is tried on every
            // pass — not only when the row itself is stale. Dropping the thumb
            // on the one pass that raced the download used to lose it for
            // good: the row then carried the exporter's own `updated_at`, and
            // the test below is a strict `<`, so it was never looked at again.
            let thumb = it.thumb.as_deref().filter(|rel| self.pull_blob(rel, blobs)).map(str::to_string);
            let stale = local.as_ref().map(|l| l.updated_at < it.updated_at).unwrap_or(true);
            if stale {
                // Bring the thumb along; page shots stay on the device that made them.
                let mut row = it.clone();
                row.thumb = thumb;
                if row.page_shot.as_ref().map(|p| !self.blob_path(p).exists()).unwrap_or(false) {
                    row.page_shot = None;
                }
                let waiting = !row.trashed && matches!(row.status.as_str(), "pending" | "fetching" | "enriching" | "queued");
                self.db.lock().upsert_item_for_sync(&row).map_err(|e| e.to_string())?;
                // Something another device could not finish — the phone has no
                // browser, so everything it captures arrives "waiting for a
                // browser" (§7.3). This is the device that can: put it on the
                // queue, or it sits pending forever with nobody looking at it.
                if waiting && self.browser.available() {
                    log::info!("sync: taking over {} ({})", row.id, row.status);
                    self.queue_fetch(&row.id);
                }
                n += 1;
            } else if let Some(mut row) = local.filter(|l| l.thumb.is_none() && thumb.is_some()) {
                // The row is ours or a tie — only the picture was missing, so
                // fill that in and leave every other column alone. Overwriting
                // with `it` here would undo a newer edit made on this device.
                row.thumb = thumb;
                self.db.lock().upsert_item_for_sync(&row).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Make `rel` local if it is not already; true when the bytes are here.
    fn pull_blob(&self, rel: &str, blobs: &Path) -> bool {
        let dst = self.blob_path(rel);
        if dst.exists() {
            return true;
        }
        let src = blobs.join(rel);
        if !src.exists() {
            return false;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(&src, &dst).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::browser::NoBrowser;
    use crate::model::CaptureInput;
    use crate::Library;
    use serde_json::json;
    use std::sync::Arc;

    /// Cannot actually fetch, but says a browser is here — enough to make this
    /// library the one that takes work off the queue. Records what it was asked
    /// to load so a test can prove the hand-off happened.
    struct RecordingBrowser(Arc<parking_lot::Mutex<Vec<String>>>);
    impl crate::browser::Browser for RecordingBrowser {
        fn navigate(&self, url: &str) -> crate::browser::Result<()> {
            self.0.lock().push(url.to_string());
            Err(crate::browser::BrowserError("recorded".into()))
        }
        fn eval(&self, _: &str) -> crate::browser::Result<String> {
            Err(crate::browser::BrowserError("recorded".into()))
        }
        fn snapshot_png(&self, _: bool) -> crate::browser::Result<Vec<u8>> {
            Err(crate::browser::BrowserError("recorded".into()))
        }
    }

    fn lib(sync: &std::path::Path) -> Arc<Library> {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::open(dir.path(), Arc::new(NoBrowser)).unwrap();
        l.dispatch("settings", json!({ "syncEnabled": true, "syncDir": sync.display().to_string() })).unwrap();
        std::mem::forget(dir);
        l
    }

    #[test]
    fn two_devices_converge_through_a_shared_folder() {
        let shared = tempfile::tempdir().unwrap();
        let a = lib(shared.path());
        let b = lib(shared.path());
        assert_ne!(a.device_id(), b.device_id());

        // A captures, B sees it.
        let id = a.capture(CaptureInput { text: "a note from A".into(), ..Default::default() }).unwrap();
        a.sync_export().unwrap();
        assert_eq!(b.sync_import().unwrap(), 1);
        assert_eq!(b.db.lock().get_item(&id).unwrap().unwrap().title, "a note from A");

        // B edits later, A takes B's version.
        std::thread::sleep(std::time::Duration::from_millis(5));
        b.dispatch("update", json!({ "id": id, "patch": { "title": "renamed on B" } })).unwrap();
        b.sync_export().unwrap();
        assert!(a.sync_import().unwrap() >= 1);
        assert_eq!(a.db.lock().get_item(&id).unwrap().unwrap().title, "renamed on B");

        // A's stale export must not undo B's edit.
        a.sync_export().unwrap();
        b.sync_import().unwrap();
        assert_eq!(b.db.lock().get_item(&id).unwrap().unwrap().title, "renamed on B");

        // Deleting on A propagates as a tombstone.
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.dispatch("deleteForever", json!({ "ids": [id] })).unwrap();
        a.sync_export().unwrap();
        b.sync_import().unwrap();
        assert!(b.db.lock().get_item(&id).unwrap().is_none());
        assert_eq!(b.snapshot().unwrap().items.len(), 0);

        // Folders travel too.
        let fid = b.create_folder("From B", None, Some("🅱️"), false).unwrap();
        b.sync_export().unwrap();
        a.sync_import().unwrap();
        assert!(a.db.lock().all_folders().unwrap().iter().any(|f| f.id == fid));
    }

    /// A link saved on the phone must not wait for the Mac to be restarted.
    ///
    /// The phone has no browser (§7.3), so it captures the URL as `pending` and
    /// sends it on. `open` re-queues mid-flight rows for a *previous* run, but
    /// a desktop that is already running used to merely write the row — nobody
    /// ever looked at it again.
    #[test]
    fn a_link_the_phone_could_not_fetch_is_taken_over_by_the_desktop() {
        let shared = tempfile::tempdir().unwrap();
        let phone = lib(shared.path()); // NoBrowser, like the real thing

        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let dir = tempfile::tempdir().unwrap();
        let desk = Library::open(dir.path(), Arc::new(RecordingBrowser(seen.clone()))).unwrap();
        desk.dispatch("settings", json!({ "syncEnabled": true, "syncDir": shared.path().display().to_string() })).unwrap();
        std::mem::forget(dir);

        let id = phone
            .capture(CaptureInput { text: "https://example.com/saved-on-my-phone".into(), ..Default::default() })
            .unwrap();
        assert_eq!(phone.db.lock().get_item(&id).unwrap().unwrap().status, "pending");

        phone.sync_export().unwrap();
        assert_eq!(desk.sync_import().unwrap(), 1);

        // The desktop's worker should pick it up on its own.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if seen.lock().iter().any(|u| u.contains("saved-on-my-phone")) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the desktop never tried to fetch the phone's link: {:?}", seen.lock());
    }

    /// A thumb that iCloud has not materialised yet must not be lost for good.
    ///
    /// On a phone the shared `blobs/` arrive as placeholders, so at merge time
    /// `src.exists()` is false and the row is written with `thumb = None`. The
    /// file lands seconds later — but the row now carries the *same*
    /// `updated_at` as the exporter's, and the merge test is a strict `<`, so
    /// it is never reconsidered and the tile stays blank for good.
    #[test]
    fn a_thumb_that_arrives_late_still_reaches_the_other_device() {
        let shared = tempfile::tempdir().unwrap();
        let a = lib(shared.path());
        let b = lib(shared.path());

        let png = {
            use base64::Engine;
            let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
            v.extend_from_slice(&[7u8; 64]);
            base64::engine::general_purpose::STANDARD.encode(v)
        };
        let id = a
            .capture(CaptureInput { image_base64: Some(png), ..Default::default() })
            .unwrap();
        let rel = a.db.lock().get_item(&id).unwrap().unwrap().thumb.unwrap();

        a.sync_export().unwrap();
        assert!(shared.path().join("blobs").join(&rel).exists(), "export publishes the thumb");

        // iCloud has not handed the phone the bytes yet: the name is there in
        // the snapshot, the file is not.
        std::fs::remove_file(shared.path().join("blobs").join(&rel)).unwrap();
        assert_eq!(b.sync_import().unwrap(), 1);
        assert_eq!(b.db.lock().get_item(&id).unwrap().unwrap().thumb, None, "nothing to copy yet");

        // …and now it lands, exactly as startDownloadingUbiquitousItem finishes.
        let dst = shared.path().join("blobs").join(&rel);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(a.blob_path(&rel), &dst).unwrap();

        b.sync_import().unwrap();
        assert_eq!(
            b.db.lock().get_item(&id).unwrap().unwrap().thumb,
            Some(rel),
            "the thumb must come back once iCloud materialises it"
        );
    }
}
