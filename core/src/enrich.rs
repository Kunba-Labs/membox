//! The enrichment pipeline — spec §3. Fetch (browser / yt-dlp, no model) then
//! the agent stage. Runs on the library's single worker thread (§3.9:
//! concurrency 1, agent CLIs are chatty and rate-limited).

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::agent;
use crate::browser::EXTRACT_JS;
use crate::model::{now, Item};
use crate::ytdlp;
use crate::Library;

const AGENT_TIMEOUT: Duration = Duration::from_secs(240);

pub fn run(lib: &Arc<Library>, id: &str) {
    let Some(mut item) = lib.db.lock().get_item(id).ok().flatten() else { return };
    if item.trashed {
        return;
    }
    let started = std::time::Instant::now();
    log::info!("enrich {id} [{}] {}", item.kind, item.url.as_deref().unwrap_or(&item.title));
    let scratch = lib.data_dir.join("scratch").join(id);
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::create_dir_all(&scratch);

    lib.set_status(id, "fetching", None);
    let mut screenshot: Option<Vec<u8>> = None;
    let t_fetch = std::time::Instant::now();
    match fetch(lib, &mut item, &scratch, &mut screenshot) {
        Ok(()) => {}
        Err(FetchError::NoBrowser) => {
            // iOS, or a desktop without the hidden webview yet: leave it for a
            // host that has one (§7.3).
            item.status = "pending".into();
            item.error = Some("waiting for a browser".into());
            let _ = lib.db.lock().save_item(&item);
            lib.changed();
            return;
        }
        Err(FetchError::Other(e)) => {
            log::warn!("fetch {id}: {e}");
            match item.meta["guess"].as_str().map(String::from) {
                // `Cargo.site` didn't load — it was text after all (§1.5).
                // ponytail: the guess token, not the whole paste; prose around
                // it already lives in `notes`.
                Some(raw) => {
                    item.kind = "snippet".into();
                    item.url = None;
                    item.domain = None;
                    item.title = crate::capture::draft_title(None, &raw);
                    item.body_text = Some(raw.clone());
                    item.summary = Some(raw);
                    item.meta = Value::Null;
                }
                None => item.error = Some(e),
            }
        }
    }
    log::info!(
        "  fetched {id} in {}ms — {} · {} · {}",
        t_fetch.elapsed().as_millis(),
        item.url.as_deref().unwrap_or("no url"),
        if item.thumb.is_some() { "thumb" } else { "no thumb" },
        item.error.as_deref().unwrap_or("ok")
    );
    // A redirect can land on a page that is already saved: the older item
    // keeps its id, and takes this paste's notes and folders.
    let dup = lib.db.lock().find_by_dedupe(&item.dedupe_key).ok().flatten().filter(|d| d.id != item.id);
    if let Some(mut keep) = dup {
        log::info!("  {id} is {} — merged", keep.id);
        keep.last_seen_at = now();
        if let Some(n) = item.notes.filter(|n| !n.trim().is_empty()) {
            keep.notes = Some(match keep.notes.take().filter(|k| !k.trim().is_empty()) {
                Some(k) if k.contains(&n) => k,
                Some(k) => format!("{k}\n\n{n}"),
                None => n,
            });
        }
        for f in item.folder_ids {
            if !keep.folder_ids.contains(&f) {
                keep.folder_ids.push(f);
            }
        }
        let _ = lib.db.lock().save_item(&keep);
        let _ = lib.db.lock().delete_items(&[id.to_string()]);
        lib.prune_proposed();
        lib.changed();
        return;
    }
    // ---- no-model tagging + filing: every paste gets these (§2.5) ----
    lib.apply_auto_tags(&mut item);
    let _ = lib.db.lock().save_item(&item);
    lib.changed();

    // The agent stage is its own queue (§3.9): one CLI at a time, but *after*
    // everything pasted has a screenshot and a title. Twenty links stop looking
    // stuck within a minute instead of an hour.
    let settings = lib.db.lock().settings().unwrap_or_default();
    let wants_agent = !item.meta["skipAgent"].as_bool().unwrap_or(false)
        && agent::by_key(&settings.agent).map(|a| which::which(a.binary).is_ok()).unwrap_or(false);
    if wants_agent {
        item.status = "queued".into();
        let _ = lib.db.lock().save_item(&item);
        lib.changed();
        lib.queue_agent(id);
        log::info!("  queued {id} for the agent after {}ms", started.elapsed().as_millis());
        return;
    }
    finish(lib, item, started);
}

/// Did the second shot reach further down the page than the first? Unmeasurable
/// PNGs count as yes — the tile can still decide, an empty column cannot.
fn taller_than(full: &[u8], viewport: &[u8]) -> bool {
    match (image_dims(full), image_dims(viewport)) {
        (Some((_, f)), Some((_, v))) => f > v,
        _ => true,
    }
}

/// The page describes itself into `meta`, wiping whatever was there. These keys
/// are not the page's to overwrite: the thread beside it, and the "no agent"
/// flag a refetch set before the queue ever reached this item — losing that one
/// made "Retake screenshot" spend an agent run every time.
fn carried(meta: &Value) -> Vec<(String, Value)> {
    ["hn", "reddit", "skipAgent"]
        .iter()
        .filter(|k| !meta[**k].is_null())
        .map(|k| (k.to_string(), meta[*k].clone()))
        .collect()
}

/// The agent half — spec §3.5, one at a time, after every fetch is done.
pub fn run_agent(lib: &Arc<Library>, id: &str) {
    let Some(mut item) = lib.db.lock().get_item(id).ok().flatten() else { return };
    if item.trashed {
        return;
    }
    let started = std::time::Instant::now();
    let scratch = lib.data_dir.join("scratch").join(id);
    let screenshot = item.thumb.as_ref().and_then(|t| std::fs::read(lib.blob_path(t)).ok());
    let settings = lib.db.lock().settings().unwrap_or_default();
    let adapter = agent::by_key(&settings.agent);
    let skip_agent = item.meta["skipAgent"].as_bool().unwrap_or(false);
    item.meta["skipAgent"] = Value::Null;
    if let Some(ad) = adapter.filter(|a| !skip_agent && which::which(a.binary).is_ok()) {
        lib.set_status(id, "enriching", None);
        let t_agent = std::time::Instant::now();
        match agent_stage(lib, &item, ad, &settings.local_model, &scratch, screenshot.as_deref()) {
            Ok(applied) => {
                item = lib.db.lock().get_item(id).ok().flatten().unwrap_or(item);
                if !applied {
                    item.error = Some("agent returned no result.json".into());
                }
            }
            Err(e) => {
                log::warn!("agent {id}: {e}");
                item = lib.db.lock().get_item(id).ok().flatten().unwrap_or(item);
                item.error = Some(e);
            }
        }
        log::info!("  {} agent {id} in {}ms", ad.key, t_agent.elapsed().as_millis());
    }
    finish(lib, item, started);
}

