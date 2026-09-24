// In-app updates, gliph's shape.
//
// latest.json sits on the newest GitHub release and is signed with a key whose
// public half is baked into the binary (tauri.release.conf.json). That
// signature is the security model: GitHub can serve any bytes it likes and the
// client refuses anything the private key did not sign.
//
// Only a CI release build carries `plugins.updater`. A `yarn install:app` or
// dev build has none, and the plugin refuses to initialise without it — so
// main.rs registers it on the same test as `supported()`.

use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

pub fn supported(app: &AppHandle) -> bool {
    app.config().plugins.0.contains_key("updater")
}

/// Look shortly after launch, then every half hour while the app is open. A hit
/// emits `update://available` with the version; the UI decides how loudly.
pub fn poll(app: &AppHandle) {
    if !supported(app) {
        return;
    }
    let app = app.clone();
    // A plain thread for the wait: sleeping on a runtime worker parks a thread
    // the plugins share.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        loop {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match check(&app).await {
                    Ok(Some(v)) => {
                        log::info!("update available: {v}");
                        let _ = app.emit("update://available", v);
                    }
                    Ok(None) => {}
                    // Offline, or no release yet. Not worth a word.
                    Err(e) => log::debug!("update check failed: {e}"),
                }
            });
            std::thread::sleep(std::time::Duration::from_secs(30 * 60));
        }
    });
}

/// The plugin's default client waits forever; a deadline turns a stalled
/// download into an error the UI can show and retry.
fn updater(app: &AppHandle, secs: u64) -> Result<tauri_plugin_updater::Updater, String> {
    app.updater_builder()
        .timeout(std::time::Duration::from_secs(secs))
        .build()
        .map_err(|e| e.to_string())
}

async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    // Without the plugin `updater_builder()` reaches for unmanaged state and
    // panics — a promise that never settles on the JS side.
    if !supported(app) {
        return Err("this build has no updater (a local install:app build)".into());
    }
    let found = updater(app, 30)?.check().await.map_err(|e| e.to_string())?;
    Ok(found.map(|u| u.version))
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<String>, String> {
    check(&app).await
}

/// Download, verify, replace the .app, relaunch.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    if !supported(&app) {
        return Err("this build has no updater".into());
    }
    let update = updater(&app, 10 * 60)?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
