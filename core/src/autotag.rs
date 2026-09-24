//! Tags and a folder without a model — what every paste gets, agent lane or
//! not. Precise, not clever: the site, the kind, the channel, the page's own
//! keywords. The agent (when on) adds the judgement calls on top.
//!
//! Everything here lands in `auto_tags` as well as `tags`, so the UI can show
//! which tags a person actually chose (§2.5).

use crate::model::{Folder, Item};

/// Tags derived from what the fetch stage already knows.
pub fn auto_tags(item: &Item) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: &str| {
        let t = slug(t);
        if t.len() >= 2 && !out.contains(&t) {
            out.push(t);
        }
    };

    match item.kind.as_str() {
        "youtube_video" | "vimeo" => push("video"),
        "youtube_music" | "music" => push("music"),
        "x_post" | "reddit" => push("post"),
        "youtube_playlist" => push("playlist"),
        "instagram" | "tiktok" => push("reel"),
        "github_repo" => push("repo"),
        "pdf" => push("pdf"),
        "webpage" => push("article"),
        "image" => push("image"),
        _ => {}
    }
    if let Some(d) = item.domain.as_deref() {
        push(site_name(d));
    }
    if let Some(s) = item.meta["siteName"].as_str() {
        push(s);
    }
    if let Some(c) = item.meta["channel"].as_str() {
        push(c);
    }
    for t in music_tags(item) {
        push(&t);
    }
    if let Some(k) = item.meta["keywords"].as_str() {
        for kw in k.split(',').map(str::trim).filter(|k| !k.is_empty()).take(4) {
            push(kw);
        }
    }
    out
}

/// Music says what it is in its own title far more often than in its metadata:
/// "Raga Bilawal", "Qawwali", "Carnatic vocal". Words, no model — the agent
/// adds the genres that aren't spelled out.
///
/// ponytail: a flat word list and the word after "raga". A compound name
/// ("Yaman Kalyan") tags as its first word, which is still the right shelf;
/// a real raga list is the upgrade if that ever matters.
fn music_tags(item: &Item) -> Vec<String> {
    let category_music = item.meta["categories"].as_array().is_some_and(|a| a.iter().any(|c| c.as_str() == Some("Music")));
    if !matches!(item.kind.as_str(), "music" | "youtube_music") && !category_music {
        return Vec::new();
    }
    let hay = format!(
        "{} {} {}",
        item.title,
        item.summary.as_deref().unwrap_or(""),
        item.meta["keywords"].as_str().unwrap_or("")
    )
    .to_lowercase();
    let mut out = Vec::new();
    // (needle, what it means). Order is the output order.
    const GENRES: &[(&str, &[&str])] = &[
        ("qawwali", &["qawwali", "sufi", "indian-classical"]),
        ("qawali", &["qawwali", "sufi", "indian-classical"]),
        ("hindustani", &["hindustani", "indian-classical"]),
        ("carnatic", &["carnatic", "indian-classical"]),
        ("dhrupad", &["dhrupad", "hindustani", "indian-classical"]),
        ("khayal", &["khayal", "hindustani", "indian-classical"]),
        ("khyal", &["khayal", "hindustani", "indian-classical"]),
        ("thumri", &["thumri", "hindustani", "indian-classical"]),
        ("tarana", &["tarana", "indian-classical"]),
        ("ghazal", &["ghazal"]),
        ("bhajan", &["bhajan"]),
        ("kirtan", &["kirtan"]),
    ];
    for (needle, tags) in GENRES {
        if hay.contains(needle) {
            out.extend(tags.iter().map(|t| t.to_string()));
        }
    }
    const TALAS: &[&str] = &["teental", "tintal", "jhaptal", "ektal", "rupak", "keherwa", "dadra", "chautal"];
    for t in TALAS {
        if hay.contains(t) {
            out.push(format!("tala-{t}"));
        }
    }
    if let Some(r) = raga_name(&hay) {
        out.push("indian-classical".into());
        out.push(format!("raga-{r}"));
    }
    out
}

/// The word after "raga" / "raag" / "rag", when there is one worth keeping.
fn raga_name(hay: &str) -> Option<String> {
    let words: Vec<&str> = hay.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let i = words.iter().position(|w| matches!(*w, "raga" | "raag" | "rag" | "ragam" | "raaga"))?;
    let name = words.get(i + 1)?;
    // "raga of the month", "raag is" — a stopword means the title was talking
    // about ragas, not naming one.
    const NOT_A_NAME: &[&str] = &["is", "in", "of", "the", "and", "on", "by", "a", "for", "with", "music", "based"];
    (name.len() >= 3 && !NOT_A_NAME.contains(name)).then(|| name.to_string())
}