/// File it if nothing else did, settle on ready or failed, save, say so.
/// Keeps the scratch dir: it is the debuggable trail (§3.9).
fn finish(lib: &Arc<Library>, mut item: Item, started: std::time::Instant) {
    if item.folder_ids.is_empty() && item.meta["suggestedFolderId"].is_null() {
        let folders = lib.db.lock().all_folders().unwrap_or_default();
        if let Some((fid, why)) = crate::autotag::rule_folder(&item, &folders) {
            item.folder_ids.push(fid);
            if item.agent_reason.is_none() {
                item.agent_reason = Some(why);
            }
        }
    }
    item.status = if item.error.is_some() && item.thumb.is_none() && item.body_text.is_none() { "failed".into() } else { "ready".into() };
    let _ = lib.db.lock().save_item(&item);
    lib.changed();
    log::info!(
        "  done {} {} in {}ms{}",
        item.id,
        item.status,
        started.elapsed().as_millis(),
        item.error.as_deref().map(|e| format!(" — {e}")).unwrap_or_default()
    );
}

/// The fetch stage on its own, for the network-gated test in lib.rs.
#[cfg(test)]
pub fn fetch_for_test(lib: &Library, item: &mut Item, scratch: &std::path::Path, shot: &mut Option<Vec<u8>>) -> Result<(), String> {
    fetch(lib, item, scratch, shot).map_err(|e| match e {
        FetchError::NoBrowser => "no browser".to_string(),
        FetchError::Other(e) => e,
    })
}

enum FetchError {
    NoBrowser,
    Other(String),
}

