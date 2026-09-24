//! The in-core MCP server — spec §4.7. JSON-RPC over loopback HTTP (MCP
//! Streamable HTTP, JSON-only: no server-initiated messages, so no SSE).
//! Transport + origin/host/token gates lifted from inbox2's `mcp::http`.
//!
//! Scope: the agent under enrichment may only write the item it was given
//! (§4.8). Every call is appended to the audit log.

use std::io::Cursor;
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::browser::EXTRACT_JS;
use crate::Library;

#[derive(Debug, Deserialize)]
pub struct Rpc {
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub at: String,
    pub tool: String,
    pub args: Value,
    pub ok: bool,
}

pub struct McpServer {
    lib: Arc<Library>,
    pub token: String,
    pub port: u16,
    /// One entry per enrichment run in flight: its own bearer token, and the one
    /// item it may write. Runs go in parallel now, so a single global scope
    /// would let one item's agent write another's.
    pub runs: Mutex<HashMap<String, String>>,
    pub audit: Mutex<Vec<AuditEntry>>,
}

impl McpServer {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    /// A token for one enrichment run, good for that item only.
    pub fn open_run(&self, item_id: &str) -> String {
        let t = crate::model::hex(&rand::random::<[u8; 24]>());
        self.runs.lock().insert(t.clone(), item_id.to_string());
        t
    }

    pub fn close_run(&self, token: &str) {
        self.runs.lock().remove(token);
    }

