//! The embedded browser — spec §4.1–4.3. A hidden Tauri `WebviewWindow` whose
//! native WKWebView we drive directly: `evaluateJavaScript` for the readable
//! extraction and `takeSnapshot` for the screenshots. One browser, one cookie
//! jar; the agent never spawns Chrome.
//!
//! Every native call is marshalled to the main thread by `with_webview`; the
//! calling (worker) thread blocks on a channel for the completion block.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use membox_core::browser::{Browser, BrowserError, Result};
use tauri::{AppHandle, Manager, WebviewWindow};

pub const WINDOW: &str = "browser";
const VIEWPORT: (f64, f64) = (1280.0, 800.0);
const MAX_PAGE_HEIGHT: f64 = 8000.0; // ponytail: taller pages get cut; tile+stitch if it matters
/// The tile at rest is the viewport shot, and a site that fades its hero in
/// paints nothing on the first frame — the person only sees the page once they
/// hover and the full-page shot arrives. So the resting frame is taken about a
/// second in, not at first paint (§4.5).
const FIRST_FRAME_SETTLE: Duration = Duration::from_millis(1200);

/// Alpha 0 on the NSWindow: on screen for WebKit, unseen by the person.
/// (`takeSnapshot` renders the web content layer, not the window, so the
/// capture is unaffected.)
///
/// Unseen was not unheard. The window is a real, on-screen WebKit view, so a
/// YouTube watch page did what a watch page does — started playing, out of a
/// window nobody could see or pause. Same for any autoplaying hero video.
/// `setAllMediaPlaybackSuspended` blocks playback *and* every later attempt by
/// the page to start it, so this holds across navigations and only has to be
/// set once. Nobody watches anything in this window; it exists to be read and
/// photographed.
pub fn make_invisible(w: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    if let Ok(ptr) = w.ns_window() {
        let win: &objc2_app_kit::NSWindow = unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
        win.setAlphaValue(0.0);
        win.setHasShadow(false);
    }
    #[cfg(target_os = "macos")]
    let _ = w.with_webview(|pw| mac::silence(pw.inner()));
    #[cfg(not(target_os = "macos"))]
    let _ = w;
}

pub struct TauriBrowser {
    app: AppHandle,
}

impl TauriBrowser {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn window(&self) -> Result<WebviewWindow> {
        self.app.get_webview_window(WINDOW).ok_or_else(|| BrowserError("browser window missing".into()))
    }

    fn eval_raw(&self, js: &str) -> Result<String> {
        let w = self.window()?;
        let (tx, rx) = mpsc::channel::<std::result::Result<String, String>>();
        let js = js.to_string();
        w.with_webview(move |pw| {
            #[cfg(target_os = "macos")]
            mac::eval(pw.inner(), &js, tx);
            #[cfg(not(target_os = "macos"))]
            let _ = tx.send(Err("unsupported platform".into()));
        })
        .map_err(|e| BrowserError(e.to_string()))?;
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| BrowserError("eval timed out".into()))?
            .map_err(BrowserError)
    }
}