fn fetch(lib: &Library, item: &mut Item, scratch: &std::path::Path, screenshot: &mut Option<Vec<u8>>) -> Result<(), FetchError> {
    // A pasted/dropped file: Quick Look renders the thumbnail for anything it
    // knows (PDF, Office, video, images…) — no per-format code.
    if let Some(rel) = item.meta["file"].as_str() {
        if item.thumb.is_none() {
            if let Some(png) = quicklook_thumb(&lib.blob_path(rel), scratch) {
                if let Some((w, h)) = image_dims(&png) {
                    item.aspect = w as f64 / h as f64;
                    item.dimensions = format!("{w}×{h}");
                }
                item.thumb = Some(lib.put_blob(&png, "png").map_err(FetchError::Other)?);
            }
        }
        if item.kind == "pdf" && item.body_text.is_none() {
            item.body_text = pdf_text(&lib.blob_path(rel));
            if item.summary.is_none() {
                item.summary = item.body_text.as_ref().map(|t| t.chars().take(600).collect());
            }
        }
        return Ok(());
    }
    // A book / film / series / game the person *named* rather than linked (§3.4).
    // A reference-site url doesn't disqualify it: the agent hands us a Wikipedia
    // article for a film all the time, and screenshotting that gives a tile of
    // an encyclopedia page where the poster should be.
    let reference = item
        .url
        .as_deref()
        .map(|u| u.contains("wikipedia.org") || u.contains("openlibrary.org") || u.contains("wikidata.org"))
        .unwrap_or(true);
    if matches!(item.kind.as_str(), "book" | "movie" | "tv" | "game") && item.thumb.is_none() && (reference || item.meta["query"].is_string()) {
        match shelf_lookup(lib, item, scratch) {
            Ok(()) if item.thumb.is_some() => return Ok(()),
            Ok(()) => {}
            // Open Library and Wikipedia only know what they know. The thing
            // still exists — it just isn't in a catalogue (§3.4).
            Err(FetchError::Other(e)) => log::info!("  {} not in a catalogue ({e}) — searching", item.id),
            Err(other) => return Err(other),
        }
    }

    // Nothing to open yet: a product nobody linked, or a title no catalogue
    // has. Search for it in our own browser and take the first real result —
    // the page exists, it just wasn't pasted.
    if item.url.is_none() && crate::SHELF.contains(&item.kind.as_str()) {
        let q = item.meta["query"].as_str().unwrap_or(&item.title).trim().to_string();
        let phrase = match item.kind.as_str() {
            "product" => format!("{q} buy"),
            "book" => format!("{q} book"),
            "movie" => format!("{q} film"),
            "tv" => format!("{q} tv series"),
            _ => q.clone(),
        };
        if let Some(found) = search_web(lib, &phrase) {
            log::info!("  found {} by search: {found}", item.id);
            item.domain = url::Url::parse(&found).ok().and_then(|u| u.host_str().map(String::from));
            item.url = Some(found);
            item.meta["foundBy"] = serde_json::json!("search");
            item.meta["query"] = serde_json::json!(q);
        }
    }

    let Some(url) = item.url.clone() else { return Ok(()) }; // snippets: nothing to fetch
    // An image URL: the bytes are the thumb; no page to screenshot.
    if item.kind == "image" {
        let tmp = scratch.join("image");
        if ytdlp::download(&url, &tmp).is_ok() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                let ext = image_ext(&bytes);
                if let Some((w, h)) = image_dims(&bytes) {
                    item.aspect = w as f64 / h as f64;
                    item.dimensions = format!("{w}×{h}");
                }
                item.size = human_size(bytes.len());
                item.thumb = Some(lib.put_blob(&bytes, ext).map_err(FetchError::Other)?);
                return Ok(());
            }
        }
    }
    // A Hacker News item is two things: the thing, and what people said about
    // it (§3.5). The thing is what gets screenshotted; the thread is kept
    // beside it, in `meta.hn` and in the transcript that search reads.
    let url = match item.kind.as_str() {
        "hn" => hn_lookup(item, scratch).unwrap_or(url),
        // Reddit's JSON is shut to anonymous curl but open to a real browser —
        // and we own one (§4.1). Same deal as HN: the link and the thread.
        "reddit" => reddit_lookup(lib, item, scratch).unwrap_or(url),
        _ => url,
    };

    let is_media = matches!(item.kind.as_str(), "youtube_video" | "youtube_music" | "youtube_playlist" | "instagram" | "tiktok");

    // Not finding the tool used to be indistinguishable from not needing it:
    // the condition below just went false and the page took the browser path,
    // no error, no warning, a YouTube save quietly missing its transcript.
    if is_media && !ytdlp::available() {
        log::warn!("yt-dlp is not on PATH — {} gets a screenshot and no transcript", item.id);
    }
    if is_media && ytdlp::available() {
        match ytdlp::fetch(&url, scratch) {
            Ok(m) => {
                if !item.user_edited {
                    if let Some(t) = m.title {
                        item.title = t;
                    }
                }
                item.duration = m.duration_s.map(ytdlp::fmt_duration);
                item.transcript = m.transcript;
                if item.summary.is_none() {
                    item.summary = m.description.map(|d| d.chars().take(600).collect());
                }
                item.body_text = item.summary.clone();
                item.meta = m.meta;
                // YouTube says which of its videos are music, so a music video
                // pasted from the main site files with the music, not the talks.
                if item.kind == "youtube_video" && item.meta["categories"].as_array().is_some_and(|a| a.iter().any(|c| c == "Music")) {
                    item.kind = "youtube_music".into();
                }
                if let Some(tags) = v_tags(&item.meta) {
                    item.meta["keywords"] = serde_json::json!(tags);
                }
                // A playlist tile is four of its videos; fetch those posters
                // into blobs and leave the rest as urls the inspector can lazy-load.
                if let Some(entries) = item.meta["playlist"].as_array_mut() {
                    for (i, e) in entries.iter_mut().take(4).enumerate() {
                        let Some(u) = e["thumb"].as_str().map(String::from) else { continue };
                        let tmp = scratch.join(format!("poster-{i}"));
                        if ytdlp::download(&u, &tmp).is_ok() {
                            if let Ok(bytes) = std::fs::read(&tmp) {
                                if let Ok(rel) = lib.put_blob(&bytes, image_ext(&bytes)) {
                                    e["thumb"] = serde_json::json!(rel);
                                }
                            }
                        }
                    }
                }
                if let Some(t) = m.thumbnail_url {
                    let tmp = scratch.join("poster");
                    if ytdlp::download(&t, &tmp).is_ok() {
                        if let Ok(bytes) = std::fs::read(&tmp) {
                            let ext = image_ext(&bytes);
                            if let Ok(rel) = lib.put_blob(&bytes, ext) {
                                item.thumb = Some(rel);
                                item.aspect = 16.0 / 9.0;
                                item.size = human_size(bytes.len());
                            }
                        }
                    }
                }
                return Ok(());
            }
            Err(e) => log::warn!("yt-dlp failed, falling back to browser: {e}"),
        }
    }

    let b = &lib.browser;
    if !b.available() {
        return Err(FetchError::NoBrowser);
    }
    // Held for the whole page — navigate, extract, both shots — so an agent's
    // MCP call can't move the page out from under the screenshot.
    let _drive = lib.browser_lock.lock();
    b.navigate(&url).map_err(|e| FetchError::Other(e.to_string()))?;
    // The cookie banner first: it would otherwise be the tile (§4.5).
    let consent = crate::browser::dismiss_consent(&**b);
    if consent != "nothingDetected" {
        log::info!("  consent on {url}: {consent}");
    }
    // Extraction is best-effort: a page that fights the script still gets
    // its screenshot; the error is kept on the item, not fatal.
    // Read after the banner: a consent gate can redirect back to the page.
    if item.url.as_deref() == Some(url.as_str()) {
        let landed = b.eval("location.href").ok().and_then(|j| serde_json::from_str::<String>(&j).ok());
        if let Some(to) = landed.and_then(|l| crate::capture::landed_url(&url, &l)) {
            log::info!("  {url} landed on {to}");
            item.domain = url::Url::parse(&to).ok().and_then(|u| u.host_str().map(String::from));
            item.dedupe_key = crate::capture::dedupe_key(Some(&to), "");
            item.url = Some(to);
        }
    }
    let v: Value = match b.eval(EXTRACT_JS) {
        Ok(meta) => serde_json::from_str(&meta).unwrap_or(Value::Null),
        Err(e) => {
            item.error = Some(format!("readable text: {e}"));
            Value::Null
        }
    };
    if !item.user_edited {
        if let Some(t) = v["title"].as_str().filter(|t| !t.trim().is_empty()) {
            item.title = t.trim().to_string();
        }
    }
    if let Some(d) = v["description"].as_str() {
        item.summary = Some(d.chars().take(600).collect());
    }
    item.body_text = v["text"].as_str().map(String::from);
    let carried = carried(&item.meta); // what has to survive the page's own metadata
    item.meta = serde_json::json!({ "siteName": v["siteName"], "keywords": v["keywords"], "ogImage": v["image"], "height": v["height"], "linkCount": v["links"].as_array().map(|a| a.len()) });
    for (k, val) in carried {
        item.meta[k] = val;
    }

    let png = b.snapshot_png(false).map_err(|e| FetchError::Other(e.to_string()))?;
    // A poster outranks a screenshot of the page that identified it: a film
    // tile should be its poster, not an encyclopedia article about the film.
    let keep_cover = item.thumb.is_some() && matches!(item.kind.as_str(), "book" | "movie" | "tv" | "game");
    if !keep_cover {
        if let Some((w, h)) = image_dims(&png) {
            item.aspect = w as f64 / h as f64;
            item.dimensions = format!("{w}×{h}");
        }
        item.size = human_size(png.len());
        item.thumb = Some(lib.put_blob(&png, "png").map_err(FetchError::Other)?);
    }
    *screenshot = Some(png);

    if matches!(item.kind.as_str(), "webpage" | "github_repo" | "x_post" | "hn" | "reddit") {
        match b.snapshot_png(true) {
            // A one-screen page photographs the same picture twice: the window
            // never grew, so the "full" shot is the viewport again. Keeping it
            // would crop the tile into the 4:5 pan frame for a pan with nowhere
            // to travel — a sideways tool page reads better whole.
            Ok(full) if taller_than(&full, screenshot.as_deref().unwrap_or_default()) => {
                item.page_shot = Some(lib.put_blob(&full, "png").map_err(FetchError::Other)?)
            }
            // Clear it, don't just skip: a page that used to be long and is now
            // one screen would otherwise keep pointing at the old tall shot.
            // Only a *measured* one-screen page clears it — a failed snapshot
            // below leaves whatever pan the item already had.
            Ok(_) => item.page_shot = None,
            // A tile with no pan looked like a design decision; it was a
            // swallowed error. Say which page, and what WebKit said.
            Err(e) => log::warn!("  no full-page shot for {}: {e}", item.url.as_deref().unwrap_or("?")),
        }
    }
    // The page said its piece; now the thread says its part, in the summary.
    let thread = if item.meta["hn"].is_object() { "hn" } else { "reddit" };
    if let Some(t) = item.meta[thread].as_object().cloned() {
        let where_ = match t.get("subreddit").and_then(Value::as_str) {
            Some(sub) => format!("r/{sub}"),
            None => "Hacker News".to_string(),
        };
        let line = format!(
            "On {where_}: {} points, {} comments.",
            t.get("points").and_then(Value::as_i64).unwrap_or(0),
            t.get("comments").and_then(Value::as_i64).unwrap_or(0)
        );
        item.summary = Some(match item.summary.take() {
            Some(s) if !s.trim().is_empty() => format!("{s}\n\n{line}"),
            _ => line,
        });
    }
    Ok(())
}

