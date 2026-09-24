/* The library store — one API, two backends.
 *
 * In the Tauri app every write is `invoke("dispatch", {action, args})` into
 * the Rust core (SQLite, the enrichment queue, the MCP server), and the state
 * here is a snapshot refreshed on the core's `library-changed` event.
 *
 * In a plain browser (`yarn dev`) the same action names run against an
 * in-memory copy persisted to localStorage, seeded from mock.js, with a faked
 * enrichment timer — so the UI is workable without the native shell.
 *
 * Components see neither: they call `actions.*` and read `useStore()`. */

import { useSyncExternalStore } from "react";
import * as seed from "./mock.js";
import { canonicalUrl, detectKind, firstUrl, draftTitle, dedupeKey, extractUrls, looksLikeCode } from "./capture.js";

export const tauri = typeof window !== "undefined" && !!window.__TAURI_INTERNALS__;

let state = { items: [], folders: [], settings: {}, queue: [], ready: false };
const listeners = new Set();
const emit = () => listeners.forEach((l) => l());

export function useStore(selector = (s) => s) {
  return useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => selector(state),
    () => selector(state)
  );
}
export const getState = () => state;

// ---------------------------------------------------------------------------
// Tauri backend
// ---------------------------------------------------------------------------

let invoke, convertFileSrc, listen, blobs = "";

async function tauriInit() {
  ({ invoke, convertFileSrc } = await import("@tauri-apps/api/core"));
  ({ listen } = await import("@tauri-apps/api/event"));
  blobs = await invoke("blobs_dir");
  // Every outbound link in the app is `window.open(url, "_blank")` or an
  // `<a target="_blank">`; WKWebView drops both, so route them to the OS.
  window.open = (url) => invoke("open_url", { url: String(url) });
  document.addEventListener("click", (e) => {
    const a = e.target.closest?.("a[target=_blank]");
    if (a) { e.preventDefault(); window.open(a.href); }
  }, true); // capture: React's stopPropagation in a sheet never reaches document
  await refresh();
  let t;
  await listen("library-changed", () => {
    clearTimeout(t);
    t = setTimeout(refresh, 80);
  });
  // A release build polls latest.json (updater.rs) and says when one is waiting.
  await listen("update://available", (e) => {
    state = { ...state, update: e.payload };
    emit();
  });
}

const dispatch = (action, args = {}) => invoke("dispatch", { action, args });

async function refresh() {
  const s = await dispatch("snapshot");
  const blobUrl = (rel) => (rel ? convertFileSrc(`${blobs}/${rel}`) : null);
  // Playlist entries past the first four keep their i.ytimg.com url (§3.3).
  const entryThumb = (t) => (t && !t.startsWith("http") ? blobUrl(t) : t);
  const withPlaylist = (i) =>
    i.meta?.playlist ? { ...i, meta: { ...i.meta, playlist: i.meta.playlist.map((e) => ({ ...e, thumb: entryThumb(e.thumb) })) } } : i;
  state = {
    items: s.items.map((i) => withPlaylist({ ...i, thumb: blobUrl(i.thumb), pageShot: blobUrl(i.pageShot) })),
    folders: nest(s.folders),
    settings: s.settings,
    queue: s.queue,
    update: state.update,
    ready: true,
  };
  emit();
}

// Core folders are flat (parentId); the sidebar wants groups with children.
function nest(flat) {
  const groups = flat.filter((f) => !f.parentId).map((f) => ({ ...f, children: [] }));
  const byId = Object.fromEntries(groups.map((g) => [g.id, g]));
  flat.filter((f) => f.parentId).forEach((f) => byId[f.parentId]?.children.push({ ...f }));
  return groups;
}

