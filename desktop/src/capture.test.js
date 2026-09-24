// node --test src/capture.test.js — the one runnable check for the pure logic
// that silently rots: canonicalisation, dedupe and kind detection (§11.1).
import { test } from "node:test";
import assert from "node:assert/strict";
import { canonicalUrl, detectKind, firstUrl, draftTitle, dedupeKey, extractUrls, looksLikeCode, isGuess } from "./capture.js";

test("youtube variants collapse to one canonical", () => {
  const want = "https://youtube.com/watch?v=abc123";
  for (const u of [
    "https://www.youtube.com/watch?v=abc123&t=42s&si=xyz",
    "https://youtu.be/abc123?si=xyz",
    "https://m.youtube.com/watch?v=abc123",
    "https://www.youtube.com/shorts/abc123",
  ])
    assert.equal(canonicalUrl(u), want, u);
  assert.equal(detectKind(want), "youtube_video");
});

test("music and playlists are their own kinds", () => {
  assert.equal(detectKind(canonicalUrl("https://music.youtube.com/watch?v=q1&list=RD")), "youtube_music");
  assert.equal(detectKind(canonicalUrl("https://www.youtube.com/playlist?list=PL9")), "youtube_playlist");
  assert.equal(canonicalUrl("https://www.youtube.com/watch?v=JuoVZkPBiKk&list=PLoRO&index=7"), "https://youtube.com/playlist?list=PLoRO");
});

test("tracking params and hashes drop, path is preserved", () => {
  assert.equal(
    canonicalUrl("https://WWW.Example.com/a/b/?utm_source=x&fbclid=y&page=2#top"),
    "https://example.com/a/b?page=2"
  );
});

test("dedupe key is stable across noise", () => {
  const a = dedupeKey(canonicalUrl("https://youtu.be/abc123"));
  const b = dedupeKey(canonicalUrl("https://www.youtube.com/watch?v=abc123&utm_medium=share"));
  assert.equal(a, b);
  assert.notEqual(a, dedupeKey(canonicalUrl("https://youtu.be/abc124")));
});

test("kinds", () => {
  assert.equal(detectKind(canonicalUrl("https://www.instagram.com/reel/XyZ/")), "instagram");
  assert.equal(detectKind(canonicalUrl("https://github.com/asg017/sqlite-vec")), "github_repo");
  assert.equal(detectKind(canonicalUrl("https://x.com/u/status/1")), "x_post");
  assert.equal(detectKind(canonicalUrl("https://a.org/paper.PDF")), "pdf");
  assert.equal(detectKind(canonicalUrl("https://nautil.us/light")), "webpage");
  assert.equal(detectKind(null), "snippet");
  assert.equal(detectKind(null, { hasImage: true }), "image");
});

test("non-http and garbage are rejected", () => {
  assert.equal(canonicalUrl("ftp://x.org/f"), null);
  assert.equal(canonicalUrl("not a url"), null);
});

test("first url and draft title", () => {
  assert.equal(firstUrl("look at this https://youtu.be/abc123 wow"), "https://youtu.be/abc123");
  assert.equal(firstUrl("no links"), null);
  assert.equal(draftTitle("https://youtube.com/watch?v=abc123"), "youtube.com · abc123");
  assert.equal(draftTitle("https://example.com/some-long-slug"), "some long slug");
  assert.equal(draftTitle(null, "First line\nsecond"), "First line");
});

test("anything pasted finds its urls", () => {
  assert.deepEqual(extractUrls("look youtube.com/watch?v=abc and www.bbc.co.uk, plus https://x.com/a/status/1."),
    ["https://youtube.com/watch?v=abc", "https://www.bbc.co.uk", "https://x.com/a/status/1"]);
  assert.deepEqual(extractUrls("e.g. version 1.5 costs 3.50"), []);
  assert.deepEqual(extractUrls("Intangible.ai\nCargo.site\nDeath to stock\nShadergradient.co"),
    ["https://Intangible.ai", "https://Cargo.site", "https://Shadergradient.co"]);
  assert.deepEqual(extractUrls("see capture.rs and notes.md, README.txt"), []);
  assert.deepEqual(extractUrls("went to Paris.Then home"), []);
  assert.ok(isGuess("https://cargo.site"));
  assert.ok(!isGuess("https://github.com"));
  assert.ok(!isGuess("https://cargo.site/work"));
  assert.deepEqual(extractUrls("", '<iframe src="https://www.youtube.com/embed/abc123"></iframe>'), ["https://www.youtube.com/embed/abc123"]);
  assert.equal(detectKind(canonicalUrl("https://vimeo.com/1")), "vimeo");
  assert.equal(detectKind(canonicalUrl("https://i.imgur.com/a.JPG")), "image");
  assert.ok(looksLikeCode("fn main() {\n    println!(\"hi\");\n}"));
  assert.ok(!looksLikeCode("Remember: the Karoo light is flat\nand gold after five."));
});