/// Algolia's HN API, keyless: the story, its link, and the thread in one call.
/// The thread lands in `transcript` — search already reads that, and the agent
/// harness already hands it to the model.
fn hn_lookup(item: &mut Item, scratch: &std::path::Path) -> Option<String> {
    let discussion = item.url.clone()?;
    let id = url::Url::parse(&discussion).ok()?.query_pairs().find(|(k, _)| k == "id")?.1.into_owned();
    let v = get_json(&format!("https://hn.algolia.com/api/v1/items/{id}"), &scratch.join("hn.json"))?;
    if !item.user_edited {
        if let Some(t) = v["title"].as_str().filter(|t| !t.trim().is_empty()) {
            item.title = t.to_string();
        }
    }

    // Top-level comments, most-upvoted first as Algolia returns them.
    let mut thread = String::new();
    let mut count = 0usize;
    let mut stack: Vec<&Value> = v["children"].as_array().map(|a| a.iter().rev().collect()).unwrap_or_default();
    let top: Vec<&Value> = v["children"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    while let Some(n) = stack.pop() {
        count += 1;
        if let Some(kids) = n["children"].as_array() {
            stack.extend(kids.iter());
        }
    }
    for c in top.iter().take(8) {
        let text = crate::strip_html_or(&decode_entities(c["text"].as_str().unwrap_or("")), "");
        if text.is_empty() {
            continue;
        }
        let who = c["author"].as_str().unwrap_or("someone");
        thread.push_str(&format!("{who}: {}\n\n", text.chars().take(700).collect::<String>()));
    }
    // A Launch/Show HN is a text post: the pitch is the post, and the product's
    // link lives inside it. That link is what the person came for.
    let story_html = v["text"].as_str().unwrap_or("");
    let story = crate::strip_html_or(&decode_entities(story_html), "");
    if !story.is_empty() {
        thread = format!("{}: {story}\n\n{thread}", v["author"].as_str().unwrap_or("op"));
    }
    item.transcript = (!thread.is_empty()).then(|| thread.trim().to_string());
    item.meta = serde_json::json!({
        "hn": { "url": discussion, "id": id, "points": v["points"], "author": v["author"], "comments": count }
    });
    // The story's own link becomes the item's; a text post lends its first
    // outward link instead; an Ask HN with neither keeps the thread as its page.
    let link = v["url"].as_str().map(String::from).filter(|u| u.starts_with("http")).or_else(|| first_link(story_html));
    link.map(|u| {
        item.domain = url::Url::parse(&u).ok().and_then(|p| p.host_str().map(String::from));
        item.url = Some(u.clone());
        u
    })
}

/// The post id in any Reddit link: `/r/x/comments/<id>/slug`, `/comments/<id>`,
/// `redd.it/<id>`.
fn reddit_id(u: &str) -> Option<String> {
    let p = url::Url::parse(u).ok()?;
    let segs: Vec<String> = p.path_segments()?.filter(|s| !s.is_empty()).map(str::to_string).collect();
    if p.host_str() == Some("redd.it") {
        return segs.first().cloned();
    }
    let i = segs.iter().position(|s| s == "comments")?;
    segs.get(i + 1).cloned()
}

/// Reddit through our own browser: the post, its outward link, and the thread.
/// Media posts (i.redd.it, galleries) keep the thread as their page.
fn reddit_lookup(lib: &Library, item: &mut Item, scratch: &std::path::Path) -> Option<String> {
    let permalink = item.url.clone()?;
    let id = reddit_id(&permalink)?;
    let b = &lib.browser;
    let _drive = lib.browser_lock.lock();
    b.navigate(&format!("https://www.reddit.com/comments/{id}.json?limit=30&raw_json=1")).ok()?;
    let raw = b.eval("document.body.innerText").ok()?;
    let _ = std::fs::write(scratch.join("reddit.json"), &raw);
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let post = &v[0]["data"]["children"][0]["data"];
    if post.is_null() {
        return None;
    }

    if !item.user_edited {
        if let Some(t) = post["title"].as_str().filter(|t| !t.trim().is_empty()) {
            item.title = t.to_string();
        }
    }
    let sub = post["subreddit"].as_str().unwrap_or("").to_string();
    item.meta = serde_json::json!({ "reddit": {
        "url": format!("https://www.reddit.com{}", post["permalink"].as_str().unwrap_or("")),
        "id": id, "subreddit": sub, "author": post["author"],
        "points": post["score"], "comments": post["num_comments"],
    }});

    // The post itself, then the top replies, in the field search reads.
    let mut thread = String::new();
    if let Some(t) = post["selftext"].as_str().filter(|t| !t.trim().is_empty()) {
        thread.push_str(&format!("{}: {}\n\n", post["author"].as_str().unwrap_or("op"), t.chars().take(2500).collect::<String>()));
    }
    if let Some(kids) = v[1]["data"]["children"].as_array() {
        for c in kids.iter().filter(|c| c["kind"] == "t1").take(8) {
            let d = &c["data"];
            let body = d["body"].as_str().unwrap_or("").trim();
            if body.is_empty() || d["author"].as_str() == Some("AutoModerator") {
                continue;
            }
            thread.push_str(&format!("{}: {}\n\n", d["author"].as_str().unwrap_or("someone"), body.chars().take(700).collect::<String>()));
        }
    }
    item.transcript = (!thread.is_empty()).then(|| thread.trim().to_string());

    // A link post points outward; a self post may still name a link in its text.
    let outward = post["url"].as_str().filter(|u| u.starts_with("http")).map(String::from)
        .filter(|u| !u.contains("reddit.com") && !u.contains("redd.it"))
        .or_else(|| post["selftext_html"].as_str().and_then(|h| first_link(&decode_entities(h))))
        .filter(|u| !u.contains("reddit.com"));
    outward.map(|u| {
        item.domain = url::Url::parse(&u).ok().and_then(|p| p.host_str().map(String::from));
        item.url = Some(u.clone());
        u
    })
}

/// The first link out of a post that isn't back into Hacker News.
fn first_link(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let end = rest.find('"').unwrap_or(rest.len());
        let href = decode_entities(&rest[..end]);
        rest = &rest[end..];
        if href.starts_with("http") && !href.contains("ycombinator.com") {
            return Some(href);
        }
    }
    None
}

