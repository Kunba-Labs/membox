//! Pure capture logic — spec §1.5 type detection and §1.7 canonicalisation.
//! Line-for-line port of desktop/src/capture.js; the two test suites agree.

use url::Url;

const TRACKING: &[&str] = &["fbclid", "gclid", "si", "igshid", "ref", "mc_cid", "mc_eid"];

pub fn canonical_url(raw: &str) -> Option<String> {
    let mut u = Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host).to_string();

    if host == "youtu.be" {
        return Some(format!("https://youtube.com/watch?v={}", u.path().trim_start_matches('/')));
    }
    if host == "youtube.com" || host == "music.youtube.com" {
        let q = |k: &str| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned());
        // A video opened from inside a playlist (`watch?v=…&list=PL…&index=7`)
        // is a save of the playlist. `RD…` is a radio mix — not a list, drop it.
        if let Some(list) = q("list").filter(|l| !l.starts_with("RD")) {
            return Some(format!("https://{host}/playlist?list={list}"));
        }
        let path_id = u
            .path_segments()
            .and_then(|mut s| match (s.next(), s.next()) {
                (Some("shorts" | "embed" | "live"), Some(id)) => Some(id.to_string()),
                _ => None,
            });
        if let Some(v) = path_id.or_else(|| q("v")) {
            return Some(format!("https://{host}/watch?v={v}"));
        }
    }

    let kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| !(k.starts_with("utm_") || TRACKING.contains(&k.as_ref())))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    u.set_fragment(None);
    let path = u.path().trim_end_matches('/').to_string();
    let query = if kept.is_empty() {
        String::new()
    } else {
        let mut s = Url::parse("https://x/").unwrap();
        s.query_pairs_mut().extend_pairs(kept);
        format!("?{}", s.query().unwrap_or(""))
    };
    Some(format!("https://{host}{path}{query}"))
}

/// Where the browser ended up, if it should replace what was pasted: a short
/// link or a moved page is filed under where it landed, so the same page pasted
/// two ways is one item. A landing that is a wall (sign-in, consent, captcha)
/// or a bounce to the site's front page says nothing about the page — keep the
/// paste, or every dead link on a site would merge into its homepage.
pub fn landed_url(pasted: &str, landed: &str) -> Option<String> {
    let c = canonical_url(landed).filter(|c| c != pasted)?;
    let u = Url::parse(&c).ok()?;
    let p = Url::parse(pasted).ok()?;
    if u.path().trim_matches('/').is_empty() && !p.path().trim_matches('/').is_empty() {
        return None;
    }
    let wall = ["login", "signin", "sign-in", "sign_in", "auth", "sso", "consent", "captcha", "challenge"];
    let host_wall = u.host_str().is_some_and(|h| h.split('.').any(|l| wall.contains(&l) || l == "accounts" || l == "myprivacy"));
    let path_wall = u.path_segments().is_some_and(|mut s| s.any(|seg| wall.iter().any(|w| seg.to_ascii_lowercase().starts_with(w))));
    (!host_wall && !path_wall).then_some(c)
}

pub fn detect_kind(canonical: Option<&str>, has_image: bool, has_file: bool) -> &'static str {
    if has_image {
        return "image";
    }
    if has_file {
        return "file";
    }
    let Some(c) = canonical else { return "snippet" };
    let Ok(u) = Url::parse(c) else { return "snippet" };
    let h = u.host_str().unwrap_or("");
    let p = u.path();
    let pl = p.to_ascii_lowercase();
    match h {
        "music.youtube.com" => "youtube_music",
        "youtube.com" if p == "/playlist" => "youtube_playlist",
        "youtube.com" => "youtube_video",
        "instagram.com" => "instagram",
        "tiktok.com" => "tiktok",
        "x.com" | "twitter.com" => "x_post",
        "vimeo.com" => "vimeo",
        "open.spotify.com" | "spotify.com" | "soundcloud.com" => "music",
        "reddit.com" | "old.reddit.com" | "np.reddit.com" | "redd.it" => "reddit",
        // The comments are half of why the link was worth keeping (§3.5).
        "news.ycombinator.com" => "hn",
        // Things with a cover and a synopsis, resolved in enrich (§3.4).
        "goodreads.com" | "openlibrary.org" | "books.google.com" => "book",
        "imdb.com" | "letterboxd.com" | "themoviedb.org" | "trakt.tv" | "rottentomatoes.com" => {
            if p.contains("/tv") || p.contains("/series") { "tv" } else { "movie" }
        }
        "store.steampowered.com" | "gog.com" | "epicgames.com" | "itch.io" | "nintendo.com" | "playstation.com" | "xbox.com" => "game",
        "amazon.com" | "amazon.co.uk" | "amazon.de" | "amazon.nl" | "ebay.com" | "etsy.com" | "aliexpress.com" => "product",
        "github.com" if p.matches('/').count() >= 2 => "github_repo",
        _ if pl.ends_with(".pdf") => "pdf",
        _ if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".svg"].iter().any(|e| pl.ends_with(e)) => "image",
        _ => "webpage",
    }
}