/// Where a kind goes when nobody smarter is around. Uses the deterministic
/// seed ids, so it only fires while those folders exist.
pub fn rule_folder(item: &Item, folders: &[Folder]) -> Option<(String, String)> {
    let want = match item.kind.as_str() {
        "youtube_video" | "vimeo" | "instagram" | "tiktok" => ("f-seed-watching", "videos go to Watching"),
        "youtube_music" | "youtube_playlist" | "music" => ("f-seed-listening", "music goes to Listening"),
        "github_repo" => ("f-seed-build-code", "repositories go to Build › Code"),
        "note" => ("f-seed-notes", "stickies go to Notes"),
        "book" => ("f-seed-reading", "books go to Reading"),
        "movie" | "tv" => ("f-seed-watching", "films and series go to Watching"),
        "webpage" | "pdf" | "x_post" | "reddit" | "hn" => ("f-seed-reading", "pages go to Reading"),
        _ => return None,
    };
    folders.iter().find(|f| f.id == want.0 && !f.proposed).map(|f| (f.id.clone(), format!("Filed by rule: {}", want.1)))
}

/// `en.wikipedia.org` → `wikipedia`, `music.youtube.com` → `youtube`.
pub fn site_name(domain: &str) -> &str {
    let parts: Vec<&str> = domain.split('.').collect();
    match parts.as_slice() {
        [.., name, tld] if tld.len() <= 3 && !matches!(*name, "co" | "com" | "org" | "net" | "ac" | "gov") => name,
        // co.uk-style: take the part before the two-level suffix
        [.., name, _, _] => name,
        [name] => name,
        _ => domain,
    }
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tags_come_from_kind_site_channel_keywords() {
        let it = Item {
            kind: "youtube_video".into(),
            domain: Some("youtube.com".into()),
            meta: json!({ "channel": "TED", "keywords": "procrastination, Tim Urban, psychology" }),
            ..Default::default()
        };
        assert_eq!(auto_tags(&it), vec!["video", "youtube", "ted", "procrastination", "tim-urban", "psychology"]);
        let w = Item { kind: "webpage".into(), domain: Some("en.wikipedia.org".into()), meta: json!({ "siteName": "Wikipedia" }), ..Default::default() };
        assert_eq!(auto_tags(&w), vec!["article", "wikipedia"]);
        assert_eq!(site_name("bbc.co.uk"), "bbc");
        assert_eq!(site_name("github.com"), "github");
    }

    #[test]
    fn music_gets_its_genre_and_raga() {
        let it = Item {
            kind: "youtube_video".into(),
            title: "Pt Pushpraj Koshti Raga Bilawal (Alap & Compositions), Barsi of Ustad ZM Dagar".into(),
            summary: Some("Pandit Pushpraj Koshti plays an exquisite dhrupad in teental.".into()),
            domain: Some("youtube.com".into()),
            meta: json!({ "categories": ["Music"] }),
            ..Default::default()
        };
        let tags = auto_tags(&it);
        assert!(tags.contains(&"dhrupad".to_string()), "{tags:?}");
        assert!(tags.contains(&"indian-classical".to_string()), "{tags:?}");
        assert!(tags.contains(&"tala-teental".to_string()), "{tags:?}");
        assert!(tags.contains(&"raga-bilawal".to_string()), "{tags:?}");

        // Not music, and a raga only spoken about, stay out of it.
        let page = Item { kind: "webpage".into(), title: "What a raga is".into(), ..Default::default() };
        assert!(!auto_tags(&page).iter().any(|t| t.starts_with("raga-")));
        let q = Item { kind: "music".into(), title: "Nusrat Fateh Ali Khan — Allah Hoo (Qawwali)".into(), ..Default::default() };
        assert!(auto_tags(&q).contains(&"qawwali".to_string()));
    }

    #[test]
    fn rule_folder_uses_seed_ids() {
        let folders = vec![Folder { id: "f-seed-watching".into(), name: "Watching".into(), emoji: None, parent_id: None, proposed: false, position: 0, updated_at: String::new(), deleted_at: None }];
        let it = Item { kind: "youtube_video".into(), ..Default::default() };
        assert_eq!(rule_folder(&it, &folders).unwrap().0, "f-seed-watching");
        let page = Item { kind: "webpage".into(), ..Default::default() };
        assert!(rule_folder(&page, &folders).is_none(), "no Reading folder here");
        let film = Item { kind: "movie".into(), ..Default::default() };
        assert_eq!(rule_folder(&film, &folders).unwrap().0, "f-seed-watching");
    }
}