/// HN's JSON is HTML-escaped twice over: `&#x2F;` for every slash.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 8) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let body = &rest[1..end];
        let ch = match body {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => body
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Open Library for books, Wikipedia for films and series — both keyless, both
/// fine with the app being offline (the item just stays a name).
///
/// ponytail: no API keys, no crate; `curl` fetches JSON and the cover. Covers
/// are 2:3 often enough that measuring them isn't worth a JPEG decoder.
fn shelf_lookup(lib: &Library, item: &mut Item, scratch: &std::path::Path) -> Result<(), FetchError> {
    let q = item.meta["query"].as_str().unwrap_or(&item.title).trim().to_string();
    if q.is_empty() {
        return Ok(());
    }
    // "The Last Question - Isaac Asimov" found nothing at all: a leading `-` is
    // Solr's NOT operator, so it searched for the book *without* Asimov in it.
    // Words only, then — every search here is a name, not a query.
    let words = q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let esc = urlencode(&words);
    let mut cover = None;
    let mut page = None;

    // Steam's store API is keyless and has the box art. A console-only game
    // isn't on it, so Wikipedia catches that one like any other title.
    if item.kind == "game" {
        if let Some(c) = steam_lookup(item, &q, scratch) {
            cover = Some(c);
            page = item.url.clone();
        }
    }
    if cover.is_some() {
        // Steam answered.
    } else if item.kind == "book" {
        let url = format!("https://openlibrary.org/search.json?q={esc}&limit=5&fields=key,title,author_name,first_publish_year,cover_i,first_sentence");
        let v = get_json(&url, &scratch.join("book.json")).ok_or_else(|| FetchError::Other("open library unreachable".into()))?;
        let docs = v["docs"].as_array().cloned().unwrap_or_default();
        // The top hit is the right work; the edition with a scan is often the
        // second one under the same title. Take the best hit that has a cover.
        let first = docs.first().cloned().unwrap_or(Value::Null);
        let title_of = |d: &Value| d["title"].as_str().unwrap_or("").to_lowercase();
        let d = docs
            .iter()
            .find(|d| d["cover_i"].is_i64() && title_of(d) == title_of(&first))
            .cloned()
            .unwrap_or(first);
        if d.is_null() {
            return Err(FetchError::Other(format!("no book found for \"{q}\"")));
        }
        // Open Library answers *something* for anything. "The Formation of the
        // Secular Mind" came back as "The light of the Cross in the twentieth
        // century" — a stranger's cover is worse than none.
        let with_author = format!("{} {}", title_of(&d), d["author_name"][0].as_str().unwrap_or(""));
        if !same_thing(&with_author, &q.to_lowercase()) {
            log::warn!("  open library gave {:?} for {q:?} — not using it", d["title"]);
            return Err(FetchError::Other(format!("no confident match for \"{q}\"")));
        }
        let d = &d;
        if !item.user_edited {
            if let Some(t) = d["title"].as_str() {
                item.title = t.to_string();
            }
        }
        let author = d["author_name"][0].as_str().unwrap_or("").to_string();
        let year = d["first_publish_year"].as_i64();
        item.meta = serde_json::json!({ "author": author, "year": year, "query": q });
        item.summary = d["first_sentence"][0].as_str().or_else(|| d["first_sentence"].as_str()).map(String::from)
            .or_else(|| (!author.is_empty()).then(|| match year { Some(y) => format!("{author}, {y}"), None => author.clone() }));
        item.body_text = item.summary.clone();
        item.duration = year.map(|y| y.to_string());
        cover = d["cover_i"].as_i64().map(|c| format!("https://covers.openlibrary.org/b/id/{c}-L.jpg"));
        page = d["key"].as_str().map(|k| format!("https://openlibrary.org{k}"));
    } else {
        // "Severance" is a word; "Severance TV series" is the show. The hint
        // costs nothing and picks the right article far more often.
        // If something already found the article — the agent usually has —
        // use it. Searching again with a title that was only ever a nickname
        // ("Zaina wa Nahoul" for Maya the Honey Bee) throws away the answer.
        let known = item
            .url
            .as_deref()
            .and_then(|u| u.split("/wiki/").nth(1))
            .map(|k| k.split(['#', '?']).next().unwrap_or(k).to_string())
            .filter(|k| !k.is_empty());

        let hint = match item.kind.as_str() {
            "tv" => "TV series",
            "game" => "video game",
            _ => "film",
        };
        let mut hit = Value::Null;
        let key = match known {
            Some(k) => k,
            None => {
                let url = format!("https://en.wikipedia.org/w/rest.php/v1/search/page?q={esc}+{}&limit=1", urlencode(hint));
                let v = get_json(&url, &scratch.join("search.json")).ok_or_else(|| FetchError::Other("wikipedia unreachable".into()))?;
                hit = v["pages"][0].clone();
                match hit["key"].as_str() {
                    Some(k) => k.to_string(),
                    None => return Err(FetchError::Other(format!("nothing found for \"{q}\""))),
                }
            }
        };
        if !item.user_edited {
            if let Some(t) = hit["title"].as_str() {
                item.title = t.to_string();
            }
        }
        let wiki = format!("https://en.wikipedia.org/wiki/{key}");
        // Wikipedia identifies it; IMDb is where you actually want to land.
        // Wikidata carries the link between them (P345), keylessly.
        page = Some(wiki.clone());
        if matches!(item.kind.as_str(), "movie" | "tv") {
            if let Some(w) = get_json(
                &format!("https://www.wikidata.org/w/api.php?action=wbgetentities&sites=enwiki&titles={}&props=claims&format=json", urlencode(&key)),
                &scratch.join("wikidata.json"),
            ) {
                let claim = |p: &str| {
                    w["entities"].as_object().and_then(|e| e.values().next()).map(|e| e["claims"][p][0]["mainsnak"]["datavalue"]["value"].clone())
                    .and_then(|v| v.as_str().map(String::from))
                };
                if let Some(imdb) = claim("P345") {
                    page = Some(format!("https://www.imdb.com/title/{imdb}/"));
                }
            }
        }
        if let Some(sum) = get_json(&format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", urlencode(&key)), &scratch.join("summary.json")) {
            item.summary = sum["extract"].as_str().map(|e| e.chars().take(600).collect());
            item.body_text = item.summary.clone();
            cover = sum["originalimage"]["source"].as_str().or_else(|| sum["thumbnail"]["source"].as_str()).map(String::from);
            item.meta = serde_json::json!({ "description": sum["description"], "query": q, "wikipedia": wiki });
        }
    }

    item.url = page;
    item.domain = item.url.as_deref().and_then(|u| url::Url::parse(u).ok()).and_then(|u| u.host_str().map(String::from));
    if let Some(c) = cover {
        let tmp = scratch.join("cover");
        log::info!("  cover for {} ← {c}", item.id);
        if ytdlp::download(&c, &tmp).is_ok() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                let ext = image_ext(&bytes);
                log::info!("  cover bytes: {}", bytes.len());
                item.size = human_size(bytes.len());
                item.aspect = image_dims(&bytes).map(|(w, h)| w as f64 / h as f64).unwrap_or(2.0 / 3.0);
                item.thumb = Some(lib.put_blob(&bytes, ext).map_err(FetchError::Other)?);
            }
        }
    }
    Ok(())
}

