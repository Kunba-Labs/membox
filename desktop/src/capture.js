/* Pure capture logic — spec §1.5 type detection and §1.7 canonicalisation.
   No DOM, no React, so it runs under `node --test` (see capture.test.js) and
   ports to the Rust core line for line. */

const TRACKING = /^(utm_|fbclid$|gclid$|si$|igshid$|ref$|mc_cid$|mc_eid$)/;

export function canonicalUrl(raw) {
  let u;
  try {
    u = new URL(raw.trim());
  } catch {
    return null;
  }
  if (!/^https?:$/.test(u.protocol)) return null;

  const host = u.hostname.replace(/^www\./, "").replace(/^m\./, "");

  // YouTube collapses to the video id — youtu.be, /watch, /shorts, /embed.
  if (host === "youtu.be") return `https://youtube.com/watch?v=${u.pathname.slice(1)}`;
  if (host === "youtube.com" || host === "music.youtube.com") {
    const list = u.searchParams.get("list");
    // A video opened from inside a playlist is a save of the playlist; RD… is a radio mix, not a list.
    if (list && !list.startsWith("RD")) return `https://${host}/playlist?list=${list}`;
    const m = u.pathname.match(/^\/(?:shorts|embed|live)\/([^/?]+)/);
    const v = m ? m[1] : u.searchParams.get("v");
    if (v) return `https://${host}/watch?v=${v}`;
  }

  for (const k of [...u.searchParams.keys()]) if (TRACKING.test(k)) u.searchParams.delete(k);
  u.hash = "";
  const q = u.searchParams.toString();
  const path = u.pathname.replace(/\/+$/, "") || "";
  return `https://${host}${path}${q ? "?" + q : ""}`;
}

export function detectKind(canonical, { hasImage = false, hasFile = false } = {}) {
  if (hasImage) return "image";
  if (hasFile) return "file";
  if (!canonical) return "snippet";
  const u = new URL(canonical);
  const h = u.hostname;
  if (h === "music.youtube.com") return "youtube_music";
  if (h === "youtube.com") return u.pathname === "/playlist" ? "youtube_playlist" : "youtube_video";
  if (h === "instagram.com") return "instagram";
  if (h === "tiktok.com") return "tiktok";
  if (h === "x.com" || h === "twitter.com") return "x_post";
  if (h === "vimeo.com") return "vimeo";
  if (h === "open.spotify.com" || h === "spotify.com" || h === "soundcloud.com") return "music";
  if (h === "reddit.com") return "reddit";
  if (h === "github.com" && u.pathname.split("/").length >= 3) return "github_repo";
  if (/\.pdf$/i.test(u.pathname)) return "pdf";
  if (/\.(png|jpe?g|gif|webp|avif|svg)$/i.test(u.pathname)) return "image";
  return "webpage";
}

const KNOWN_HOSTS = ["youtube.com", "youtu.be", "github.com", "instagram.com", "tiktok.com", "x.com", "twitter.com", "vimeo.com", "reddit.com", "spotify.com", "soundcloud.com", "wikipedia.org", "medium.com", "substack.com"];

// Extensions a lone `name.ext` is far likelier to be than a host. (.sh/.app/.dev/.ai stay out: real sites live there.)
const NOT_TLDS = ["js","ts","jsx","tsx","py","rs","go","rb","php","java","swift","cpp","env","md","txt","json","yml","yaml","toml","lock","log","css","html","xml","sql","csv","png","jpg","jpeg","gif","svg","webp","pdf","zip","mp3","mp4","mov","exe"];

function looksLikeBareUrl(tok) {
  const host = tok.split("/")[0].split("?")[0];
  const labels = host.split(".");
  if (labels.length < 2 || host.includes("@")) return false;
  const tld = labels[labels.length - 1];
  const okTld = tld.length >= 2 && tld.length <= 12 && /^[a-z]+$/i.test(tld);
  const okLabels = labels.every((l) => l && /^[a-z0-9-]+$/i.test(l));
  const enough = tok.includes("/") || labels[0] === "www" || labels.length >= 3 || KNOWN_HOSTS.includes(host.toLowerCase());
  // `Cargo.site` — a bare two-label token with a lowercase, non-file-extension
  // TLD is worth *trying*; isGuess() marks it so a page that won't load can go
  // back to being text.
  const loose = labels.length === 2 && tld === tld.toLowerCase() && !NOT_TLDS.includes(tld);
  return okTld && okLabels && (enough || loose);
}

export function isGuess(url) {
  let u;
  try { u = new URL(url); } catch { return false; }
  const h = u.hostname;
  return u.pathname.replace(/\//g, "") === "" && h.split(".").length === 2 && !h.startsWith("www.") && !KNOWN_HOSTS.includes(h.toLowerCase());
}

// Every URL a paste carries: http(s) in the text, bare `youtube.com/watch?v=…`,
// and — when the text has none — href/src attributes in the HTML flavour.
export function extractUrls(text = "", html = "") {
  const out = [];
  const push = (u) => {
    u = u.replace(/[.,;:!?)\]'"]+$/, "");
    if (!out.includes(u) && out.length < 20) out.push(u);
  };
  for (let tok of text.split(/[\s<>"'()\[\]{}`]+/).filter(Boolean)) {
    tok = tok.replace(/[.,;:!?]+$/, "");
    if (/^https?:\/\//i.test(tok)) push(tok);
    else if (looksLikeBareUrl(tok)) push("https://" + tok);
  }
  if (!out.length && html) {
    for (const m of html.matchAll(/(?:href|src)="(https?:\/\/[^"]+)"/g)) push(m[1]);
  }
  return out;
}

export function firstUrl(text) {
  return extractUrls(text)[0] ?? null;
}

export function looksLikeCode(text) {
  const lines = (text || "").split("\n").filter((l) => l.trim());
  if (lines.length < 2) return false;
  const codey = lines.filter((l) => /[;{}]$/.test(l.trimEnd()) || /^(    |\t)/.test(l) || /=>|\bfn |\bdef |\bimport |\bconst |\blet |^#include|^SELECT |^\$ /.test(l)).length;
  return codey * 2 >= lines.length;
}

// A working title before the agent has produced a real one.
export function draftTitle(canonical, text) {
  if (canonical) {
    const u = new URL(canonical);
    const v = u.searchParams.get("v");
    if (v) return `${u.hostname} · ${v}`;
    const last = u.pathname.split("/").filter(Boolean).pop();
    return last ? decodeURIComponent(last).replace(/[-_]+/g, " ") : u.hostname;
  }
  const line = (text || "").trim().split("\n")[0];
  return line.length > 72 ? line.slice(0, 69) + "…" : line || "Untitled";
}

// Cheap, stable, sync — blake3 in the Rust core, this is enough for the JS side.
export function dedupeKey(canonical, text) {
  const s = canonical || (text || "").trim();
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, "0");
}