    /// Bind an ephemeral loopback port and serve until the process exits.
    pub fn spawn(lib: Arc<Library>) -> Result<Arc<McpServer>, String> {
        let http = Server::http("127.0.0.1:0").map_err(|e| format!("mcp bind: {e}"))?;
        let port = match http.server_addr() {
            tiny_http::ListenAddr::IP(a) => a.port(),
            _ => return Err("mcp: no ip addr".into()),
        };
        let token = crate::model::hex(&rand::random::<[u8; 24]>());
        let server = Arc::new(McpServer { lib, token, port, runs: Mutex::new(HashMap::new()), audit: Mutex::new(Vec::new()) });
        let s2 = server.clone();
        std::thread::Builder::new()
            .name("membox-mcp".into())
            .spawn(move || {
                for req in http.incoming_requests() {
                    handle(&s2, req);
                }
            })
            .map_err(|e| e.to_string())?;
        log::info!("membox MCP on {}", server.url());
        let cfg = json!({ "mcpServers": { "membox": { "type": "http", "url": server.url(), "headers": { "Authorization": format!("Bearer {}", server.token) } } } });
        let path = server.lib.data_dir.join("mcp.json");
        if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()) {
            log::warn!("mcp.json: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
        Ok(server)
    }

    pub fn handle_rpc(&self, rpc: &Rpc, caller: &str) -> Option<Value> {
        let id = rpc.id.clone()?; // notifications get no response
        let result = match rpc.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": rpc.params["protocolVersion"].as_str().unwrap_or("2025-03-26"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "membox", "version": env!("CARGO_PKG_VERSION") }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_defs() })),
            "tools/call" => {
                let name = rpc.params["name"].as_str().unwrap_or("");
                let args = rpc.params["arguments"].clone();
                let r = self.call(name, &args, caller);
                self.audit.lock().push(AuditEntry { at: crate::model::now(), tool: name.into(), args: args.clone(), ok: r.is_ok() });
                Ok(match r {
                    Ok(v) => v,
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                })
            }
            _ => Err((-32601, format!("Method not found: {}", rpc.method))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, msg)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }),
        })
    }

    fn call(&self, name: &str, a: &Value, caller: &str) -> Result<Value, String> {
        let s = |k: &str| a[k].as_str().map(str::to_string);
        let text_out = |t: String| json!({ "content": [{ "type": "text", "text": t }] });
        let b = &self.lib.browser;
        // One browser, several agents: whoever is mid-call keeps it.
        // ponytail: per-call, not per-run. Two agents can interleave pages; if
        // that ever bites, hand each run its own window.
        let _drive = matches!(name, "navigate" | "screenshot" | "screenshot_fullpage" | "extract_readable" | "extract_links" | "scroll" | "click")
            .then(|| self.lib.browser_lock.lock());
        match name {
            // ---- browser ----
            "navigate" => {
                let url = s("url").ok_or("url required")?;
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err("only http(s) urls".into());
                }
                b.navigate(&url).map_err(|e| e.to_string())?;
                // Same clean page the fetch stage photographs (§4.5).
                let consent = crate::browser::dismiss_consent(&**b);
                log::info!("  consent on {url}: {consent}");
                let meta = b.eval(EXTRACT_JS).map_err(|e| e.to_string())?;
                let v: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
                Ok(text_out(json!({ "url": url, "title": v["title"], "height": v["height"] }).to_string()))
            }
            "screenshot" | "screenshot_fullpage" => {
                let png = b.snapshot_png(name == "screenshot_fullpage").map_err(|e| e.to_string())?;
                let rel = self.lib.put_blob(&png, "png").map_err(|e| e.to_string())?;
                let abs = self.lib.blob_path(&rel);
                use base64::Engine;
                Ok(json!({ "content": [
                    { "type": "text", "text": format!("saved to {}", abs.display()) },
                    { "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png" }
                ] }))
            }
            "extract_readable" => {
                let meta = b.eval(EXTRACT_JS).map_err(|e| e.to_string())?;
                let v: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
                Ok(text_out(format!("# {}\n\n{}\n\n{}", v["title"].as_str().unwrap_or(""), v["description"].as_str().unwrap_or(""), v["text"].as_str().unwrap_or(""))))
            }
            "extract_links" => {
                let meta = b.eval(EXTRACT_JS).map_err(|e| e.to_string())?;
                let v: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
                Ok(text_out(v["links"].to_string()))
            }
            "scroll" => {
                let y = a["y"].as_f64().unwrap_or(800.0);
                b.eval(&format!("(()=>{{window.scrollBy(0,{y});return String(window.scrollY)}})()")).map(text_out).map_err(|e| e.to_string())
            }
            "click" => {
                let sel = s("selector").ok_or("selector required")?;
                let js = format!("(()=>{{const e=document.querySelector({});if(!e)return 'no match';e.click();return 'clicked'}})()", json!(sel));
                b.eval(&js).map(text_out).map_err(|e| e.to_string())
            }
            "wait_for" => {
                let sel = s("selector").ok_or("selector required")?;
                for _ in 0..40 {
                    let r = b.eval(&format!("String(!!document.querySelector({}))", json!(sel))).map_err(|e| e.to_string())?;
                    if r.contains("true") {
                        return Ok(text_out("found".into()));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                Err("timeout".into())
            }
            // ---- library (read) ----
            "list_folders" => Ok(text_out(serde_json::to_string(&self.lib.db.lock().all_folders().map_err(|e| e.to_string())?).unwrap())),
            "list_tags" => Ok(text_out(serde_json::to_string(&self.lib.tag_counts()).unwrap())),
            "search" => {
                let q = s("query").ok_or("query required")?;
                let ids = self.lib.db.lock().search(&q).map_err(|e| e.to_string())?;
                let db = self.lib.db.lock();
                let hits: Vec<Value> = ids
                    .iter()
                    .take(20)
                    .filter_map(|id| db.get_item(id).ok().flatten())
                    .map(|i| json!({ "id": i.id, "title": i.title, "url": i.url, "kind": i.kind, "tags": i.tags, "summary": i.summary }))
                    .collect();
                Ok(text_out(serde_json::to_string(&hits).unwrap()))
            }
            "get_item" => {
                let id = s("id").ok_or("id required")?;
                let it = self.lib.db.lock().get_item(&id).map_err(|e| e.to_string())?.ok_or("not found")?;
                Ok(text_out(serde_json::to_string(&json!({ "id": it.id, "title": it.title, "url": it.url, "kind": it.kind, "tags": it.tags, "folderIds": it.folder_ids, "summary": it.summary, "bodyText": it.body_text.map(|t| t.chars().take(8000).collect::<String>()) })).unwrap()))
            }
            // ---- library (write, scoped) ----
            "set_item" => {
                let id = s("id").ok_or("id required")?;
                self.check_scope(&id, caller)?;
                self.lib.apply_agent_patch(&id, a).map_err(|e| e.to_string())?;
                Ok(text_out("ok".into()))
            }
            // Unscoped on purpose: this is how an outside agent says "save this
            // to membox" — the item is top-level and goes through the normal
            // pipeline. Everything else that writes stays scoped.
            "capture" => {
                let text = s("text").or_else(|| s("url")).ok_or("text or url required")?;
                let id = self.lib.capture(crate::model::CaptureInput { text, html: s("html").unwrap_or_default(), ..Default::default() }).map_err(|e| e.to_string())?;
                Ok(text_out(id))
            }
            "create_item" => {
                let url = s("url").ok_or("url required")?;
                let parent = self.runs.lock().get(caller).cloned().ok_or("no active run")?;
                let id = self.lib.capture_child(&url, &parent).map_err(|e| e.to_string())?;
                Ok(text_out(id))
            }
            "create_folder" => {
                let name = s("name").ok_or("name required")?;
                let id = self.lib.create_folder(&name, s("parentId").as_deref(), s("emoji").as_deref(), true).map_err(|e| e.to_string())?;
                Ok(text_out(id))
            }
            _ => Err(format!("unknown tool {name}")),
        }
    }

    fn check_scope(&self, id: &str, caller: &str) -> Result<(), String> {
        match self.runs.lock().get(caller) {
            Some(s) if s == id => Ok(()),
            _ => Err("this run may only write the item it was given".into()),
        }
    }
}