/// The first URL in a pasted blob.
pub fn first_url(text: &str) -> Option<String> {
    extract_urls(text, "").into_iter().next()
}

/// Every URL a paste carries, in order, deduped: `http(s)://…` in the text,
/// scheme-less `youtube.com/watch?v=…` / `www.bbc.co.uk`, and — when the
/// text has none — `href`/`src` attributes in the HTML flavour (a share
/// dialog's `<iframe src>`, a copied image's `<img src>`).
pub fn extract_urls(text: &str, html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn push(out: &mut Vec<String>, u: String) {
        let u = u.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '\'' | '"')).to_string();
        if !out.contains(&u) && out.len() < 20 {
            out.push(u);
        }
    }
    let is_stop = |c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '`');
    for tok in text.split(is_stop).filter(|t| !t.is_empty()) {
        // "www.bbc.co.uk," — the sentence's punctuation isn't the URL's.
        let tok = tok.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
        let lower = tok.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            push(&mut out, tok.to_string());
        } else if looks_like_bare_url(tok) {
            push(&mut out, format!("https://{tok}"));
        }
    }
    if out.is_empty() && !html.is_empty() {
        for attr in ["href=\"", "src=\""] {
            let mut rest = html;
            while let Some(i) = rest.find(attr) {
                rest = &rest[i + attr.len()..];
                let end = rest.find('"').unwrap_or(rest.len());
                let v = &rest[..end];
                if v.starts_with("http://") || v.starts_with("https://") {
                    push(&mut out, v.to_string());
                }
                rest = &rest[end..];
            }
        }
    }
    out
}

/// `youtube.com/watch?v=x`, `www.bbc.co.uk`, `github.com/a/b` — a domain
/// with a real-looking TLD, optionally a path. Not `e.g.` or `1.5`.
fn looks_like_bare_url(tok: &str) -> bool {
    let host = tok.split('/').next().unwrap_or("").split('?').next().unwrap_or("");
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || host.contains('@') {
        return false;
    }
    let tld = labels.last().unwrap();
    let ok_tld = tld.len() >= 2 && tld.len() <= 12 && tld.chars().all(|c| c.is_ascii_alphabetic());
    let ok_labels = labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let has_path_or_www = tok.contains('/') || labels[0] == "www" || labels.len() >= 3 || KNOWN_HOSTS.iter().any(|h| host.eq_ignore_ascii_case(h));
    // `Cargo.site`, `Intangible.ai` — a bare two-label token. A lowercase TLD
    // that isn't a file extension is enough to *try* it; [`is_guess`] marks it
    // so enrich can put it back to text when it doesn't load.
    let loose = labels.len() == 2 && tld.chars().all(|c| c.is_ascii_lowercase()) && !NOT_TLDS.contains(tld);
    ok_tld && ok_labels && (has_path_or_www || loose)
}

/// A URL from [`extract_urls`] that only the loose rule accepted: bare host,
/// two labels, no path. Worth opening; not worth believing until it loads.
pub fn is_guess(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    let h = u.host_str().unwrap_or("");
    u.path().trim_matches('/').is_empty()
        && h.matches('.').count() == 1
        && !h.starts_with("www.")
        && !KNOWN_HOSTS.iter().any(|k| h.eq_ignore_ascii_case(k))
}

/// Extensions a lone `name.ext` is far likelier to be than a host. (`.sh`,
/// `.app`, `.dev`, `.ai` stay out: real sites live there.)
const NOT_TLDS: &[&str] = &[
    "js", "ts", "jsx", "tsx", "py", "rs", "go", "rb", "php", "java", "swift", "cpp", "env", "md", "txt", "json",
    "yml", "yaml", "toml", "lock", "log", "css", "html", "xml", "sql", "csv", "png", "jpg", "jpeg", "gif", "svg",
    "webp", "pdf", "zip", "mp3", "mp4", "mov", "exe",
];

