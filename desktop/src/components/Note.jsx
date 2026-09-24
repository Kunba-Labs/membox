import { useEffect, useMemo, useRef, useState } from "react";
import css from "./Note.module.css";
import * as I from "../icons.jsx";
import { useStore, actions } from "../store.js";
import { kindLabel } from "../mock.js";

// The paper a sticky is written on. Light stock with dark ink even though the
// app is dark: that is what makes it read as a note stuck to the wall rather
// than another panel of the app.
// The pad belongs to the look: unmixed stock under Bauhaus, pastel under
// Glass. Same four slots either way, so a note keeps its colour across a
// switch — and the old names still resolve to the slot they became.
const PADS = {
  bauhaus: {
    yellow: ["#f0b323", "#17171c", "rgba(23, 23, 28, 0.32)"],
    red: ["#d93b2b", "#f2f1ee", "rgba(242, 241, 238, 0.34)"],
    paper: ["#e8e6e1", "#17171c", "rgba(23, 23, 28, 0.28)"],
    blue: ["#2d51e0", "#f2f1ee", "rgba(242, 241, 238, 0.34)"],
  },
  glass: {
    yellow: ["#fbeaa0", "#3d3617", "rgba(120, 96, 20, 0.22)"],
    red: ["#fbc9d8", "#43212c", "rgba(140, 60, 85, 0.22)"],
    paper: ["#c4ecd3", "#1f3a2a", "rgba(40, 110, 75, 0.22)"],
    blue: ["#cbe4fb", "#1d3245", "rgba(40, 90, 140, 0.22)"],
  },
};
const LEGACY = { pink: "red", mint: "paper", sky: "blue" };
export const pads = () => PADS[document.documentElement.dataset.theme || "bauhaus"] || PADS.bauhaus;
export const PAPERS = PADS.bauhaus; // the slot names; colours come from pads()
export const paper = (item) => {
  const p = pads();
  const c = item?.meta?.color;
  return p[c] || p[LEGACY[c]] || p.yellow;
};
export const paperVars = (item) => {
  const [bg, ink, line] = paper(item);
  return { "--paper": bg, "--ink": ink, "--paper-line": line };
};

// §9 — a sticky. A title, a body you can format, and links to the things in
// the library it is about. Not a todo list: no checkboxes, no due dates, no
// done. It is the note you would have written on the back of an envelope.
//
// ponytail: contenteditable and execCommand, which every WebKit has and which
// ⌘B / ⌘I already drive. A rich-text framework here would be 200kB to earn
// bold. If tables or images are ever wanted, that is when to vendor one.
export default function Note({ id, onOpenItem }) {
  const item = useStore((s) => s.items.find((i) => i.id === id));
  const items = useStore((s) => s.items);
  const body = useRef(null);
  const timer = useRef(null);
  const [picking, setPicking] = useState(false);
  const [q, setQ] = useState("");

  // The DOM owns the text while you type; React only seeds it.
  useEffect(() => {
    if (body.current && body.current.innerHTML !== (item?.bodyHtml ?? "")) {
      body.current.innerHTML = item?.bodyHtml ?? "";
    }
  }, [id]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = (patch) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => actions.update(id, patch), 500);
  };
  const saveBody = () => save({ bodyHtml: body.current?.innerHTML ?? "" });

  useEffect(() => () => {
    // Leaving mid-sentence still keeps the sentence.
    clearTimeout(timer.current);
    if (body.current) actions.update(id, { bodyHtml: body.current.innerHTML });
  }, [id]);

  const cmd = (name, arg) => {
    body.current?.focus();
    document.execCommand(name, false, arg);
    saveBody();
  };

  const hits = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return items
      .filter((i) => i.id !== id && !i.trashed && i.kind !== "note")
      .filter((i) => !needle || i.title.toLowerCase().includes(needle) || (i.domain || "").includes(needle))
      .slice(0, 8);
  }, [items, q, id]);

  const link = (it) => {
    body.current?.focus();
    // A chip the note carries with it: the id lives in the markup, so the links
    // survive export, sync and a copy-paste into another note.
    document.execCommand(
      "insertHTML",
      false,
      `<a data-item="${it.id}" href="#${it.id}" class="${css.chip}" contenteditable="false">${escape(it.title)}</a>&nbsp;`
    );
    setPicking(false);
    setQ("");
    saveBody();
  };

  if (!item) return null;

  return (
    <div className={css.note} style={paperVars(item)}>
      <input
        className={css.title}
        defaultValue={item.title}
        placeholder="Untitled"
        aria-label="Note title"
        onChange={(e) => save({ title: e.target.value })}
      />

      <div className={css.bar}>
        <button title="Bold (⌘B)" onClick={() => cmd("bold")}><b>B</b></button>
        <button title="Italic (⌘I)" onClick={() => cmd("italic")}><i>I</i></button>
        <button title="Heading" onClick={() => cmd("formatBlock", "<h3>")}>H</button>
        <button title="Bullets" onClick={() => cmd("insertUnorderedList")}>•</button>
        <button title="Quote" onClick={() => cmd("formatBlock", "<blockquote>")}>❝</button>
        <span className={css.spacer} />
        <span className={css.colors}>
          {Object.keys(PAPERS).map((c) => (
            <button
              key={c}
              className={`${css.swatch} ${(item.meta?.color || "yellow") === c ? css.swatchOn : ""}`}
              style={{ background: pads()[c][0] }}
              title={c}
              aria-label={`${c} paper`}
              onClick={() => actions.update(id, { color: c })}
            />
          ))}
        </span>
        <button className={css.linkBtn} onClick={() => setPicking((p) => !p)}><I.Link /> Link something</button>
      </div>

      {picking && (
        <div className={css.picker}>
          <input autoFocus placeholder="Search the library…" value={q} onChange={(e) => setQ(e.target.value)} />
          <ul>
            {hits.map((it) => (
              <li key={it.id}>
                <button onClick={() => link(it)}>
                  {it.thumb ? <img src={it.thumb} alt="" /> : <span className={css.noShot} />}
                  <span className={css.hitTitle}>{it.title}</span>
                  <span className={css.hitKind}>{kindLabel[it.kind] || it.kind}</span>
                </button>
              </li>
            ))}
            {!hits.length && <li className={css.none}>nothing matches</li>}
          </ul>
        </div>
      )}

      <div
        ref={body}
        className={css.body}
        contentEditable
        suppressContentEditableWarning
        aria-label="Note body"
        data-placeholder="Write it down…"
        onInput={saveBody}
        onBlur={saveBody}
        onClick={(e) => {
          const a = e.target.closest?.("[data-item]");
          if (a) {
            e.preventDefault();
            onOpenItem?.(a.getAttribute("data-item"));
          }
        }}
      />
    </div>
  );
}

const escape = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