/// Steam's storefront API, no key: the store page, the blurb, the studio, and
/// the 600×900 library capsule — real box art, in the shape a shelf wants.
fn steam_lookup(item: &mut Item, q: &str, scratch: &std::path::Path) -> Option<String> {
    let hit = get_json(
        &format!("https://store.steampowered.com/api/storesearch/?term={}&cc=us&l=en", urlencode(q)),
        &scratch.join("steam-search.json"),
    )?;
    let id = hit["items"][0]["id"].as_i64()?;
    let d = get_json(
        &format!("https://store.steampowered.com/api/appdetails?appids={id}&l=en"),
        &scratch.join("steam.json"),
    )?;
    let app = &d[id.to_string()]["data"];
    if app.is_null() {
        return None;
    }
    if !item.user_edited {
        if let Some(t) = app["name"].as_str() {
            item.title = t.to_string();
        }
    }
    item.summary = app["short_description"].as_str().map(|s| crate::strip_html_or(s, "").chars().take(600).collect());
    item.body_text = item.summary.clone();
    let genres: Vec<&str> = app["genres"].as_array().map(|a| a.iter().filter_map(|g| g["description"].as_str()).collect()).unwrap_or_default();
    item.duration = app["release_date"]["date"].as_str().map(String::from);
    item.meta = serde_json::json!({
        "steamAppId": id,
        "developer": app["developers"][0],
        "genres": genres,
        "released": app["release_date"]["date"],
        "query": q,
    });
    item.url = Some(format!("https://store.steampowered.com/app/{id}/"));
    Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_600x900.jpg"))
}

/// Is this answer about the thing that was asked for? Most of the query's real
/// words have to show up in the title. Cheap, and it only has to catch the
/// answers that are about something else entirely.
fn same_thing(title: &str, query: &str) -> bool {
    const NOISE: &[&str] = &["the", "a", "an", "of", "and", "in", "on", "to", "for", "de", "la", "le"];
    let words = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 1 && !NOISE.contains(w))
            .map(str::to_string)
            .collect()
    };
    let (q, t) = (words(query), words(title));
    if q.is_empty() {
        return true;
    }
    let hit = q.iter().filter(|w| t.contains(w)).count();
    hit * 10 >= q.len() * 6
}

/// The web, through our own browser (§4.1): DuckDuckGo's plain-HTML endpoint,
/// the first result that isn't the engine itself.
///
/// ponytail: one search engine, first hit, no ranking of our own. If a page
/// turns out to be the wrong one, the person can paste the right link over it —
/// which is cheaper than us pretending to judge relevance.
fn search_web(lib: &Library, query: &str) -> Option<String> {
    let b = &lib.browser;
    if !b.available() || query.trim().is_empty() {
        return None;
    }
    let _drive = lib.browser_lock.lock();
    b.navigate(&format!("https://duckduckgo.com/html/?q={}", urlencode(query))).ok()?;
    let raw = b.eval(crate::browser::EXTRACT_JS).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let skip = ["duckduckgo.com", "google.", "bing.com", "yandex.", "/y.js", "ad_provider"];
    v["links"]
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str())
        .find(|u| u.starts_with("http") && !skip.iter().any(|s| u.contains(s)))
        .map(str::to_string)
}

/// A gap between metadata lookups. Twenty in a row is exactly what trips
/// Wikimedia's anonymous limit — `429, retry-after: 6` — and then every item in
/// the batch comes back empty, which is what "most of our movies went to
/// Wikipedia" looked like from the outside.
///
/// ponytail: one global gap for every host, not a per-host token bucket. These
/// are cover lookups on a personal library; a second and a bit between them
/// costs nothing and keeps us welcome.
fn polite() {
    const GAP: std::time::Duration = std::time::Duration::from_millis(1300);
    static LAST: parking_lot::Mutex<Option<std::time::Instant>> = parking_lot::Mutex::new(None);
    let mut last = LAST.lock();
    if let Some(t) = *last {
        let since = t.elapsed();
        if since < GAP {
            std::thread::sleep(GAP - since);
        }
    }
    *last = Some(std::time::Instant::now());
}

