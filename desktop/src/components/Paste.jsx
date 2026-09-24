import { useEffect, useRef, useState } from "react";
import css from "./Paste.module.css";
import * as I from "../icons.jsx";
import { actions, useStore } from "../store.js";
import { extractUrls } from "../capture.js";

// What each thing is, and what membox is about to do with it. The mark matters
// most where it is recognisable — an orange Y says "the thread, too".
const WHAT = {
  hn: [I.HN, "Hacker News", "the page it links to, plus the thread"],
  reddit: [I.Reddit, "Reddit", "the page it links to, plus the thread"],
  book: [I.Book, "Book", "cover and synopsis"],
  movie: [I.Film, "Film", "poster and synopsis"],
  tv: [I.Film, "Series", "poster and synopsis"],
  game: [I.Game, "Game", "box art, blurb, the studio"],
  product: [I.Tag, "Product", "page and screenshot"],
  youtube_video: [I.Play, "Video", "poster, description, transcript"],
  youtube_music: [I.Play, "Music", "cover and description"],
  youtube_playlist: [I.Play, "Playlist", "cover and description"],
  vimeo: [I.Play, "Video", "poster and description"],
  instagram: [I.Play, "Reel", "poster and caption"],
  tiktok: [I.Play, "TikTok", "poster and caption"],
  github_repo: [I.Link, "Repo", "screenshot and readme"],
  pdf: [I.Link, "PDF", "first page and its text"],
  image: [I.Layout, "Image", "kept as it is"],
  file: [I.Layout, "File", "thumbnail and text"],
  snippet: [I.Bookmark, "Text", "kept as a note"],
  url: [I.Link, "Link", "screenshot and readable text"],
};
const what = (kind) => WHAT[kind] || [I.Link, "Link", "screenshot and readable text"];

const LANE = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", opencode: "OpenCode", local: "the local model" };

// Same rule as the core's `needs_agent`: only a bare line needs classifying —
// and a bare line is exactly where the person's own words decide the answer.
const hasBareLine = (text) =>
  text.split("\n").map((l) => l.trim()).filter(Boolean).some((l) => l.length <= 280 && !extractUrls(l).length);

const lineCount = (text) => text.split("\n").map((l) => l.trim()).filter(Boolean).length;

function Sheet({ children, onClose, label }) {
  return (
    <div className={css.backdrop} onMouseDown={onClose}>
      <div className={css.sheet} onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={label}>
        {children}
      </div>
    </div>
  );
}

