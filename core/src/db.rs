//! SQLite store. One file, WAL, FTS5 over the searchable text — spec §0.4/§5.1.
//!
//! ponytail: list-valued columns (tags, folder_ids, palette) are JSON text. The
//! UI filters from a full snapshot anyway, and it keeps the schema to three
//! tables. Split into join tables when a library outgrows a snapshot.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::model::{now, AgentRun, Folder, Item, Settings};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  url TEXT,
  domain TEXT,
  dedupe_key TEXT NOT NULL,
  thumb TEXT,
  page_shot TEXT,
  aspect REAL NOT NULL DEFAULT 1.333,
  tags TEXT NOT NULL DEFAULT '[]',
  folder_ids TEXT NOT NULL DEFAULT '[]',
  rating INTEGER NOT NULL DEFAULT 0,
  duration TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  error TEXT,
  body_html TEXT,
  body_text TEXT,
  summary TEXT,
  notes TEXT,
  transcript TEXT,
  agent_reason TEXT,
  confidence REAL,
  added_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  size TEXT NOT NULL DEFAULT '',
  dimensions TEXT NOT NULL DEFAULT '',
  palette TEXT NOT NULL DEFAULT '[]',
  trashed INTEGER NOT NULL DEFAULT 0,
  user_edited INTEGER NOT NULL DEFAULT 0,
  meta TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS items_dedupe ON items(dedupe_key);
CREATE INDEX IF NOT EXISTS items_added ON items(added_at);

CREATE TABLE IF NOT EXISTS folders (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  emoji TEXT,
  parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
  proposed INTEGER NOT NULL DEFAULT 0,
  position INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS agent_runs (
  id INTEGER PRIMARY KEY,
  item_id TEXT NOT NULL,
  adapter TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  prompt TEXT NOT NULL,
  output TEXT,
  error TEXT,
  ms INTEGER
);

CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY);

CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
  id UNINDEXED, title, summary, body_text, transcript, tags, notes,
  tokenize = 'unicode61 remove_diacritics 2'
);
"#;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    // ---- items ----

    pub fn insert_item(&self, it: &Item) -> rusqlite::Result<()> {
        self.write_item(it, true, true)
    }

    pub fn save_item(&self, it: &Item) -> rusqlite::Result<()> {
        self.write_item(it, false, true)
    }

    /// Sync writes keep the remote clock instead of stamping a new one.
    pub fn upsert_item_for_sync(&self, it: &Item) -> rusqlite::Result<()> {
        let exists = self.conn.query_row("SELECT 1 FROM items WHERE id=?1", params![it.id], |_| Ok(())).optional()?.is_some();
        self.write_item(it, !exists, false)
    }

    fn write_item(&self, it: &Item, insert: bool, stamp: bool) -> rusqlite::Result<()> {
        let updated_at = if stamp { now() } else { it.updated_at.clone() };
        if insert {
            self.conn.execute(
            "INSERT INTO items (id,kind,title,url,domain,dedupe_key,thumb,page_shot,aspect,tags,folder_ids,rating,duration,status,error,body_html,body_text,summary,notes,transcript,agent_reason,confidence,added_at,last_seen_at,size,dimensions,palette,trashed,user_edited,meta,updated_at,deleted_at,auto_tags)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)",
            params![
                it.id, it.kind, it.title, it.url, it.domain, it.dedupe_key, it.thumb, it.page_shot, it.aspect,
                json(&it.tags), json(&it.folder_ids), it.rating, it.duration, it.status, it.error, it.body_html,
                it.body_text, it.summary, it.notes, it.transcript, it.agent_reason, it.confidence, it.added_at,
                it.last_seen_at, it.size, it.dimensions, json(&it.palette), it.trashed as i64, it.user_edited as i64,
                it.meta.to_string(), updated_at, it.deleted_at, json(&it.auto_tags)
            ],
            )?;
        } else {
            self.conn.execute(
            "UPDATE items SET kind=?2,title=?3,url=?4,domain=?5,dedupe_key=?6,thumb=?7,page_shot=?8,aspect=?9,tags=?10,folder_ids=?11,rating=?12,duration=?13,status=?14,error=?15,body_html=?16,body_text=?17,summary=?18,notes=?19,transcript=?20,agent_reason=?21,confidence=?22,added_at=?23,last_seen_at=?24,size=?25,dimensions=?26,palette=?27,trashed=?28,user_edited=?29,meta=?30,updated_at=?31,deleted_at=?32,auto_tags=?33 WHERE id=?1",
            params![
                it.id, it.kind, it.title, it.url, it.domain, it.dedupe_key, it.thumb, it.page_shot, it.aspect,
                json(&it.tags), json(&it.folder_ids), it.rating, it.duration, it.status, it.error, it.body_html,
                it.body_text, it.summary, it.notes, it.transcript, it.agent_reason, it.confidence, it.added_at,
                it.last_seen_at, it.size, it.dimensions, json(&it.palette), it.trashed as i64, it.user_edited as i64,
                it.meta.to_string(), updated_at, it.deleted_at, json(&it.auto_tags)
            ],
            )?;
        }
        self.reindex(&it.id)
    }

    fn reindex(&self, id: &str) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM items_fts WHERE id=?1", params![id])?;
        self.conn.execute(
            "INSERT INTO items_fts (id,title,summary,body_text,transcript,tags,notes)
             SELECT id,title,coalesce(summary,''),coalesce(body_text,''),coalesce(transcript,''),tags,coalesce(notes,'') FROM items WHERE id=?1 AND deleted_at IS NULL",
            params![id],
        )?;
        Ok(())
    }

    pub fn get_item(&self, id: &str) -> rusqlite::Result<Option<Item>> {
        self.conn
            .query_row(&format!("SELECT {ITEM_COLS} FROM items WHERE id=?1 AND deleted_at IS NULL"), params![id], row_to_item)
            .optional()
    }

    pub fn get_item_for_sync(&self, id: &str) -> rusqlite::Result<Option<Item>> {
        self.conn
            .query_row(&format!("SELECT {ITEM_COLS} FROM items WHERE id=?1"), params![id], row_to_item)
            .optional()
    }

    pub fn all_items_for_sync(&self) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.conn.prepare(&format!("SELECT {ITEM_COLS} FROM items"))?;
        let v = st.query_map([], row_to_item)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn find_by_dedupe(&self, key: &str) -> rusqlite::Result<Option<Item>> {
        self.conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE dedupe_key=?1 AND trashed=0 AND deleted_at IS NULL LIMIT 1"),
                params![key],
                row_to_item,
            )
            .optional()
    }

    pub fn all_items(&self) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.conn.prepare(&format!("SELECT {ITEM_COLS} FROM items WHERE deleted_at IS NULL ORDER BY added_at DESC"))?;
        let v = st.query_map([], row_to_item)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Tombstone, not DELETE: the row keeps its id (and nothing else of
    /// substance) so other devices learn about the deletion.
    pub fn delete_items(&self, ids: &[String]) -> rusqlite::Result<()> {
        for id in ids {
            self.conn.execute(
                "UPDATE items SET deleted_at=?2, updated_at=?2, trashed=1, body_html=NULL, body_text=NULL, transcript=NULL, summary=NULL, notes=NULL, thumb=NULL, page_shot=NULL WHERE id=?1",
                params![id, now()],
            )?;
            self.conn.execute("DELETE FROM items_fts WHERE id=?1", params![id])?;
        }
        Ok(())
    }

    pub fn delete_trashed(&self) -> rusqlite::Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT id FROM items WHERE trashed=1 AND deleted_at IS NULL")?;
        let ids: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        self.delete_items(&ids)?;
        Ok(ids)
    }

    /// FTS5 match → ids, best first. Free text only; the prefix filters
    /// (`kind:` `tag:` …) are applied by the caller over the snapshot.
    pub fn search(&self, q: &str) -> rusqlite::Result<Vec<String>> {
        let terms: Vec<String> = q
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect();
        if terms.is_empty() {
            return Ok(vec![]);
        }
        let mut st = self
            .conn
            .prepare("SELECT id FROM items_fts WHERE items_fts MATCH ?1 ORDER BY rank LIMIT 500")?;
        let v = st.query_map(params![terms.join(" ")], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---- folders ----

    pub fn all_folders(&self) -> rusqlite::Result<Vec<Folder>> {
        let mut st = self
            .conn
            .prepare("SELECT id,name,emoji,parent_id,proposed,position,updated_at,deleted_at FROM folders WHERE deleted_at IS NULL ORDER BY position, name")?;
        let v = st.query_map([], row_to_folder)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn all_folders_for_sync(&self) -> rusqlite::Result<Vec<Folder>> {
        let mut st = self.conn.prepare("SELECT id,name,emoji,parent_id,proposed,position,updated_at,deleted_at FROM folders")?;
        let v = st.query_map([], row_to_folder)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn get_folder_for_sync(&self, id: &str) -> rusqlite::Result<Option<Folder>> {
        self.conn
            .query_row("SELECT id,name,emoji,parent_id,proposed,position,updated_at,deleted_at FROM folders WHERE id=?1", params![id], row_to_folder)
            .optional()
    }

    pub fn upsert_folder_for_sync(&self, f: &Folder) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO folders (id,name,emoji,parent_id,proposed,position,updated_at,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, emoji=excluded.emoji, parent_id=excluded.parent_id, proposed=excluded.proposed, position=excluded.position, updated_at=excluded.updated_at, deleted_at=excluded.deleted_at",
            params![f.id, f.name, f.emoji, f.parent_id, f.proposed as i64, f.position, f.updated_at, f.deleted_at],
        )?;
        Ok(())
    }

    pub fn insert_folder(&self, f: &Folder) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO folders (id,name,emoji,parent_id,proposed,position,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![f.id, f.name, f.emoji, f.parent_id, f.proposed as i64, f.position, now()],
        )?;
        Ok(())
    }

    /// Including tombstones: a folder that was deleted on purpose is still
    /// "known", and must not be conjured back.
    pub fn folder_exists(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT 1 FROM folders WHERE id=?1", params![id], |_| Ok(())).optional().map(|o| o.is_some())
    }

    pub fn update_folder(&self, id: &str, name: Option<&str>, proposed: Option<bool>) -> rusqlite::Result<()> {
        if let Some(n) = name {
            self.conn.execute("UPDATE folders SET name=?2, updated_at=?3 WHERE id=?1", params![id, n, now()])?;
        }
        if let Some(p) = proposed {
            self.conn.execute("UPDATE folders SET proposed=?2, updated_at=?3 WHERE id=?1", params![id, p as i64, now()])?;
        }
        Ok(())
    }

    /// Deletes the folder and its children; returns every id removed so callers
    /// can scrub `folder_ids` on items.
    pub fn delete_folder(&self, id: &str) -> rusqlite::Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT id FROM folders WHERE parent_id=?1 AND deleted_at IS NULL")?;
        let mut gone: Vec<String> = st.query_map(params![id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        gone.push(id.to_string());
        for g in &gone {
            self.conn.execute("UPDATE folders SET deleted_at=?2, updated_at=?2 WHERE id=?1", params![g, now()])?;
        }
        Ok(gone)
    }

    pub fn find_folder_by_name(&self, name: &str, parent: Option<&str>) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT id FROM folders WHERE lower(name)=lower(?1) AND coalesce(parent_id,'')=coalesce(?2,'') AND deleted_at IS NULL LIMIT 1",
                params![name, parent],
                |r| r.get(0),
            )
            .optional()
    }

    // ---- agent runs ----

    pub fn start_run(&self, item_id: &str, adapter: &str, prompt: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO agent_runs (item_id,adapter,status,started_at,prompt) VALUES (?1,?2,'running',?3,?4)",
            params![item_id, adapter, now(), prompt],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, id: i64, status: &str, output: Option<&str>, error: Option<&str>, ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE agent_runs SET status=?2, finished_at=?3, output=?4, error=?5, ms=?6 WHERE id=?1",
            params![id, status, now(), output, error, ms],
        )?;
        Ok(())
    }

    pub fn runs_for(&self, item_id: &str) -> rusqlite::Result<Vec<AgentRun>> {
        let mut st = self.conn.prepare(
            "SELECT id,item_id,adapter,status,started_at,finished_at,prompt,output,error,ms FROM agent_runs WHERE item_id=?1 ORDER BY id DESC",
        )?;
        let v = st
            .query_map(params![item_id], |r| {
                Ok(AgentRun {
                    id: r.get(0)?,
                    item_id: r.get(1)?,
                    adapter: r.get(2)?,
                    status: r.get(3)?,
                    started_at: r.get(4)?,
                    finished_at: r.get(5)?,
                    prompt: r.get(6)?,
                    output: r.get(7)?,
                    error: r.get(8)?,
                    ms: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---- settings ----

    pub fn settings(&self) -> rusqlite::Result<Settings> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key='settings'", [], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    pub fn save_settings(&self, s: &Settings) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key,value) VALUES ('settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![serde_json::to_string(s).unwrap_or_default()],
        )?;
        Ok(())
    }
}

const ITEM_COLS: &str = "id,kind,title,url,domain,dedupe_key,thumb,page_shot,aspect,tags,folder_ids,rating,duration,status,error,body_html,body_text,summary,notes,transcript,agent_reason,confidence,added_at,last_seen_at,size,dimensions,palette,trashed,user_edited,meta,updated_at,deleted_at,auto_tags";

/// Columns added after the first schema. `ADD COLUMN` is idempotent through
/// the migrations table; the schema constant stays the day-one shape.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for (name, sql) in [
        ("items_sync_clock", "ALTER TABLE items ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''; ALTER TABLE items ADD COLUMN deleted_at TEXT; UPDATE items SET updated_at = last_seen_at WHERE updated_at = '';"),
        ("folders_sync_clock", "ALTER TABLE folders ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''; ALTER TABLE folders ADD COLUMN deleted_at TEXT;"),
        ("items_auto_tags", "ALTER TABLE items ADD COLUMN auto_tags TEXT NOT NULL DEFAULT '[]';"),
    ] {
        let done: Option<String> = conn.query_row("SELECT name FROM schema_migrations WHERE name=?1", params![name], |r| r.get(0)).optional()?;
        if done.is_none() {
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO schema_migrations (name) VALUES (?1)", params![name])?;
        }
    }
    Ok(())
}

