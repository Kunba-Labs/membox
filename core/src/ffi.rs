//! The UniFFI surface for iOS — spec §7.1. Deliberately tiny: JSON in, JSON
//! out, the same `dispatch` vocabulary the desktop uses. Swift decodes what it
//! needs with `Codable`.
//!
//! The phone supplies its own browser (a WKWebView) through [`HostBrowser`],
//! so capture on iOS gets a real screenshot and readable text without waiting
//! for the desktop.

use std::sync::Arc;

use crate::browser::{Browser, BrowserError, NoBrowser};
use crate::Library;

#[uniffi::export(callback_interface)]
pub trait ChangeListener: Send + Sync {
    fn on_change(&self);
}

/// Implemented in Swift over a WKWebView. Each call may block the core's
/// worker thread; the Swift side hops to the main queue and waits.
#[uniffi::export(callback_interface)]
pub trait HostBrowser: Send + Sync {
    fn navigate(&self, url: String) -> Result<(), CoreError>;
    fn eval(&self, js: String) -> Result<String, CoreError>;
    fn snapshot_png(&self, full_page: bool) -> Result<Vec<u8>, CoreError>;
}

struct CallbackBrowser(Box<dyn HostBrowser>);

impl Browser for CallbackBrowser {
    fn navigate(&self, url: &str) -> crate::browser::Result<()> {
        self.0.navigate(url.into()).map_err(|e| BrowserError(e.to_string()))
    }
    fn eval(&self, js: &str) -> crate::browser::Result<String> {
        self.0.eval(js.into()).map_err(|e| BrowserError(e.to_string()))
    }
    fn snapshot_png(&self, full_page: bool) -> crate::browser::Result<Vec<u8>> {
        self.0.snapshot_png(full_page).map_err(|e| BrowserError(e.to_string()))
    }
}

#[derive(uniffi::Object)]
pub struct MemboxCore {
    lib: Arc<Library>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl From<String> for CoreError {
    fn from(msg: String) -> Self {
        CoreError::Failed { msg }
    }
}

#[uniffi::export]
impl MemboxCore {
    /// `browser` may be nil (e.g. in the share extension); URLs then wait for a
    /// host with one.
    #[uniffi::constructor]
    pub fn open(data_dir: String, browser: Option<Box<dyn HostBrowser>>) -> Result<Arc<Self>, CoreError> {
        let b: Arc<dyn Browser> = match browser {
            Some(b) => Arc::new(CallbackBrowser(b)),
            None => Arc::new(NoBrowser),
        };
        let lib = Library::open(std::path::Path::new(&data_dir), b)?;
        crate::seed::seed(&lib)?;
        Ok(Arc::new(Self { lib }))
    }

    pub fn set_listener(&self, listener: Box<dyn ChangeListener>) {
        self.lib.on_change(Box::new(move || listener.on_change()));
    }

    /// Whole-library JSON (items, folders, settings, queue).
    pub fn snapshot_json(&self) -> Result<String, CoreError> {
        let s = self.lib.snapshot()?;
        serde_json::to_string(&s).map_err(|e| CoreError::Failed { msg: e.to_string() })
    }

    /// Same action names as the desktop store. Returns the result as JSON.
    pub fn dispatch(&self, action: String, args_json: String) -> Result<String, CoreError> {
        let args: serde_json::Value = serde_json::from_str(&args_json).map_err(|e| CoreError::Failed { msg: e.to_string() })?;
        let v = self.lib.dispatch(&action, args)?;
        Ok(v.to_string())
    }

    /// Absolute path of a blob, for `UIImage(contentsOfFile:)`.
    pub fn blob_path(&self, rel: String) -> String {
        self.lib.blob_path(&rel).display().to_string()
    }
}