const remote = {
  update: (id, patch) => dispatch("update", { id, patch }),
  setRating: (id, rating) => dispatch("setRating", { id, rating }),
  addTag: (id, tag) => dispatch("addTag", { id, tag }),
  removeTag: (id, tag) => dispatch("removeTag", { id, tag }),
  addToFolder: (ids, folderId, from = null) => dispatch("addToFolder", { ids, folderId, from }),
  removeFromFolder: (id, folderId) => dispatch("removeFromFolder", { id, folderId }),
  trash: (ids, trashed = true) => dispatch("trash", { ids, trashed }),
  deleteForever: (ids) => dispatch("deleteForever", { ids }),
  emptyTrash: () => dispatch("emptyTrash"),
  capture: (input) => dispatch("capture", input),
  newNote: (title = "New note") => dispatch("newNote", { title }),
  plan: (text, html = "", note = "") => dispatch("plan", { text, html, note }),
  capturePlan: (plan, note = "") => dispatch("capturePlan", { plan, note }),
  annotate: (ids, note) => dispatch("annotate", { ids, note }),
  reenrich: (ids) => dispatch("reenrich", { ids }),
  refetch: (ids) => dispatch("refetch", { ids }),
  createFolder: (name, parentId = null, emoji = null) => dispatch("createFolder", { name, parentId, emoji }),
  renameFolder: (id, name) => dispatch("renameFolder", { id, name }),
  acceptFolder: (id) => dispatch("acceptFolder", { id }),
  deleteFolder: (id) => dispatch("deleteFolder", { id }),
  settings: (patch) => dispatch("settings", patch),
  agents: () => dispatch("agents"),
  runs: (id) => dispatch("runs", { id }),
  mcp: () => dispatch("mcp"),
  paths: () => dispatch("paths"),
  sync: () => dispatch("sync"),
  syncStatus: () => dispatch("syncStatus"),
  reset: () => dispatch("reset"),
  version: () => import("@tauri-apps/api/app").then((m) => m.getVersion()),
  checkUpdate: () => invoke("check_for_updates"),
  installUpdate: () => invoke("install_update"),
  fake: false,
};

// ---------------------------------------------------------------------------
// Browser fallback (localStorage) — same names, same shapes.
// ---------------------------------------------------------------------------

const KEY = "membox.library.v1";

function localLoad() {
  try {
    const raw = localStorage.getItem(KEY);
    // Older persisted shapes may lack keys added since; fill them in.
    if (raw) return { settings: { agent: "off" }, queue: [], ...JSON.parse(raw), ready: true };
  } catch {}
  const folderId = (i) => {
    for (const g of seed.folders) for (const c of g.children) if (c.name === i.folders[0]) return c.id;
    return null;
  };
  return {
    items: seed.items.map((i) => ({ ...i, status: "ready", trashed: false, folderIds: [folderId(i)] })),
    folders: seed.folders,
    settings: { agent: "off" },
    queue: [],
    nextId: seed.items.length,
    ready: true,
  };
}

function commit(next) {
  state = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {}
  emit();
}

const patchItem = (id, fn) => ({ ...state, items: state.items.map((i) => (i.id === id ? { ...i, ...fn(i) } : i)) });
const now = () => new Date().toISOString();
const stripHtml = (h) => h.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();

