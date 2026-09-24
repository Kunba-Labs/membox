import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sidebar from "./components/Sidebar.jsx";
import Toolbar from "./components/Toolbar.jsx";
import Grid from "./components/Grid.jsx";
import Inspector from "./components/Inspector.jsx";
import Detail from "./components/Detail.jsx";
import ContextMenu from "./components/ContextMenu.jsx";
import TagCloud from "./components/TagCloud.jsx";
import Settings from "./components/Settings.jsx";
import Paste from "./components/Paste.jsx";
import * as I from "./icons.jsx";
import { useStore, actions, itemsIn } from "./store.js";
import css from "./App.module.css";

const isEditing = (el) =>
  el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);

export default function App() {
  const state = useStore();
  const [view, setView] = useState({ id: "all", name: "All" });
  const [size, setSize] = useState(200);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("added");
  const [selected, setSelected] = useState(() => new Set());
  const [anchor, setAnchor] = useState(null);
  const [inspector, setInspector] = useState(true);
  const [sidebar, setSidebar] = useState(true);
  const [detailId, setDetailId] = useState(null);
  const [menu, setMenu] = useState(null);
  const [settings, setSettings] = useState(false);
  const [paste, setPaste] = useState(null);
  const [reading, setReading] = useState(0);
  const colsRef = useRef(4);
  const searchRef = useRef(null);

  // The look lives on the root element; tokens.css holds the difference.
  useEffect(() => {
    const t = state.settings?.theme || "bauhaus";
    document.documentElement.dataset.theme = t === "bauhaus" ? "" : t;
  }, [state.settings?.theme]);

  const shown = useMemo(() => itemsIn(state, view.id, query, sort), [state, view.id, query, sort]);

  // Take the last use of a tag off an item and the tag is gone — there is no
  // tag table, only what items carry. The filter it left behind is not: it sat
  // there showing an empty library and looking broken. Clear it when the tag it
  // names stops existing, but only if it existed a moment ago; half-typed
  // `tag:de` matches nothing yet and must be left alone.
  const filteredTag = useRef({ tag: null, existed: false });
  useEffect(() => {
    const tag = (query.match(/(?:^|\s)tag:(\S+)/) || [])[1]?.toLowerCase();
    if (!tag) {
      filteredTag.current = { tag: null, existed: false };
      return;
    }
    const exists = state.items.some((i) => !i.trashed && i.tags.some((t) => t.toLowerCase() === tag));
    if (filteredTag.current.tag !== tag) {
      filteredTag.current = { tag, existed: exists };
      return;
    }
    if (filteredTag.current.existed && !exists) {
      setQuery((q) => q.replace(/(?:^|\s)tag:\S+/, "").trim());
    }
    filteredTag.current.existed = exists;
  }, [state.items, query]);
  const focusId = anchor && shown.some((i) => i.id === anchor) ? anchor : shown[0]?.id ?? null;
  const focusItem = state.items.find((i) => i.id === focusId) ?? null;
  const selectedIds = useMemo(() => [...selected].filter((id) => shown.some((i) => i.id === id)), [selected, shown]);

  const select = useCallback(
    (id, { meta = false, shift = false } = {}) => {
      setSelected((prev) => {
        if (meta) {
          const next = new Set(prev);
          next.has(id) ? next.delete(id) : next.add(id);
          return next;
        }
        if (shift && anchor) {
          const a = shown.findIndex((i) => i.id === anchor);
          const b = shown.findIndex((i) => i.id === id);
          if (a >= 0 && b >= 0) {
            const [lo, hi] = a < b ? [a, b] : [b, a];
            return new Set(shown.slice(lo, hi + 1).map((i) => i.id));
          }
        }
        return new Set([id]);
      });
      if (!shift) setAnchor(id);
    },
    [anchor, shown]
  );

  const changeView = (id, name) => {
    setView({ id, name });
    setSelected(new Set());
    setAnchor(null);
  };

  // §1.1 — capture is an explicit ⌘V into the window or a drop. Nothing else.
  useEffect(() => {
    const onPaste = (e) => {
      if (isEditing(document.activeElement)) return;
      const cd = e.clipboardData;
      if (!cd) return;
      const files = [...cd.files];
      const text = cd.getData("text/plain");
      const html = cd.getData("text/html");
      if (!files.length && !text.trim() && !html.trim()) return;
      e.preventDefault();
      if (files.length) files.forEach((f) => ingest(f.type.startsWith("image/") ? { img: f } : { file: f }));
      else ingest({ text, html });
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);

  // Anything: a link, a list of links, an image, a PDF, a snippet of code.
  // Files travel as base64 (the webview cannot hand the core a path); the
  // core decides what each thing is. Text goes through the paste sheet (§1.4),
  // which reads it first and then asks the one question worth asking.
  const ingest = async ({ text = "", html = "", img = null, file = null }) => {
    changeView("all", "All");
    if (!img && !file) return setPaste({ text, html });
    const asBase64 = (f) => new Promise((res) => { const r = new FileReader(); r.onload = () => res(r.result); r.readAsDataURL(f); });
    const MAX = 50 * 1024 * 1024;
    const input = { text, html, fileName: file?.name ?? img?.name ?? null };
    if (img) input.imageBase64 = await asBase64(img);
    else if (file && file.size <= MAX) input.fileBase64 = await asBase64(file);
    const id = await actions.capture(input);
    setSelected(new Set([id]));
    setAnchor(id);
    // Images and files are already in; the sheet is only there for the note
    // ("inspiration", "receipts") that files and tags them.
    setPaste({ ids: [id], entries: [{ kind: img ? "image" : "file", text: input.fileName || "pasted image" }] });
  };

  // §1.4 — a paste that needs the agent hands the work over and lets go: the
  // sheet closes, the reading happens behind it, the items land when they land.
  const runPaste = useCallback(async (text, html, note) => {
    setReading((n) => n + 1);
    try {
      const plan = await actions.plan(text, html, note);
      if (plan.entries.length) {
        const { ids } = await actions.capturePlan(plan, note);
        if (ids?.length) {
          setSelected(new Set(ids));
          setAnchor(ids[0]);
        }
      }
    } catch (e) {
      console.error("paste", e);
    } finally {
      setReading((n) => n - 1);
    }
  }, []);

  // A sticky starts empty and open (§9) — there is nothing to fetch.
  const newNote = useCallback(async () => {
    const id = await actions.newNote();
    setPaste(null);
    setSelected(new Set([id]));
    setAnchor(id);
    setDetailId(id);
  }, []);

  const closePaste = useCallback((ids) => {
    setPaste(null);
    if (ids?.length) {
      setSelected(new Set(ids));
      setAnchor(ids[0]);
    }
  }, []);

  const onDrop = (e) => {
    if (e.dataTransfer.types.includes("application/x-membox-items")) return; // internal drag
    e.preventDefault();
    const files = [...e.dataTransfer.files];
    const text = e.dataTransfer.getData("text/uri-list") || e.dataTransfer.getData("text/plain");
    const html = e.dataTransfer.getData("text/html");
    if (files.length) files.forEach((f) => ingest(f.type.startsWith("image/") ? { img: f } : { file: f }));
    else if (text || html) ingest({ text, html });
  };

  // §6.8 — one keymap. Arrows walk the grid, digits rate, ⌫ trashes.
  useEffect(() => {
    const onKey = (e) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        return newNote();
      }
      // A sheet's Enter closes it and selects what it made; React flushes that
      // before the same keydown reaches this (re-registered) listener, which
      // would then open the selection. A handled key is not ours.
      if (e.defaultPrevented) return;
      if (isEditing(document.activeElement) || detailId || menu || settings || paste) return;
      const idx = shown.findIndex((i) => i.id === focusId);
      const go = (n) => {
        const t = shown[Math.max(0, Math.min(shown.length - 1, n))];
        if (t) select(t.id, { shift: e.shiftKey });
      };
      const cols = colsRef.current;
      if (e.key === "ArrowRight") return e.preventDefault(), go(idx + 1);
      if (e.key === "ArrowLeft") return e.preventDefault(), go(idx - 1);
      if (e.key === "ArrowDown") return e.preventDefault(), go(idx + cols);
      if (e.key === "ArrowUp") return e.preventDefault(), go(idx - cols);
      if (e.key === "Enter" || e.key === " ") return focusId && (e.preventDefault(), setDetailId(focusId));
      if (e.key === "Escape") return setSelected(new Set());
      if ((e.key === "Backspace" || e.key === "Delete") && selectedIds.length)
        return e.preventDefault(), view.id === "trash" ? actions.deleteForever(selectedIds) : actions.trash(selectedIds);
      if (e.metaKey && e.key === "a") return e.preventDefault(), setSelected(new Set(shown.map((i) => i.id)));
      if (e.metaKey && e.key === "f") return e.preventDefault(), searchRef.current?.focus();
      if (e.metaKey && e.key === "i") return e.preventDefault(), setInspector((v) => !v);
      if (e.metaKey && e.key === "\\") return e.preventDefault(), setSidebar((v) => !v);
      if (/^[0-5]$/.test(e.key) && selectedIds.length)
        return selectedIds.forEach((id) => actions.update(id, { rating: +e.key }));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shown, focusId, selectedIds, select, detailId, menu, settings, paste, view.id, newNote]);

  const itemMenu = (e, id) => {
    e.preventDefault();
    if (!selected.has(id)) select(id);
    const ids = selected.has(id) ? [...selected] : [id];
    const item = state.items.find((i) => i.id === id);
    const inTrash = view.id === "trash";
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Open", run: () => setDetailId(id) },
        item?.url && { label: "Open source", run: () => window.open(item.url, "_blank") },
        item?.url && { label: "Copy source URL", run: () => navigator.clipboard.writeText(item.url) },
        item?.bodyHtml && { label: "Copy original markup", run: () => navigator.clipboard.writeText(item.bodyHtml) },
        "-",
        { label: "Retake screenshot", run: () => actions.refetch(ids) },
        { label: "Re-enrich (agent)", run: () => actions.reenrich(ids) },
        "-",
        inTrash
          ? { label: "Put back", run: () => actions.trash(ids, false) }
          : { label: ids.length > 1 ? `Move ${ids.length} to Trash` : "Move to Trash", run: () => actions.trash(ids), danger: true },
        inTrash && { label: "Delete permanently", run: () => actions.deleteForever(ids), danger: true },
      ].filter(Boolean),
    });
  };

  return (
    <div className={css.app} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      {sidebar ? (
        <Sidebar selected={view.id} onSelect={changeView} onCollapse={() => setSidebar(false)} onSettings={() => setSettings(true)} onAdd={() => setPaste({ compose: true })} />
      ) : (
        <div className={css.rail}>
          <button className="iconBtn" title="Show sidebar (⌘\\)" aria-label="Show sidebar" onClick={() => setSidebar(true)}><I.Panel /></button>
        </div>
      )}
      <main className={css.main}>
        <Toolbar
          inset={!sidebar}
          title={view.name}
          count={shown.length}
          size={size}
          onSize={setSize}
          query={query}
          onQuery={setQuery}
          sort={sort}
          onSort={setSort}
          searchRef={searchRef}
          queue={state.queue?.length || 0}
          reading={reading}
          inspector={inspector}
          onToggleInspector={() => setInspector((v) => !v)}
          inTrash={view.id === "trash"}
          onEmptyTrash={() => confirm("Empty the trash? This cannot be undone.") && actions.emptyTrash()}
        />
        {view.id === "tags" ? (
          <TagCloud onPick={(t) => { setQuery(`tag:${t}`); changeView("all", "All"); }} />
        ) : (
          <Grid
            items={shown}
            size={size}
            selected={selected}
            focusId={focusId}
            onSelect={select}
            onOpen={setDetailId}
            onMenu={itemMenu}
            onCols={(n) => (colsRef.current = n)}
            query={query}
            inTrash={view.id === "trash"}
          />
        )}
      </main>
      {inspector && <Inspector item={focusItem} count={selectedIds.length} />}
      {detailId && <Detail id={detailId} onClose={() => setDetailId(null)} onOpen={setDetailId} />}
      {settings && <Settings onClose={() => setSettings(false)} />}
      {paste && <Paste paste={paste} onClose={closePaste} onRun={runPaste} onNote={newNote} />}
      {menu && <ContextMenu {...menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