fn row_to_folder(r: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        name: r.get(1)?,
        emoji: r.get(2)?,
        parent_id: r.get(3)?,
        proposed: r.get::<_, i64>(4)? != 0,
        position: r.get(5)?,
        updated_at: r.get(6)?,
        deleted_at: r.get(7)?,
    })
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    let list = |i: usize| -> rusqlite::Result<Vec<String>> {
        let s: String = r.get(i)?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    };
    Ok(Item {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        url: r.get(3)?,
        domain: r.get(4)?,
        dedupe_key: r.get(5)?,
        thumb: r.get(6)?,
        page_shot: r.get(7)?,
        aspect: r.get(8)?,
        tags: list(9)?,
        folder_ids: list(10)?,
        rating: r.get(11)?,
        duration: r.get(12)?,
        status: r.get(13)?,
        error: r.get(14)?,
        body_html: r.get(15)?,
        body_text: r.get(16)?,
        summary: r.get(17)?,
        notes: r.get(18)?,
        transcript: r.get(19)?,
        agent_reason: r.get(20)?,
        confidence: r.get(21)?,
        added_at: r.get(22)?,
        last_seen_at: r.get(23)?,
        size: r.get(24)?,
        dimensions: r.get(25)?,
        palette: list(26)?,
        trashed: r.get::<_, i64>(27)? != 0,
        user_edited: r.get::<_, i64>(28)? != 0,
        meta: r.get::<_, String>(29).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null),
        updated_at: r.get(30)?,
        deleted_at: r.get(31)?,
        auto_tags: list(32)?,
    })
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}