const local = {
  fake: true,
  async update(id, patch) {
    commit(patchItem(id, () => ({ ...patch, userEdited: true })));
  },
  async setRating(id, rating) {
    commit(patchItem(id, (i) => ({ rating: i.rating === rating ? 0 : rating })));
  },
  async addTag(id, tag) {
    tag = tag.trim().toLowerCase();
    if (tag) commit(patchItem(id, (i) => ({ tags: i.tags.includes(tag) ? i.tags : [...i.tags, tag], autoTags: (i.autoTags || []).filter((t) => t !== tag) })));
  },
  async removeTag(id, tag) {
    commit(patchItem(id, (i) => ({ tags: i.tags.filter((t) => t !== tag), autoTags: (i.autoTags || []).filter((t) => t !== tag) })));
  },
  async addToFolder(ids, folderId, from = null) {
    commit({
      ...state,
      items: state.items.map((i) => (ids.includes(i.id) && !i.folderIds.includes(folderId) ? { ...i, folderIds: [...i.folderIds, folderId] } : i)),
      folders: state.folders.map((g) => ({ ...g, proposed: g.id === folderId ? false : g.proposed, children: g.children.map((c) => (c.id === folderId ? { ...c, proposed: false } : c)) })),
    });
  },
  async removeFromFolder(id, folderId) {
    commit(patchItem(id, (i) => ({ folderIds: i.folderIds.filter((f) => f !== folderId) })));
  },
  async trash(ids, trashed = true) {
    commit({ ...state, items: state.items.map((i) => (ids.includes(i.id) ? { ...i, trashed } : i)) });
  },
  async deleteForever(ids) {
    commit({ ...state, items: state.items.filter((i) => !ids.includes(i.id)) });
  },
  async emptyTrash() {
    commit({ ...state, items: state.items.filter((i) => !i.trashed) });
  },
  // Rules only in the browser fallback — there is no agent lane here.
  async plan(text, html = "", note = "") {
    const lines = text.split("\n").map((l) => l.trim()).filter(Boolean);
    const entries = [];
    let intent = null;
    let rest = lines;
    if (lines.length > 1 && lines[0].endsWith(":") && !extractUrls(lines[0]).length) {
      intent = lines[0].slice(0, -1);
      rest = lines.slice(1);
    }
    for (const line of rest) {
      const urls = extractUrls(line);
      if (!urls.length) entries.push({ kind: "snippet", text: line });
      else urls.forEach((u, i) => entries.push({ kind: detectKind(canonicalUrl(u)) || "url", text: u, note: i === 0 ? line.replace(u, "").replace(/^https?:\/\//, "").trim() || null : null }));
    }
    if (!entries.length && html) for (const u of extractUrls("", html)) entries.push({ kind: "url", text: u });
    return { entries, intent, source: "rules" };
  },
  async capturePlan(plan, note = "") {
    const ids = [];
    for (const e of plan.entries) ids.push(await local.captureOne({ text: e.text, note: [note || plan.intent, e.note].filter(Boolean).join(" — ") || null }));
    return { ids };
  },
  async annotate(ids, note) {
    commit({ ...state, items: state.items.map((i) => (ids.includes(i.id) ? { ...i, notes: [note, i.notes].filter(Boolean).join(" — ") } : i)) });
  },
  async newNote(title = "New note") {
    const id = `i-${state.nextId}`;
    const item = { id, kind: "note", title, url: null, domain: "note", dedupeKey: id, thumb: null, pageShot: null, aspect: 1,
      tags: [], autoTags: [], folderIds: [], rating: 0, duration: null, status: "ready", bodyHtml: "", bodyText: "", summary: "",
      addedAt: now(), lastSeenAt: now(), size: "—", dimensions: "—", palette: ["#f0b323"], trashed: false, meta: {} };
    commit({ ...state, items: [item, ...state.items], nextId: state.nextId + 1 });
    return id;
  },
  async capture(input) {
    const urls = extractUrls(input.text, input.html);
    if (urls.length > 1 && !input.imageBase64 && !input.fileBase64) {
      let first = null;
      for (const u of urls) first ??= await local.captureOne({ text: u });
      return first;
    }
    return local.captureOne(input);
  },
  async captureOne({ text = "", html = "", imageBase64 = null, fileName = null, sourceApp = null, note = null }) {
    const url = extractUrls(text, html)[0] ?? null;
    const isCode = !url && !imageBase64 && looksLikeCode(text);
    const canonical = url ? canonicalUrl(url) : null;
    const key = dedupeKey(canonical, text);
    const existing = state.items.find((i) => i.dedupeKey === key && !i.trashed);
    if (existing) {
      commit(patchItem(existing.id, () => ({ lastSeenAt: now() })));
      return existing.id;
    }
    const kind = detectKind(canonical, { hasImage: !!imageBase64, hasFile: !!fileName });
    const id = `i-${state.nextId}`;
    const item = {
      id, kind,
      title: fileName || draftTitle(canonical, text),
      url: canonical || url || null,
      domain: canonical ? new URL(canonical).hostname : sourceApp || "paste",
      dedupeKey: key,
      thumb: imageBase64, pageShot: null, aspect: 4 / 3,
      tags: isCode ? ["code"] : [], autoTags: isCode ? ["code"] : [], folderIds: [], rating: 0, duration: null,
      status: canonical ? "pending" : "ready",
      bodyHtml: html || null,
      bodyText: html ? stripHtml(html) : text,
      summary: (html ? stripHtml(html) : text).slice(0, 600),
      addedAt: now(), lastSeenAt: now(),
      size: `${((text.length + html.length) / 1024).toFixed(1)} KB`,
      dimensions: "—",
      notes: note,
      palette: ["#2c4a6e", "#3f6d99", "#7fa8cc", "#c8dcea", "#8a93a3", "#5a6272"],
      trashed: false,
    };
    commit({ ...state, items: [item, ...state.items], nextId: state.nextId + 1 });
    if (canonical) setTimeout(() => local.update(id, { status: "ready", userEdited: false }), 1800);
    return id;
  },
  async refetch(ids) {
    return local.reenrich(ids);
  },
  async reenrich(ids) {
    ids.forEach((id) => {
      commit(patchItem(id, () => ({ status: "enriching" })));
      setTimeout(() => commit(patchItem(id, () => ({ status: "ready" }))), 1500);
    });
  },
  async createFolder(name, parentId = null, emoji = "📁") {
    name = name.trim();
    if (!name) return null;
    const id = `f-${Date.now().toString(36)}`;
    const folders = parentId
      ? state.folders.map((g) => (g.id === parentId ? { ...g, children: [...g.children, { id, name }] } : g))
      : [...state.folders, { id, name, emoji, children: [] }];
    commit({ ...state, folders });
    return id;
  },
  async renameFolder(id, name) {
    name = name.trim();
    if (!name) return;
    commit({ ...state, folders: state.folders.map((g) => (g.id === id ? { ...g, name } : { ...g, children: g.children.map((c) => (c.id === id ? { ...c, name } : c)) })) });
  },
  async acceptFolder(id) {
    await local.addToFolder([], id);
  },
  async deleteFolder(id) {
    const gone = new Set([id]);
    state.folders.forEach((g) => g.id === id && g.children.forEach((c) => gone.add(c.id)));
    commit({
      ...state,
      folders: state.folders.filter((g) => g.id !== id).map((g) => ({ ...g, children: g.children.filter((c) => c.id !== id) })),
      items: state.items.map((i) => ({ ...i, folderIds: i.folderIds.filter((f) => !gone.has(f)) })),
    });
  },
  async settings(patch) {
    commit({ ...state, settings: { ...state.settings, ...patch } });
    return state.settings;
  },
  async agents() {
    return [{ key: "off", name: "Browser preview — no agent available", installed: true, sendsToCloud: false }];
  },
  async runs() {
    return [];
  },
  async paths() {
    return { data: "(browser preview)", log: "(browser preview)" };
  },
  async mcp() {
    return null;
  },
  async sync() {
    return null;
  },
  async syncStatus() {
    return null;
  },
  async reset() {
    localStorage.removeItem(KEY);
    commit(localLoad());
  },
};

export const actions = tauri ? remote : local;

if (tauri) tauriInit().catch((e) => console.error("membox core init failed", e));
else commit(localLoad());

// ---------------------------------------------------------------------------
// Queries (pure, over the snapshot)
// ---------------------------------------------------------------------------

export function folderById(folders, id) {
  for (const g of folders) {
    if (g.id === id) return g;
    for (const c of g.children) if (c.id === id) return c;
  }
  return null;
}

export function folderPath(folders, id) {
  for (const g of folders) {
    if (g.id === id) return g.name;
    for (const c of g.children) if (c.id === id) return `${g.name} › ${c.name}`;
  }
  return "";
}

function folderScope(folders, id) {
  const g = folders.find((f) => f.id === id);
  return g ? new Set([id, ...g.children.map((c) => c.id)]) : new Set([id]);
}

const SMART = {
  "sf-week": (i) => Date.now() - Date.parse(i.addedAt) < 7 * 864e5,
  "sf-fav": (i) => i.rating === 5,
  // A thread is not a transcript: Hacker News and Reddit items keep their
  // comments in the same field (§3.5), and this folder is about listening to
  // something without watching it.
  "sf-tx": (i) => ["youtube_video", "youtube_music", "youtube_playlist", "vimeo", "instagram", "tiktok"].includes(i.kind) && (!!i.transcript || !!i.duration),
  "sf-pending": (i) => i.status !== "ready" || i.folderIds.length === 0 || !!i.meta?.suggestedFolderId,
};

export const smartFolders = [
  { id: "sf-week", name: "Added this week" },
  { id: "sf-fav", name: "Five stars" },
  { id: "sf-tx", name: "Has transcript" },
  { id: "sf-pending", name: "Needs review" },
];

export function itemsIn(state, viewId, query = "", sort = "added") {
  const live = state.items.filter((i) => !i.trashed);
  let out;
  switch (viewId) {
    case "all": case "tags": out = live; break;
    case "uncategorized": out = live.filter((i) => i.folderIds.length === 0); break;
    case "untagged": out = live.filter((i) => i.tags.length === 0); break;
    case "trash": out = state.items.filter((i) => i.trashed); break;
    default:
      if (SMART[viewId]) out = live.filter(SMART[viewId]);
      else {
        const scope = folderScope(state.folders, viewId);
        out = live.filter((i) => i.folderIds.some((f) => scope.has(f)));
      }
  }

  // ponytail: substring match over the snapshot, incl. readable text and
  // transcript. The core has FTS5 (`search` action) for iOS and the agent;
  // switch the desktop to it when a library outgrows in-memory matching.
  const q = query.trim().toLowerCase();
  if (q) {
    for (const term of q.split(/\s+/)) {
      const [k, v] = term.includes(":") ? term.split(":", 2) : [null, term];
      out = out.filter((i) => {
        if (k === "kind") return i.kind.includes(v);
        if (k === "tag") return i.tags.some((t) => t.includes(v));
        if (k === "domain") return (i.domain || "").includes(v);
        if (k === "rating") return i.rating >= parseInt(v.replace(/\D/g, ""), 10);
        if (k === "is") return v === "untagged" ? i.tags.length === 0 : true;
        if (k === "has") return v === "transcript" ? !!i.transcript : v === "autotags" ? (i.autoTags || []).length > 0 : true;
        return (
          i.title.toLowerCase().includes(v) ||
          i.tags.some((t) => t.includes(v)) ||
          (i.domain || "").includes(v) ||
          (i.summary || "").toLowerCase().includes(v) ||
          (i.bodyText || "").toLowerCase().includes(v) ||
          (i.transcript || "").toLowerCase().includes(v)
        );
      });
    }
  }

  const by = {
    added: (a, b) => (b.addedAt > a.addedAt ? 1 : -1),
    title: (a, b) => a.title.localeCompare(b.title),
    rating: (a, b) => b.rating - a.rating,
    domain: (a, b) => (a.domain || "").localeCompare(b.domain || ""),
  }[sort];
  return by ? [...out].sort(by) : out;
}

export function counts(state) {
  const live = state.items.filter((i) => !i.trashed);
  const perFolder = {};
  live.forEach((i) => i.folderIds.forEach((f) => (perFolder[f] = (perFolder[f] || 0) + 1)));
  const groupTotal = (g) => g.children.reduce((n, c) => n + (perFolder[c.id] || 0), perFolder[g.id] || 0);
  return {
    all: live.length,
    uncategorized: live.filter((i) => i.folderIds.length === 0).length,
    untagged: live.filter((i) => i.tags.length === 0).length,
    tags: new Set(live.flatMap((i) => i.tags)).size,
    trash: state.items.length - live.length,
    smart: Object.fromEntries(Object.entries(SMART).map(([k, f]) => [k, live.filter(f).length])),
    folder: (id) => {
      const g = state.folders.find((f) => f.id === id);
      return g ? groupTotal(g) : perFolder[id] || 0;
    },
  };
}

// useSyncExternalStore compares snapshots by identity, so a selector that
// builds a fresh array every call re-renders forever — which is what clicking
// All Tags did. `commit` swaps the whole state object, so caching on its
// identity is both correct and free.
let tagCache = { of: null, tags: [] };

export function allTags(state) {
  if (tagCache.of !== state) {
    const n = {};
    state.items.filter((i) => !i.trashed).forEach((i) => i.tags.forEach((t) => (n[t] = (n[t] || 0) + 1)));
    tagCache = { of: state, tags: Object.entries(n).sort((a, b) => b[1] - a[1]) };
  }
  return tagCache.tags;
}

/// Where a transcript line mentions the query — for the "why it matched" tile
/// caption (§5.4). Returns null when nothing in the body matches.
export function matchSnippet(item, query) {
  const q = query.trim().toLowerCase().split(/\s+/).find((t) => !t.includes(":"));
  if (!q) return null;
  for (const src of [item.transcript, item.bodyText, item.summary]) {
    const i = (src || "").toLowerCase().indexOf(q);
    if (i >= 0) return "…" + src.slice(Math.max(0, i - 40), i + 60).replace(/\s+/g, " ") + "…";
  }
  return null;
}