/// Three tries, backing off, because a public API's answer to "too fast" is a
/// page of prose where the JSON should be.
fn get_json(url: &str, to: &std::path::Path) -> Option<Value> {
    for attempt in 1..=3 {
        polite();
        if ytdlp::download(url, to).is_ok() {
            if let Ok(v) = serde_json::from_str(&std::fs::read_to_string(to).ok()?) {
                return Some(v);
            }
        }
        let body = std::fs::read_to_string(to).unwrap_or_default();
        log::warn!("  no json from {url} (attempt {attempt}): {}", body.lines().next().unwrap_or("no body"));
        std::thread::sleep(std::time::Duration::from_secs(4 * attempt));
    }
    None
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Returns whether the agent produced a usable result.
fn agent_stage(lib: &Arc<Library>, item: &Item, ad: &agent::Adapter, local_model: &str, scratch: &std::path::Path, screenshot: Option<&[u8]>) -> Result<bool, String> {
    let folders = lib.db.lock().all_folders().map_err(|e| e.to_string())?;
    let tags = lib.tag_counts();
    // This run's own MCP token: it may write this item and no other, however
    // many runs are in flight beside it.
    let run_token = lib.mcp.get().map(|m| m.open_run(&item.id));
    let mcp = lib.mcp.get().zip(run_token.as_ref()).map(|(m, t)| agent::McpEndpoint { url: m.url(), token: t.clone() });
    let h = agent::build_harness(scratch, item, &folders, &tags, screenshot, mcp.as_ref(), ad).map_err(|e| e.to_string())?;

    let run_id = lib.db.lock().start_run(&item.id, ad.key, &h.prompt).map_err(|e| e.to_string())?;
    let out = agent::run(ad, &h, local_model, AGENT_TIMEOUT);
    if let (Some(m), Some(t)) = (lib.mcp.get(), run_token.as_ref()) {
        m.close_run(t);
    }

    let out = match out {
        Ok(o) => o,
        Err(e) => {
            let _ = lib.db.lock().finish_run(run_id, "failed", None, Some(&e), 0);
            return Err(e);
        }
    };
    let result = agent::read_result(scratch, &out.stdout);
    let status = if out.timed_out { "timeout" } else if result.is_some() { "succeeded" } else { "failed" };
    let output = format!("{}\n--- stderr ---\n{}", out.stdout.chars().take(20000).collect::<String>(), out.stderr.chars().take(4000).collect::<String>());
    let _ = lib.db.lock().finish_run(run_id, status, Some(&output), if result.is_none() { Some("no result.json") } else { None }, out.ms);
    if out.timed_out {
        return Err(format!("agent timed out after {}s", AGENT_TIMEOUT.as_secs()));
    }
    let Some(r) = result else { return Ok(false) };
    lib.apply_agent_result(&item.id, &r).map_err(|e| e.to_string())?;
    Ok(true)
}

/// The extension for a blob from its own first bytes. Four call sites used to
/// each carry their own half of this list, so a HEIC from a phone was written
/// as `.jpg` and a WebP cover as `.jpg` too.
pub fn image_ext(bytes: &[u8]) -> &'static str {
    let ftyp = |brand: &[u8]| bytes.len() > 12 && &bytes[4..8] == b"ftyp" && bytes[8..12].starts_with(brand);
    if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.starts_with(b"RIFF") {
        "webp"
    } else if ftyp(b"avif") || ftyp(b"avis") {
        "avif"
    } else if ftyp(b"hei") || ftyp(b"mif1") || ftyp(b"msf1") {
        "heic"
    } else {
        "jpg"
    }
}

/// How big is this picture, from its header alone — PNG, JPEG, GIF, WebP and
/// the HEIC/AVIF a phone's camera actually produces.
///
/// This used to read PNG only, and every other format got the caller's
/// fallback: a photo pasted on the phone tiled at 4:3 and a JPEG cover at 2:3,
/// whatever their real shape. The grid lays tiles out by `aspect` (§6.3), so
/// the wrong number is visible on every screen.
///
/// ponytail: headers only, no decoding and no image crate. Enough for an
/// aspect ratio and a "1920×1080" line; reach for a real decoder the day
/// something needs the pixels. HEIC reports its *coded* size, which HEVC
/// rounds up to an even number of rows — a 640×371 photo reads 640×372, since
/// the one-pixel crop lives in a `clap` box we don't parse. Phone cameras
/// shoot even sizes, so this only shows on an odd-sized crop.
pub fn image_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as u32);
    let le16 = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32);
    let be32 = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let le24 = |i: usize| bytes.get(i..i + 3).map(|b| u32::from_le_bytes([b[0], b[1], b[2], 0]));

    if bytes.starts_with(b"\x89PNG") {
        return Some((be32(16)?, be32(20)?));
    }
    if bytes.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return match bytes.get(12..16)? {
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            // Lossy: the 3-byte start code, then two 14-bit sizes.
            b"VP8 " if bytes.get(23..26) == Some(&[0x9d, 0x01, 0x2a]) => {
                Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff))
            }
            // Lossless: 14 bits each, packed into one little-endian word.
            b"VP8L" if bytes.get(20) == Some(&0x2f) => {
                let v = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
                Some(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
            }
            _ => None,
        };
    }
    if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        // HEIC/AVIF: the size lives in an `ispe` box. A file carries several —
        // one per thumbnail — so the biggest one is the picture itself.
        return bytes
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"ispe")
            .filter_map(|(i, _)| Some((be32(i + 8)?, be32(i + 12)?)))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .max_by_key(|(w, h)| *w as u64 * *h as u64);
    }
    if bytes.starts_with(b"\xff\xd8") {
        // Walk the segments to the frame header; its length field is the only
        // way past a comment or an embedded thumbnail.
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xff {
                i += 1; // fill byte
                continue;
            }
            if marker == 0x01 || (0xd0..=0xd9).contains(&marker) {
                i += 2; // no payload
                continue;
            }
            // SOF0..SOF15 carry the size; C4/C8/CC sit in that range and don't.
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            let len = be16(i + 2)? as usize;
            if len < 2 {
                return None;
            }
            i += 2 + len;
        }
    }
    None
}