const KNOWN_HOSTS: &[&str] = &["youtube.com", "youtu.be", "github.com", "instagram.com", "tiktok.com", "x.com", "twitter.com", "vimeo.com", "reddit.com", "spotify.com", "soundcloud.com", "wikipedia.org", "medium.com", "substack.com"];

/// Pasted text that is source code rather than prose — worth a `code` tag
/// and a monospace tile.
pub fn looks_like_code(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 {
        return false;
    }
    let codey = lines.iter().filter(|l| {
        let t = l.trim_end();
        t.ends_with(';') || t.ends_with('{') || t.ends_with('}') || t.starts_with("    ") || t.starts_with('\t')
            || t.contains("=>") || t.contains("fn ") || t.contains("def ") || t.contains("import ") || t.contains("const ") || t.contains("let ")
            || t.starts_with("#include") || t.starts_with("SELECT ") || t.starts_with("$ ")
    }).count();
    codey * 2 >= lines.len()
}

pub fn draft_title(canonical: Option<&str>, text: &str) -> String {
    if let Some(c) = canonical {
        if let Ok(u) = Url::parse(c) {
            if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "v") {
                return format!("{} · {}", u.host_str().unwrap_or(""), v);
            }
            if let Some(last) = u.path_segments().and_then(|s| s.filter(|x| !x.is_empty()).last()) {
                let decoded = percent_decode(last);
                return decoded.replace(['-', '_'], " ");
            }
            return u.host_str().unwrap_or("").to_string();
        }
    }
    let line = text.trim().lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return "Untitled".into();
    }
    let chars: Vec<char> = line.chars().collect();
    if chars.len() > 72 {
        format!("{}…", chars[..69].iter().collect::<String>())
    } else {
        line.to_string()
    }
}