// §1.4 — a paste stops here first.
//
// Links know what they are, so they are captured at once and the note is an
// afterthought. A list of bare names does not: "Alfonso Mocha" is a perfume
// only because you say so. There the note comes first and travels with the
// question — asking the agent before it has been told wastes the call.
export default function Paste({ paste, onClose, onRun, onNote }) {
  const lane = useStore((s) => s.settings?.agent) || "off";
  const [typed, setTyped] = useState("");
  const [note, setNote] = useState("");
  const [plan, setPlan] = useState(null);
  const [ids, setIds] = useState([]);
  const [error, setError] = useState(null);
  const field = useRef(null);
  const started = useRef(false);

  const source = paste.text ?? typed;
  const files = !!paste.ids;
  const asksFirst = !files && hasBareLine(source);
  const [stage, setStage] = useState(paste.compose ? "compose" : asksFirst ? "ask" : "run");

  useEffect(() => {
    if (stage !== "run" || started.current) return;
    started.current = true;
    (async () => {
      try {
        // Files and images have nothing to parse — they are already captured.
        if (files) {
          setPlan({ entries: paste.entries ?? [], source: "files" });
          setIds(paste.ids);
          return;
        }
        const p = await actions.plan(source, paste.html ?? "", note);
        setPlan(p);
        if (!p.entries.length) return onClose(null);
        const { ids } = await actions.capturePlan(p, note);
        setIds(ids);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [stage, files, source, note, paste, onClose]);

  useEffect(() => { field.current?.focus(); }, [stage]);

  // A note the agent never saw (links took the fast path) still files them.
  const saveNote = async () => {
    const n = note.trim();
    if (n && ids.length) await actions.annotate(ids, n);
    onClose(ids);
  };

  const key = (e, go) => {
    if (e.key === "Escape") { e.preventDefault(); onClose(ids.length ? ids : null); }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); go(); }
  };

  const entries = plan?.entries ?? [];
  const preview = source.trim().slice(0, 900);

  // ---- typed at the + button ----
  if (stage === "compose") {
    return (
      <Sheet onClose={() => onClose(null)} label="Add">
        <div className={css.head}>Paste or type anything — links, a list, a name, a note.</div>
        <textarea
          className={css.area}
          ref={field}
          value={typed}
          placeholder={"https://…\nbooks to read:\nThe Dispossessed"}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Escape") onClose(null); }}
        />
        <div className={css.actions}>
          <span className={css.hint}>{typed.trim() && hasBareLine(typed) ? "you can say what these are next" : ""}</span>
          <button className={css.ghost} onClick={() => onNote?.()}>New note</button>
          <button className={css.ghost} onClick={() => onClose(null)}>Cancel</button>
          <button className={css.primary} disabled={!typed.trim()} onClick={() => setStage(hasBareLine(typed) ? "ask" : "run")}>Next</button>
        </div>
      </Sheet>
    );
  }

  // ---- a list of names: say what they are, then the agent reads them ----
  if (stage === "ask") {
    return (
      <Sheet onClose={() => onClose(null)} label="Paste">
        <div className={css.head}>
          {lineCount(source)} lines, no links — what are these?
          <span className={css.by}>{LANE[lane] ? `${LANE[lane]} will sort them` : "no agent lane — kept as text"}</span>
        </div>
        <pre className={css.preview}>{preview}</pre>
        <input
          ref={field}
          className={css.field}
          value={note}
          placeholder="stuff I want to buy · books to read · films for the weekend"
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => key(e, () => { onRun(source, paste.html ?? "", note); onClose(null); })}
        />
        <div className={css.actions}>
          <span className={css.hint}>this goes to the agent as part of the question</span>
          <button className={css.ghost} onClick={() => onClose(null)}>Cancel</button>
          {/* Hand it over and get out of the way: the reading takes as long as
              it takes, and there is nothing here worth watching it. */}
          <button className={css.primary} onClick={() => { onRun(source, paste.html ?? "", note); onClose(null); }}>Read them</button>
        </div>
      </Sheet>
    );
  }

  // ---- reading and capturing ----
  return (
    <Sheet onClose={() => onClose(ids.length ? ids : null)} label="Paste">
      <div className={css.head}>
        {!plan ? (
          <>
            <I.Spinner className={css.spin} />
            {note ? `Reading “${note}” with ${LANE[lane] ?? "the rules"}…` : "Reading what you pasted…"}
          </>
        ) : (
          <>
            {entries.length} {entries.length === 1 ? "thing" : "things"} · {ids.length ? "fetching in the background" : "capturing…"}
            {plan.source && plan.source !== "rules" && <span className={css.by}>read by {plan.source}</span>}
          </>
        )}
      </div>

      {!plan && !!preview && <pre className={css.preview}>{preview}</pre>}

      {!!entries.length && (
        <ul className={css.list}>
          {entries.slice(0, 12).map((e, i) => {
            const [Icon, name, does] = what(e.kind);
            return (
              <li key={i}>
                <Icon className={css.mark} />
                <span className={css.what}>
                  {e.title || e.text}
                  <small>{e.note || `${name} — ${does}`}</small>
                </span>
              </li>
            );
          })}
          {entries.length > 12 && <li className={css.more}>+{entries.length - 12} more</li>}
        </ul>
      )}

      {error && <div className={css.error}>{error}</div>}

      {/* The note was asked for up front when it mattered; links get it after. */}
      {!asksFirst && (
        <input
          ref={field}
          className={css.field}
          value={note}
          placeholder="Do you want to say anything about these?"
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => key(e, saveNote)}
        />
      )}

      <div className={css.actions}>
        <span className={css.hint}>{asksFirst ? note : "e.g. “things I want to buy”, “books to read”"}</span>
        {!plan
          ? <button className={css.ghost} onClick={() => onClose(null)}>Cancel</button>
          : !asksFirst && <button className={css.ghost} onClick={() => onClose(ids)}>Skip</button>}
        <button className={css.primary} onClick={asksFirst ? () => onClose(ids) : saveNote} disabled={!ids.length}>
          {asksFirst ? "Done" : "Save"}
        </button>
      </div>
    </Sheet>
  );
}
