//! The embedded browser the host supplies — spec §4. Desktop implements it over
//! Tauri's hidden WebviewWindow (a real WKWebView); a host without one returns
//! [`NoBrowser`] and items stay `pending_desktop` until a browser sees them.

use std::fmt;

#[derive(Debug)]
pub struct BrowserError(pub String);

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BrowserError {}

pub type Result<T> = std::result::Result<T, BrowserError>;

pub trait Browser: Send + Sync {
    /// Load a URL and block until the document is interactive (or times out).
    fn navigate(&self, url: &str) -> Result<()>;
    /// Evaluate JS in the page and return the JSON-encoded result.
    fn eval(&self, js: &str) -> Result<String>;
    /// PNG of the viewport, or of the whole document when `full_page`.
    fn snapshot_png(&self, full_page: bool) -> Result<Vec<u8>>;
    fn available(&self) -> bool {
        true
    }
}

pub struct NoBrowser;

impl Browser for NoBrowser {
    fn navigate(&self, _: &str) -> Result<()> {
        Err(BrowserError("no browser on this host".into()))
    }
    fn eval(&self, _: &str) -> Result<String> {
        Err(BrowserError("no browser on this host".into()))
    }
    fn snapshot_png(&self, _: bool) -> Result<Vec<u8>> {
        Err(BrowserError("no browser on this host".into()))
    }
    fn available(&self) -> bool {
        false
    }
}

/// JS the fetch stage runs in the loaded page — §3.1. Title, description, the
/// OpenGraph image, the readable text and the document height, as one JSON
/// object.
///
/// ponytail: `innerText` of the main/article element (or body) with nav/footer
/// removed stands in for Readability.js. Vendor Readability when this misses
/// too much; the call site is one line.
pub const EXTRACT_JS: &str = r#"(() => {
  const meta = (n) => (document.querySelector(`meta[property="${n}"],meta[name="${n}"]`) || {}).content || null;
  const root = document.querySelector("article, main, [role=main]") || document.body || document.documentElement;
  const clone = root ? root.cloneNode(true) : document.createElement("div");
  clone.querySelectorAll("script,style,nav,footer,header,aside,noscript,iframe,svg,form,[aria-hidden=true]").forEach(e => e.remove());
  const text = (clone.innerText || "").replace(/[ \t]+/g, " ").replace(/\n{3,}/g, "\n\n").trim();
  return JSON.stringify({
    title: document.title || meta("og:title"),
    description: meta("description") || meta("og:description"),
    image: meta("og:image") || meta("twitter:image"),
    siteName: meta("og:site_name"),
    keywords: meta("keywords") || meta("news_keywords") || meta("article:tag"),
    text: text.slice(0, 60000),
    height: Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0),
    links: [...document.querySelectorAll("a[href]")].map(a => a.href).filter(h => h.startsWith("http")).slice(0, 200),
    readyState: document.readyState
  });
})()"#;

/// The cookie banner goes before the picture is taken — §4.5. Nearly every
/// banner on the web is one of a few hundred consent managers, and DuckDuckGo
/// already keeps the rules for clicking them away: `autoconsent`, vendored
/// unmodified under `core/vendor/autoconsent` (MPL-2.0). Its bundle expects
/// an extension host to answer its `init` message; this shim is that host,
/// in-page, with `isMainWorld` so the rule snippets run inline and nothing
/// crosses a bridge. It presses *reject* (`optOut`), never accept: the one
/// choice it is fair to make for the person. One cookie jar, so a site
/// dismissed once stays dismissed on every later save.
///
/// The host registers this as a start-of-document script *in every frame*
/// (`initialization_script_for_all_frames`): a Sourcepoint or Didomi banner
/// is a cross-origin iframe the top frame cannot see into, and the rule for
/// it runs there — the same reason the extension runs in all frames. Each
/// frame answers its own `init`; the top frame keeps the lifecycle in
/// `__mbConsent`, and a child frame posts its own up, so `dismiss_consent`
/// can wait for `done` from whichever frame held the banner.
/// ponytail: ~400KB parsed per frame; if ad-heavy pages feel it, gate the
/// bundle on a cheap CMP sniff first.
pub fn consent_script() -> &'static str {
    static JS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        format!(
            r#"(() => {{
  const top = window === window.top;
  if (top) {{
    window.__mbConsent = 'loading';
    window.__mbConsentFrame = '';
    window.addEventListener('message', (e) => {{
      if (e.data && typeof e.data.__mbConsent === 'string') window.__mbConsentFrame = e.data.__mbConsent;
    }});
  }}
  window.autoconsentSendMessage = (m) => {{
    if (m.type === 'init') {{
      // `autoconsentReceiveMessage` is defined after the constructor that sends `init` returns.
      setTimeout(() => window.autoconsentReceiveMessage({{ type: 'initResp',
        config: {{ autoAction: 'optOut', isMainWorld: true, logs: {{ errors: false }} }},
        rules: {{ compact: {rules} }} }}), 0);
    }} else if (m.type === 'report' || m.type === 'autoconsentError') {{
      const state = m.type === 'report' ? m.state.lifecycle : 'error';
      if (top) window.__mbConsent = state;
      else try {{ window.top.postMessage({{ __mbConsent: state }}, '*'); }} catch (_) {{}}
    }}
  }};
  {bundle}
}})();"#,
            rules = include_str!("../vendor/autoconsent/compact-rules.json"),
            bundle = include_str!("../vendor/autoconsent/autoconsent.playwright.js"),
        )
    });
    &JS
}

/// Wait for autoconsent to finish on the loaded page, a few seconds at most.
/// Returns `top/frame` lifecycles (`done`, `nothingDetected`, `error`, or
/// wherever they were at the deadline) for the log. A host without the
/// script registered reports `none`.
pub fn dismiss_consent(b: &dyn Browser) -> String {
    let started = std::time::Instant::now();
    loop {
        let state = b.eval("(window.__mbConsent || 'none') + '/' + (window.__mbConsentFrame || '')").unwrap_or_default();
        let (top, frame) = state.split_once('/').unwrap_or((state.as_str(), ""));
        if top == "done" || frame == "done" {
            // The banner is clicked; let its fade-out finish before the shot.
            std::thread::sleep(std::time::Duration::from_millis(500));
            return state;
        }
        // No banner in the top frame and no child frame mid-way: finished.
        let frame_settled = frame.is_empty() || frame == "nothingDetected" || frame == "error";
        // A banner found but not yet clicked (Sourcepoint's iframe takes a
        // while) is worth waiting for. Nothing found yet is not: `navigate`
        // already waited for the page to settle, so a banner that exists is
        // on screen by now — detection keeps retrying for ten seconds from
        // document start (a gate the page injects late is still clicked),
        // but the shot does not wait for it to give up.
        let busy = !frame_settled || top == "cmpDetected" || top == "openPopupDetected" || top == "runningOptOut";
        let deadline = std::time::Duration::from_millis(if busy { 8000 } else { 1500 });
        if top == "none" || top == "error" || (top == "nothingDetected" && frame_settled) || started.elapsed() > deadline {
            return state;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
