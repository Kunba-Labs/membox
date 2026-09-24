//! The library's row shapes — spec §2. Serialised camelCase because the same
//! JSON feeds the desktop webview and the iOS app.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub domain: Option<String>,
    pub dedupe_key: String,
    /// Blob-relative paths ("ab/cdef….png"); the host turns them into URLs.
    pub thumb: Option<String>,
    pub page_shot: Option<String>,
    pub aspect: f64,
    pub tags: Vec<String>,
    /// The subset of `tags` a machine added (rules or the agent). A tag a
    /// person typed, or re-added, is theirs and leaves this list.
    #[serde(default)]
    pub auto_tags: Vec<String>,
    pub folder_ids: Vec<String>,
    pub rating: i64,
    pub duration: Option<String>,
    /// pending → fetching → enriching → ready | failed  (§1.6)
    pub status: String,
    pub error: Option<String>,
    pub body_html: Option<String>,
    /// Readable text (§3.1) — what search indexes.
    pub body_text: Option<String>,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub transcript: Option<String>,
    pub agent_reason: Option<String>,
    pub confidence: Option<f64>,
    pub added_at: String,
    pub last_seen_at: String,
    pub size: String,
    pub dimensions: String,
    pub palette: Vec<String>,
    pub trashed: bool,
    pub user_edited: bool,
    #[serde(default)]
    pub meta: serde_json::Value,
    /// Last-writer-wins clock for sync (RFC 3339, set by the store on every save).
    #[serde(default)]
    pub updated_at: String,
    /// Tombstone: a deleted row keeps its id so other devices delete it too.
    #[serde(default)]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
    pub parent_id: Option<String>,
    /// Suggested by the agent, not yet accepted by the user (§3.8).
    pub proposed: bool,
    pub position: i64,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: i64,
    pub item_id: String,
    pub adapter: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub prompt: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub folders: Vec<Folder>,
    pub settings: Settings,
    pub queue: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// "claude" | "codex" | "gemini" | "opencode" | "local" | "off"
    pub agent: String,
    pub local_model: String,
    pub auto_file_threshold: f64,
    /// How many agent runs at once (§3.9). Local machine, local rules.
    #[serde(default = "three")]
    pub agent_concurrency: i64,
    pub crawl_depth: i64,
    /// iCloud sync (§7.4 via iCloud Drive). Off until the person turns it on.
    #[serde(default)]
    pub sync_enabled: bool,
    /// Override for the shared folder; None = the platform default.
    #[serde(default)]
    pub sync_dir: Option<String>,
    /// "bauhaus" (default) | "glass" — §6.9. The look, not the palette: the
    /// two differ in shape and softness as much as in colour.
    #[serde(default = "bauhaus")]
    pub theme: String,
}

fn three() -> i64 {
    3
}

fn bauhaus() -> String {
    "bauhaus".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            agent: "off".into(),
            local_model: "qwen2.5:7b".into(),
            auto_file_threshold: 0.7,
            agent_concurrency: 3,
            theme: bauhaus(),
            crawl_depth: 0,
            sync_enabled: false,
            sync_dir: None,
        }
    }
}

/// What a paste or drop hands the core — §1.2, every flavour kept.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInput {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub html: String,
    /// PNG/JPEG bytes as base64 (the webview can't hand us a file path).
    pub image_base64: Option<String>,
    /// Any other pasted/dropped file, as base64. Kept as a blob.
    pub file_base64: Option<String>,
    pub file_name: Option<String>,
    pub source_app: Option<String>,
    /// What the person said about this one thing (§1.4 triage keeps the words
    /// that shared the line with a link).
    #[serde(default)]
    pub note: Option<String>,
    /// Triage's guess when the text is a name, not a URL: book | movie | tv |
    /// product. The URL still decides when there is one.
    #[serde(default)]
    pub kind: Option<String>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id(prefix: &str) -> String {
    use rand::RngCore;
    let mut b = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut b);
    format!("{prefix}-{}", hex(&b))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