pub fn dedupe_key(canonical: Option<&str>, text: &str) -> String {
    let s = canonical.map(str::to_string).unwrap_or_else(|| text.trim().to_string());
    blake3::hash(s.as_bytes()).to_hex()[..16].to_string()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landing_replaces_the_paste_unless_it_is_a_wall() {
        let short = "https://bit.ly/abc";
        assert_eq!(landed_url(short, "https://www.example.com/post/1?utm_source=x").as_deref(), Some("https://example.com/post/1"));
        assert_eq!(landed_url("https://example.com/post/1", "https://example.com/post/1/"), None);
        assert_eq!(landed_url("https://example.com/gone", "https://example.com/"), None);
        assert_eq!(landed_url("https://medium.com/p/x", "https://medium.com/m/signin?redirect=x"), None);
        assert_eq!(landed_url("https://nu.nl/a/b", "https://myprivacy.dpgmedia.nl/consent?x=1"), None);
        assert_eq!(landed_url("https://x.com/a", "https://accounts.google.com/v3/x"), None);
    }

    #[test]
    fn youtube_variants_collapse() {
        let want = "https://youtube.com/watch?v=abc123";
        for u in [
            "https://www.youtube.com/watch?v=abc123&t=42s&si=xyz",
            "https://youtu.be/abc123?si=xyz",
            "https://m.youtube.com/watch?v=abc123",
            "https://www.youtube.com/shorts/abc123",
        ] {
            assert_eq!(canonical_url(u).as_deref(), Some(want), "{u}");
        }
        assert_eq!(detect_kind(Some(want), false, false), "youtube_video");
    }

    #[test]
    fn music_and_playlists() {
        let m = canonical_url("https://music.youtube.com/watch?v=q1&list=RD").unwrap();
        assert_eq!(detect_kind(Some(&m), false, false), "youtube_music");
        let p = canonical_url("https://www.youtube.com/playlist?list=PL9").unwrap();
        assert_eq!(detect_kind(Some(&p), false, false), "youtube_playlist");
        let inside = canonical_url("https://www.youtube.com/watch?v=JuoVZkPBiKk&list=PLoRO&index=7").unwrap();
        assert_eq!(inside, "https://youtube.com/playlist?list=PLoRO");
        assert_eq!(canonical_url("https://www.youtube.com/watch?v=abc&list=RDabc").unwrap(), "https://youtube.com/watch?v=abc");
    }

    #[test]
    fn tracking_and_hash_drop() {
        assert_eq!(
            canonical_url("https://WWW.Example.com/a/b/?utm_source=x&fbclid=y&page=2#top").unwrap(),
            "https://example.com/a/b?page=2"
        );
    }

    #[test]
    fn dedupe_stable() {
        let a = dedupe_key(canonical_url("https://youtu.be/abc123").as_deref(), "");
        let b = dedupe_key(canonical_url("https://www.youtube.com/watch?v=abc123&utm_medium=x").as_deref(), "");
        assert_eq!(a, b);
        assert_ne!(a, dedupe_key(canonical_url("https://youtu.be/abc124").as_deref(), ""));
    }

    #[test]
    fn shelf_kinds() {
        let k = |u: &str| detect_kind(canonical_url(u).as_deref(), false, false);
        assert_eq!(k("https://www.goodreads.com/book/show/1.The_Hobbit"), "book");
        assert_eq!(k("https://www.imdb.com/title/tt0111161/"), "movie");
        assert_eq!(k("https://www.themoviedb.org/tv/1396-breaking-bad"), "tv");
        assert_eq!(k("https://www.amazon.nl/dp/B0C1234"), "product");
        assert_eq!(k("https://news.ycombinator.com/item?id=44123456"), "hn");
        assert_eq!(k("https://old.reddit.com/r/rust/comments/abc123/title/"), "reddit");
        assert_eq!(k("https://redd.it/abc123"), "reddit");
        assert_eq!(k("https://store.steampowered.com/app/1145350/Hades_II/"), "game");
    }

    #[test]
    fn kinds() {
        let k = |u: &str| detect_kind(canonical_url(u).as_deref(), false, false);
        assert_eq!(k("https://www.instagram.com/reel/XyZ/"), "instagram");
        assert_eq!(k("https://github.com/asg017/sqlite-vec"), "github_repo");
        assert_eq!(k("https://x.com/u/status/1"), "x_post");
        assert_eq!(k("https://a.org/paper.PDF"), "pdf");
        assert_eq!(k("https://nautil.us/light"), "webpage");
        assert_eq!(detect_kind(None, false, false), "snippet");
        assert_eq!(detect_kind(None, true, false), "image");
        assert_eq!(canonical_url("ftp://x.org/f"), None);
        assert_eq!(canonical_url("not a url"), None);
    }

    #[test]
    fn anything_pasted_finds_its_urls() {
        assert_eq!(extract_urls("look youtube.com/watch?v=abc and www.bbc.co.uk, plus https://x.com/a/status/1.", ""),
            vec!["https://youtube.com/watch?v=abc", "https://www.bbc.co.uk", "https://x.com/a/status/1"]);
        assert_eq!(extract_urls("e.g. version 1.5 costs 3.50", ""), Vec::<String>::new());
        // Bare two-label hosts: worth a try, and flagged as a guess.
        assert_eq!(extract_urls("Intangible.ai\nCargo.site\nDeath to stock\nShadergradient.co", ""),
            vec!["https://Intangible.ai", "https://Cargo.site", "https://Shadergradient.co"]);
        assert_eq!(extract_urls("see capture.rs and notes.md, README.txt", ""), Vec::<String>::new());
        assert_eq!(extract_urls("went to Paris.Then home", ""), Vec::<String>::new());
        assert!(is_guess("https://cargo.site"));
        assert!(!is_guess("https://github.com"));
        assert!(!is_guess("https://a.b.co"));
        assert!(!is_guess("https://cargo.site/work"));
        assert_eq!(extract_urls("", r#"<iframe src="https://www.youtube.com/embed/abc123" allow=""></iframe>"#), vec!["https://www.youtube.com/embed/abc123"]);
        assert_eq!(extract_urls("", r#"<a href="https://a.org/p"><img src="https://a.org/i.png"></a>"#), vec!["https://a.org/p", "https://a.org/i.png"]);
        let k = |u: &str| detect_kind(canonical_url(u).as_deref(), false, false);
        assert_eq!(k("https://vimeo.com/12345"), "vimeo");
        assert_eq!(k("https://open.spotify.com/track/abc"), "music");
        assert_eq!(k("https://i.imgur.com/abc.JPG"), "image");
        assert_eq!(k("https://www.reddit.com/r/rust/comments/x"), "reddit");
        assert!(looks_like_code("fn main() {\n    println!(\"hi\");\n}"));
        assert!(!looks_like_code("Remember: the Karoo light is flat at noon\nand gold after five."));
    }

    #[test]
    fn titles() {
        assert_eq!(first_url("look https://youtu.be/abc123 wow").as_deref(), Some("https://youtu.be/abc123"));
        assert_eq!(first_url("no links"), None);
        assert_eq!(draft_title(Some("https://youtube.com/watch?v=abc123"), ""), "youtube.com · abc123");
        assert_eq!(draft_title(Some("https://example.com/some-long-slug"), ""), "some long slug");
        assert_eq!(draft_title(None, "First line\nsecond"), "First line");
    }
}
