//! `yt-dlp` sidecar — spec §3.2–3.4. One tool covers YouTube, YouTube Music and
//! Instagram: metadata as JSON, the poster, and the transcript as VTT.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[derive(Debug, Default)]
pub struct Media {
    pub title: Option<String>,
    pub channel: Option<String>,
    pub description: Option<String>,
    pub duration_s: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub transcript: Option<String>,
    pub meta: Value,
}

pub fn available() -> bool {
    which::which("yt-dlp").is_ok()
}

pub fn fetch(url: &str, scratch: &Path) -> Result<Media, String> {
    // A playlist is its entries' titles and ids, not 18 videos' metadata and
    // captions — flat, or the fetch takes minutes and gets rate-limited.
    let playlist = url.contains("/playlist?");
    let out = Command::new("yt-dlp")
        .args(if playlist { &["--flat-playlist"][..] } else { &[] })
        .args([
            "--dump-single-json",
            // --dump-json implies --simulate, which would skip writing the subs.
            "--no-simulate",
            "--skip-download",
            "--no-warnings",
            "--write-auto-subs",
            "--write-subs",
            // Original English only. `en.*` also matches every auto-translated
            // track (en-ar, en-de, …), which trips YouTube's 429 rate limit.
            "--sub-langs",
            "en-orig,en-en,en",
            "--sub-format",
            "vtt",
            "-o",
            "media.%(ext)s",
        ])
        .arg(url)
        .current_dir(scratch)
        .output()
        .map_err(|e| format!("yt-dlp: {e}"))?;
    // With --no-simulate, stdout also carries "[info] Writing subtitles…" lines;
    // the metadata is the (one, huge) line that is a JSON object. A subtitle
    // failure makes yt-dlp exit 1 after it has printed the JSON, so the exit
    // status only matters when there is no JSON to use.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json_line = stdout.lines().rev().find(|l| l.starts_with('{'));
    let Some(line) = json_line else {
        return Err(format!("yt-dlp exit {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    };
    let v: Value = serde_json::from_str(line).map_err(|e| format!("yt-dlp json: {e}"))?;

    // Manual captions first, then the original-language auto track, then the
    // English auto track. read_dir order is arbitrary, so choose explicitly.
    let transcript = ["media.en.vtt", "media.en-orig.vtt", "media.en-en.vtt"]
        .iter()
        .find_map(|f| std::fs::read_to_string(scratch.join(f)).ok())
        .map(|vtt| vtt_to_text(&vtt));

    // Entries as the tile and the inspector want them; `thumb` is the poster
    // url until enrich has fetched it into a blob. Thumbs are capped because a
    // tile shows four; the list itself is the whole playlist so search reads it.
    let entries: Vec<Value> = v["entries"]
        .as_array()
        .map(|es| {
            es.iter()
                .filter_map(|e| {
                    let id = e["id"].as_str()?;
                    Some(serde_json::json!({
                        "id": id, "title": e["title"], "duration": e["duration"].as_f64().map(fmt_duration),
                        "thumb": format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let total: f64 = v["entries"].as_array().into_iter().flatten().filter_map(|e| e["duration"].as_f64()).sum();
    let transcript = if playlist {
        Some(entries.iter().enumerate().map(|(i, e)| format!("{}. {}", i + 1, e["title"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"))
    } else {
        transcript
    };

    Ok(Media {
        title: v["title"].as_str().map(String::from),
        channel: v["channel"].as_str().or(v["uploader"].as_str()).map(String::from),
        description: v["description"].as_str().map(String::from),
        duration_s: v["duration"].as_f64().or(if total > 0.0 { Some(total) } else { None }),
        thumbnail_url: v["thumbnail"].as_str().map(String::from).or(entries.first().and_then(|e| e["thumb"].as_str().map(String::from))),
        transcript,
        meta: serde_json::json!({
            "playlist": if playlist { Value::Array(entries) } else { Value::Null },
            "count": v["playlist_count"],
            "channel": v["channel"], "uploader": v["uploader"], "uploadDate": v["upload_date"],
            "viewCount": v["view_count"], "categories": v["categories"], "tags": v["tags"], "artist": v["artist"],
            "album": v["album"], "track": v["track"], "chapters": v["chapters"], "extractor": v["extractor_key"],
        }),
    })
}

/// Download a poster with curl (present on every Mac; no HTTP client in core).
pub fn download(url: &str, to: &Path) -> Result<(), String> {
    let st = Command::new("curl")
        // Wikimedia (and others) throttle or refuse a bare `curl/8` user agent.
        .args(["-sL", "--max-time", "20", "-A", "membox/0.1 (local personal library)", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if st.success() && to.exists() {
        Ok(())
    } else {
        Err(format!("curl exit {st}"))
    }
}

pub fn fmt_duration(s: f64) -> String {
    let s = s.round() as i64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// VTT cues → "[m:ss] text" lines, deduplicating the rolling repeats auto-subs
/// produce. The timestamps are what let the inspector jump the video (§6.5).
pub fn vtt_to_text(vtt: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut last = String::new();
    let mut stamp = String::new();
    for line in vtt.lines() {
        let line = line.trim();
        if let Some((start, _)) = line.split_once(" --> ") {
            stamp = short_stamp(start);
            continue;
        }
        if line.is_empty() || line == "WEBVTT" || line.starts_with("Kind:") || line.starts_with("Language:") || line.starts_with("NOTE") {
            continue;
        }
        let text = strip_tags(line);
        if text.is_empty() || text == last || last.ends_with(&text) {
            continue;
        }
        out.push(format!("[{stamp}] {text}"));
        last = text;
    }
    out.join("\n")
}

fn short_stamp(t: &str) -> String {
    // 00:01:23.456 → 1:23
    let core = t.split('.').next().unwrap_or(t);
    let parts: Vec<&str> = core.split(':').collect();
    match parts.as_slice() {
        [h, m, s] if *h != "00" => format!("{}:{m}:{}", h.trim_start_matches('0'), &s[..2.min(s.len())]),
        [_, m, s] => format!("{}:{}", m.trim_start_matches('0').to_string().max("0".into()), &s[..2.min(s.len())]),
        [m, s] => format!("{m}:{s}"),
        _ => core.to_string(),
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_dedupes_rolling_auto_subs() {
        let vtt = "WEBVTT\nKind: captions\n\n00:00:01.000 --> 00:00:03.000\nhello<00:00:02.000><c> world</c>\n\n00:00:03.000 --> 00:00:05.000\nhello world\n\n00:00:05.000 --> 00:00:07.000\nnext line\n";
        assert_eq!(vtt_to_text(vtt), "[0:01] hello world\n[0:05] next line");
        assert_eq!(fmt_duration(3725.0), "1:02:05");
        assert_eq!(fmt_duration(65.0), "1:05");
    }
}