fn tool_defs() -> Vec<Value> {
    let t = |name: &str, desc: &str, props: Value, req: &[&str]| {
        json!({ "name": name, "description": desc, "inputSchema": { "type": "object", "properties": props, "required": req } })
    };
    let str_p = |d: &str| json!({ "type": "string", "description": d });
    vec![
        t("navigate", "Load a URL in membox's browser and return its title and height.", json!({ "url": str_p("http(s) URL") }), &["url"]),
        t("screenshot", "PNG of the current viewport.", json!({}), &[]),
        t("screenshot_fullpage", "PNG of the whole document.", json!({}), &[]),
        t("extract_readable", "Readable text of the current page.", json!({}), &[]),
        t("extract_links", "All http links on the current page.", json!({}), &[]),
        t("scroll", "Scroll the page by y pixels.", json!({ "y": { "type": "number" } }), &[]),
        t("click", "Click the first element matching a CSS selector.", json!({ "selector": str_p("CSS selector") }), &["selector"]),
        t("wait_for", "Wait up to 10s for a selector to appear.", json!({ "selector": str_p("CSS selector") }), &["selector"]),
        t("list_folders", "Folder tree with ids.", json!({}), &[]),
        t("list_tags", "Tags in use with counts.", json!({}), &[]),
        t("search", "Search the library.", json!({ "query": str_p("free text") }), &["query"]),
        t("get_item", "One item by id.", json!({ "id": str_p("item id") }), &["id"]),
        t("set_item", "Update the item under enrichment: title, summary, tags, folderId, reason.", json!({ "id": str_p("item id"), "title": str_p(""), "summary": str_p(""), "tags": { "type": "array", "items": { "type": "string" } }, "folderId": str_p("existing folder id"), "reason": str_p("") }), &["id"]),
        t("capture", "Save a URL or text to membox as a new item; it is screenshotted, indexed and filed like a paste.", json!({ "url": str_p("http(s) URL"), "text": str_p("or any pasted text") }), &[]),
        t("create_item", "Save a linked page as its own item, related to the one being enriched (deep crawl).", json!({ "url": str_p("http(s) URL") }), &["url"]),
        t("create_folder", "Propose a new folder. It appears as a suggestion the person accepts or rejects.", json!({ "name": str_p(""), "parentId": str_p("parent folder id, optional"), "emoji": str_p("") }), &["name"]),
    ]
}

// ---- HTTP transport ----

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn respond_json(status: u16, body: String) -> Response<Cursor<Vec<u8>>> {
    let mut r = Response::from_string(body).with_status_code(status);
    r.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    r
}

fn handle(server: &Arc<McpServer>, mut req: Request) {
    let path = req.url().split('?').next().unwrap_or("");
    if req.method() == &Method::Get && path == "/health" {
        let _ = req.respond(Response::from_string("ok"));
        return;
    }
    if req.method() != &Method::Post || path != "/mcp" {
        let _ = req.respond(Response::from_string("not found").with_status_code(404));
        return;
    }
    // DNS-rebinding defence: a real web Origin, or a non-loopback Host, is refused.
    if !origin_ok(header(&req, "Origin")) || !host_ok(header(&req, "Host")) {
        let _ = req.respond(respond_json(403, r#"{"error":"forbidden origin"}"#.into()));
        return;
    }
    // The server token (outside agents), or a run's own token (enrichment).
    let caller = header(&req, "Authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_string)).unwrap_or_default();
    let authed = ct_eq(&caller, &server.token) || server.runs.lock().contains_key(&caller);
    if !authed {
        let _ = req.respond(respond_json(401, r#"{"error":"unauthorized"}"#.into()));
        return;
    }
    let mut body = String::new();
    if req.as_reader().read_to_string(&mut body).is_err() {
        let _ = req.respond(respond_json(400, r#"{"error":"unreadable body"}"#.into()));
        return;
    }
    let rpc: Rpc = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            let _ = req.respond(respond_json(200, json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Parse error: {e}") } }).to_string()));
            return;
        }
    };
    match server.handle_rpc(&rpc, &caller) {
        Some(v) => {
            let _ = req.respond(respond_json(200, v.to_string()));
        }
        None => {
            let _ = req.respond(Response::from_string("").with_status_code(202));
        }
    }
}

fn loopback(hostport: &str) -> bool {
    let host = if let Some(rest) = hostport.strip_prefix('[') { rest.split(']').next().unwrap_or("") } else { hostport.split(':').next().unwrap_or("") };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}
fn origin_ok(o: Option<&str>) -> bool {
    match o {
        None => true,
        Some("null") => true,
        Some(o) => loopback(o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).unwrap_or(o)),
    }
}
fn host_ok(h: Option<&str>) -> bool {
    h.map(loopback).unwrap_or(true)
}
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gates() {
        assert!(ct_eq("tok", "tok") && !ct_eq("tok", "toK") && !ct_eq("a", "ab"));
        assert!(origin_ok(None) && origin_ok(Some("http://127.0.0.1:1")) && !origin_ok(Some("https://evil.example")));
        assert!(host_ok(Some("localhost:9")) && host_ok(Some("[::1]:9")) && !host_ok(Some("evil.example:9")));
    }
}
