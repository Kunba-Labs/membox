// membox desktop shell. The main window is transparent with the native macOS
// vibrancy material behind it (docs/Feature-Spec.md §6.1); a second, hidden
// window is the embedded browser the enrichment pipeline and the agent drive.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browser;

use std::sync::Arc;

use membox_core::Library;
use serde_json::Value;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Core(Arc<Library>);

/// Async on purpose: a sync `#[tauri::command]` runs on the main thread, and
/// `plan` can sit there for ten seconds waiting on an agent CLI — which is the
/// whole window frozen, cursor and all. Off to the blocking pool it goes.
#[tauri::command]
async fn dispatch(core: State<'_, Core>, action: String, args: Value) -> Result<Value, String> {
    let lib = core.0.clone();
    tauri::async_runtime::spawn_blocking(move || lib.dispatch(&action, args))
        .await
        .map_err(|e| e.to_string())?
}

/// WKWebView swallows `window.open` / `target=_blank`; the default browser is a
/// `open` away. ponytail: macOS only, tauri-plugin-opener if this ever ships elsewhere.
#[tauri::command]
fn open_url(url: String) {
    if url.starts_with("http://") || url.starts_with("https://") {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

#[tauri::command]
fn blobs_dir(core: State<Core>) -> String {
    core.0.blobs.display().to_string()
}

#[tauri::command]
fn show_browser(app: tauri::AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window(browser::WINDOW) {
        let _ = if visible { w.show() } else { w.hide() };
    }
}

/// A GUI app does not get your shell's PATH. Launched from Finder or the Dock,
/// membox inherits launchd's — `/usr/bin:/bin:/usr/sbin:/sbin` — and Homebrew,
/// mise and `~/.local/bin` are simply not in it. yt-dlp goes missing, so a
/// YouTube save loses its transcript; every agent CLI goes missing with it, so
/// the agent stage never runs either. Nothing errors — the code takes its
/// fallback path and says nothing — and launching the same binary from a
/// terminal works perfectly, which makes it the worst kind of bug to be told
/// about.
///
/// So ask the login shell what PATH actually is, before anything looks for a
/// tool. One spawn, and only when the PATH we were handed is the stub launchd
/// gives out; it also picks up version managers that a hardcoded list of
/// directories would miss.
fn inherit_login_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    if current.split(':').filter(|s| !s.is_empty()).count() > 5 {
        return; // already a real shell PATH — started from a terminal
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Ok(out) = std::process::Command::new(shell).args(["-lc", "printf %s \"$PATH\""]).output() else { return };
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.split(':').filter(|s| !s.is_empty()).count() > current.split(':').count() {
        std::env::set_var("PATH", path);
    }
}

fn main() {
    inherit_login_path();

    tauri::Builder::default()
        // Size and position come back, like inbox2's. MAXIMIZED is dropped from
        // the flags: with a transparent window and an Overlay titlebar,
        // is_maximized() lies on macOS and the app relaunches maximized when the
        // last session wasn't. The hidden browser window is denied — it is
        // resized to the page height on every full-page shot.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::all() & !tauri_plugin_window_state::StateFlags::MAXIMIZED)
                .with_denylist(&[browser::WINDOW])
                .build(),
        )
        .setup(|app| {
            // The embedded browser (§4.1): its own window, a real WKWebView.
            // WebKit pauses requestAnimationFrame and WebGL in a window it
            // considers invisible, so canvas-drawn pages (3D heroes, editors,
            // galleries) capture as empty chrome. The window is therefore
            // ordered on screen — but at the bottom of the stack, transparent,
            // ignoring the mouse, off the Dock and Cmd-Tab — so it exists for
            // the compositor and never for the person.
            let bw = WebviewWindowBuilder::new(app, browser::WINDOW, WebviewUrl::External("about:blank".parse().unwrap()))
                .title("membox browser")
                // Cookie banners go before the picture — in every frame, since
                // the banner is often a cross-origin iframe (core browser.rs).
                .initialization_script_for_all_frames(membox_core::browser::consent_script())
                .inner_size(1280.0, 800.0)
                .position(0.0, 0.0)
                .visible(false)
                .decorations(false)
                .skip_taskbar(true)
                .always_on_bottom(true)
                .build()?;
            let _ = bw.set_ignore_cursor_events(true);
            browser::make_invisible(&bw);
            let _ = bw.show();

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // Before anything else: stderr is nowhere for a bundled app.
            let log_path = membox_core::applog::init(&data_dir, log::LevelFilter::Info);
            let browser = Arc::new(browser::TauriBrowser::new(app.handle().clone()));
            let lib = Library::open(&data_dir, browser).map_err(std::io::Error::other)?;
            membox_core::seed::seed(&lib).map_err(std::io::Error::other)?;
            let h = app.handle().clone();
            lib.on_change(Box::new(move || {
                let _ = h.emit("library-changed", ());
            }));
            log::info!("membox data dir: {} · log: {}", data_dir.display(), log_path.display());
            app.manage(Core(lib));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dispatch, blobs_dir, show_browser, open_url])
        .run(tauri::generate_context!())
        .expect("error while running membox");
}