impl Browser for TauriBrowser {
    fn navigate(&self, url: &str) -> Result<()> {
        let w = self.window()?;
        let u = url::Url::parse(url).map_err(|e| BrowserError(e.to_string()))?;
        // A mark on the *current* document: WKWebView keeps it when the new
        // navigation never commits (bad host, refused connection), and the old
        // page is `complete` already — without this we'd screenshot it.
        let _ = self.eval("(window.__mbNav = 1, true)");
        w.navigate(u).map_err(|e| BrowserError(e.to_string()))?;
        // Wait for the document, then a short settle for late-rendering pages.
        let deadline = Instant::now() + Duration::from_secs(25);
        // A host that resolves has answered long before this; one that doesn't
        // never will.
        let commit_by = Instant::now() + Duration::from_secs(10);
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let fresh = !matches!(self.eval_raw("String(window.__mbNav || 0)"), Ok(ref s) if s.contains('1'));
            if !fresh && Instant::now() > commit_by {
                return Err(BrowserError(format!("could not load {url}")));
            }
            match self.eval_raw("document.readyState") {
                Ok(s) if fresh && s.contains("complete") => break,
                _ if Instant::now() > deadline => return Err(BrowserError("page load timed out".into())),
                _ => {}
            }
        }
        // `complete` is when the HTML arrived, not when the app rendered. Wait
        // for the text on the page to stop changing (SPAs, lazy sections),
        // then sweep one screen down and back so intersection-driven content
        // fires before the first screenshot.
        let mut last = String::new();
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(350));
            let now = self.eval("String((document.body && document.body.innerText || '').length) + ':' + document.images.length").unwrap_or_default();
            if now == last && now != "0:0" {
                break;
            }
            last = now;
        }
        let _ = self.eval("(window.scrollTo(0, Math.min(window.innerHeight, 900)), true)");
        std::thread::sleep(Duration::from_millis(250));
        let _ = self.eval("(window.scrollTo(0,0), true)");
        std::thread::sleep(FIRST_FRAME_SETTLE);
        Ok(())
    }

    fn eval(&self, js: &str) -> Result<String> {
        // Normalise to a string result so the core never sees an NSObject.
        let wrapped = format!(
            "(function(){{try{{const r=({js});return typeof r==='string'?r:JSON.stringify(r===undefined?null:r)}}catch(e){{return 'ERR:'+e}}}})()"
        );
        let out = self.eval_raw(&wrapped)?;
        if let Some(e) = out.strip_prefix("ERR:") {
            return Err(BrowserError(e.into()));
        }
        Ok(out)
    }

    fn snapshot_png(&self, full_page: bool) -> Result<Vec<u8>> {
        let w = self.window()?;
        let mut restore = None;
        if full_page {
            let h: f64 = self
                .eval("Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0)")?
                .parse()
                .unwrap_or(VIEWPORT.1);
            let h = h.clamp(VIEWPORT.1, MAX_PAGE_HEIGHT);
            w.set_size(tauri::LogicalSize::new(VIEWPORT.0, h)).map_err(|e| BrowserError(e.to_string()))?;
            restore = Some(VIEWPORT);
            // The newly exposed area is not painted yet, and lazy content below
            // the first viewport has never been on screen. Sweep the page so
            // observers fire and tiles get painted, then let it settle.
            for y in (0..h as i64).step_by(700) {
                let _ = self.eval(&format!("(window.scrollTo(0,{y}), true)"));
                std::thread::sleep(Duration::from_millis(120));
            }
            let _ = self.eval("(window.scrollTo(0,0), true)");
            std::thread::sleep(Duration::from_millis(900));
        }
        let (tx, rx) = mpsc::channel::<std::result::Result<Vec<u8>, String>>();
        w.with_webview(move |pw| {
            #[cfg(target_os = "macos")]
            mac::snapshot(pw.inner(), tx);
            #[cfg(not(target_os = "macos"))]
            let _ = tx.send(Err("unsupported platform".into()));
        })
        .map_err(|e| BrowserError(e.to_string()))?;
        let png = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| BrowserError("snapshot timed out".into()))?;
        if let Some((cw, ch)) = restore {
            let _ = w.set_size(tauri::LogicalSize::new(cw, ch));
        }
        png.map_err(BrowserError)
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSString};
    use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

    pub fn eval(ptr: *mut c_void, js: &str, tx: Sender<Result<String, String>>) {
        let wv: &WKWebView = unsafe { &*(ptr as *const WKWebView) };
        let block = RcBlock::new(move |obj: *mut AnyObject, err: *mut NSError| {
            let r = if !err.is_null() {
                Err(unsafe { &*err }.localizedDescription().to_string())
            } else if obj.is_null() {
                Ok("null".to_string())
            } else {
                // The wrapper JS always returns a string; anything else is described.
                let o = unsafe { &*obj };
                match o.downcast_ref::<NSString>() {
                    Some(s) => Ok(s.to_string()),
                    None => Ok(format!("{o:?}")),
                }
            };
            let _ = tx.send(r);
        });
        unsafe { wv.evaluateJavaScript_completionHandler(&NSString::from_str(js), Some(&block)) };
    }

    /// Suspend media for the life of the webview — see `make_invisible`.
    pub fn silence(ptr: *mut c_void) {
        let wv: &WKWebView = unsafe { &*(ptr as *const WKWebView) };
        unsafe { wv.setAllMediaPlaybackSuspended_completionHandler(true, None) };
    }

    pub fn snapshot(ptr: *mut c_void, tx: Sender<Result<Vec<u8>, String>>) {
        let wv: &WKWebView = unsafe { &*(ptr as *const WKWebView) };
        let block = RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
            let r = (|| {
                if !err.is_null() {
                    return Err(unsafe { &*err }.localizedDescription().to_string());
                }
                if img.is_null() {
                    return Err("no image".into());
                }
                let img = unsafe { &*img };
                let tiff = img.TIFFRepresentation().ok_or("no tiff")?;
                let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("no bitmap rep")?;
                let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }.ok_or("no png")?;
                Ok(png.to_vec())
            })();
            let _ = tx.send(r);
        });
        // afterScreenUpdates: wait for pending layout/paint before capturing.
        // with_webview runs this on the main thread.
        let mtm = unsafe { objc2::MainThreadMarker::new_unchecked() };
        let cfg = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe { cfg.setAfterScreenUpdates(true) };
        unsafe { wv.takeSnapshotWithConfiguration_completionHandler(Some(&cfg), &block) };
    }
}