pub fn human_size(n: usize) -> String {
    if n > 1_000_000 {
        format!("{:.2} MB", n as f64 / 1_048_576.0)
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

#[allow(dead_code)]
fn _now() -> String {
    now()
}

/// yt-dlp's `tags` array → one comma list, like a page's meta keywords.
/// (Not `categories`: "People & Blogs" is YouTube's bucket, not a tag.)
fn v_tags(meta: &Value) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(a) = meta["tags"].as_array() {
        out.extend(a.iter().filter_map(Value::as_str).map(String::from));
    }
    (!out.is_empty()).then(|| out.into_iter().take(6).collect::<Vec<_>>().join(", "))
}

/// `qlmanage -t` — macOS Quick Look thumbnail for any file it understands.
#[cfg(target_os = "macos")]
fn quicklook_thumb(file: &std::path::Path, scratch: &std::path::Path) -> Option<Vec<u8>> {
    let out = scratch.join("ql");
    std::fs::create_dir_all(&out).ok()?;
    std::process::Command::new("qlmanage").args(["-t", "-s", "1024", "-o"]).arg(&out).arg(file).output().ok()?;
    let png = std::fs::read_dir(&out).ok()?.filter_map(|e| e.ok()).find(|e| e.path().extension().map(|x| x == "png").unwrap_or(false))?;
    std::fs::read(png.path()).ok()
}
#[cfg(not(target_os = "macos"))]
fn quicklook_thumb(_: &std::path::Path, _: &std::path::Path) -> Option<Vec<u8>> {
    None
}

/// Text of a PDF via the system `mdls`-free route: macOS ships no pdftotext,
/// but `textutil` can't read PDF either — so use Python's built-ins? No:
/// ponytail — `strings` is wrong and a PDF crate is heavy. Use Quick Look's
/// sibling: `mdimport` writes nothing we can read. Simplest honest option:
/// the system's `pdftotext` if installed (poppler), else none.
fn pdf_text(file: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("pdftotext").args(["-layout", "-l", "20"]).arg(file).arg("-").output().ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!t.is_empty()).then(|| t.chars().take(60000).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_screen_page_is_not_photographed_twice() {
        // Real PNG headers, one 1280×800 and one 1280×2400.
        let png = |w: u32, h: u32| {
            let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
            b.extend(w.to_be_bytes());
            b.extend(h.to_be_bytes());
            b.extend([8, 6, 0, 0, 0]);
            b
        };
        assert!(taller_than(&png(1280, 2400), &png(1280, 800)), "a long page has somewhere to pan");
        assert!(!taller_than(&png(1280, 800), &png(1280, 800)), "a one-screen page does not");
        assert!(taller_than(b"not a png", &png(1280, 800)), "unmeasurable is kept");
    }

    #[test]
    fn a_refetch_still_says_no_agent_after_the_page_has_spoken() {
        let meta = serde_json::json!({ "skipAgent": true, "hn": { "points": 4 }, "siteName": "old" });
        let kept = carried(&meta);
        let mut fresh = serde_json::json!({ "siteName": "new" });
        for (k, v) in kept {
            fresh[k] = v;
        }
        assert_eq!(fresh["skipAgent"], serde_json::json!(true), "the flag outlives the fetch");
        assert_eq!(fresh["hn"]["points"], 4);
        assert_eq!(fresh["siteName"], "new", "the page still gets to describe itself");
        // Nothing to carry from a plain page.
        assert!(carried(&serde_json::json!({ "siteName": "x" })).is_empty());
    }

    #[test]
    fn a_stranger_is_not_the_book_you_asked_for() {
        assert!(same_thing("Formations of the secular", "formations of the secular"));
        assert!(same_thing("Politics of Piety: The Islamic Revival", "politics of piety"));
        assert!(!same_thing("The light of the Cross in the twentieth century", "the formation of the secular mind"));
        // The query names the author as often as not.
        assert!(same_thing("The Last Question Isaac Asimov", "the last question — isaac asimov"));
    }

    #[test]
    fn every_reddit_link_shape_yields_its_id() {
        for u in ["https://www.reddit.com/r/rust/comments/abc123/some_title/", "https://reddit.com/comments/abc123", "https://redd.it/abc123"] {
            assert_eq!(reddit_id(u).as_deref(), Some("abc123"), "{u}");
        }
        assert_eq!(reddit_id("https://www.reddit.com/r/rust/"), None);
    }

    #[test]
    fn a_launch_hn_lends_its_first_link() {
        let html = "Hey HN — We&#x27;re Jacky and Mac from Trellis (<a href=\"https:&#x2F;&#x2F;runtrellis.com&#x2F;\">https:&#x2F;&#x2F;runtrellis.com&#x2F;</a>).";
        assert_eq!(first_link(html).as_deref(), Some("https://runtrellis.com/"));
        assert!(decode_entities(html).contains("We\'re"));
        // A thread link is not the product.
        assert_eq!(first_link("<a href=\"https:&#x2F;&#x2F;news.ycombinator.com&#x2F;item?id=1\">x</a>"), None);
    }

    /// Every format a phone or a cover download actually hands us. The offsets
    /// here were checked once against real `sips` output (png/jpeg/heic/gif and
    /// both webp flavours at a known size); these headers keep them honest.
    #[test]
    fn image_dims_reads_more_than_png() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&371u32.to_be_bytes());
        assert_eq!(image_dims(&png), Some((640, 371)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&640u16.to_le_bytes());
        gif.extend_from_slice(&371u16.to_le_bytes());
        assert_eq!(image_dims(&gif), Some((640, 371)));

        // A comment segment before the frame header: the walker has to step
        // over it by its length, not scan for the next 0xFFC0-looking byte.
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xfe, 0x00, 0x06, 0xc0, 0x01, 0x02, 0x03];
        jpg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpg.extend_from_slice(&371u16.to_be_bytes());
        jpg.extend_from_slice(&640u16.to_be_bytes());
        jpg.extend_from_slice(&[0; 8]);
        assert_eq!(image_dims(&jpg), Some((640, 371)));

        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\n\0\0\0\0".to_vec();
        vp8x.extend_from_slice(&639u32.to_le_bytes()[..3]);
        vp8x.extend_from_slice(&370u32.to_le_bytes()[..3]);
        assert_eq!(image_dims(&vp8x), Some((640, 371)));

        let mut vp8l = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        vp8l.extend_from_slice(&(639u32 | (370 << 14)).to_le_bytes());
        assert_eq!(image_dims(&vp8l), Some((640, 371)));

        // HEIC carries an `ispe` per thumbnail; the picture is the biggest.
        let mut heic = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic".to_vec();
        for (w, h) in [(160u32, 92u32), (640, 371)] {
            heic.extend_from_slice(b"ispe\0\0\0\0");
            heic.extend_from_slice(&w.to_be_bytes());
            heic.extend_from_slice(&h.to_be_bytes());
        }
        assert_eq!(image_ext(&heic), "heic");
        assert_eq!(image_dims(&heic), Some((640, 371)));

        assert_eq!(image_dims(b"not a picture at all"), None);
    }
}
