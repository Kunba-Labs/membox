import { useState } from "react";
import css from "./Sidebar.module.css";
import * as I from "../icons.jsx";
import { useStore, actions, counts, smartFolders } from "../store.js";
import ContextMenu from "./ContextMenu.jsx";
import mark from "../../../brand/generated/launch-mark-512.png";

const collections = [
  { id: "all", name: "All", icon: I.Inbox, key: "all" },
  { id: "uncategorized", name: "Uncategorized", icon: I.FolderQ, key: "uncategorized" },
  { id: "untagged", name: "Untagged", icon: I.TagQ, key: "untagged" },
  { id: "tags", name: "All Tags", icon: I.Bookmark, key: "tags" },
  { id: "trash", name: "Trash", icon: I.Trash, key: "trash" },
];

const DRAG = "application/x-membox-items";

// The seed tree gets the app's own glyphs; everything else gets a folder.
// Drawn, not typed: an emoji is somebody else's font, at somebody else's
// weight, and a sidebar of them reads as a sticker sheet.
const FOLDER_ICONS = {
  "f-seed-watching": I.Film,
  "f-seed-reading": I.Book,
  "f-seed-listening": I.Headphones,
  "f-seed-travel": I.Globe,
  "f-seed-build": I.Tools,
  "f-seed-notes": I.Sticky,
};
// An emoji the agent chose still says something; map the ones it reaches for.
const EMOJI_ICONS = {
  "🛒": I.Cart, "🛍": I.Cart, "🎬": I.Film, "📚": I.Book, "📖": I.Book, "🎧": I.Headphones,
  "🌍": I.Globe, "🌎": I.Globe, "🗺": I.Globe, "🛠": I.Tools, "🔧": I.Tools, "📝": I.Sticky,
  "🎮": I.Game, "🎥": I.Film, "📺": I.Film, "🏷": I.Tag, "💡": I.Bolt,
};
const folderIcon = (f) => FOLDER_ICONS[f.id] || EMOJI_ICONS[(f.emoji || "").trim()] || I.Folder;

function Row({ icon: Icon, emoji, label, count, active, indent, onClick, caret, dropId, from, onMenu, editing, onRename, proposed, smart }) {
  const [over, setOver] = useState(false);
  const droppable = !!dropId;
  return (
    <button
      className={`${css.row} ${indent ? css.child : ""} ${over ? css.dropOver : ""} ${proposed ? css.proposed : ""} ${smart ? css.smart : ""}`}
      aria-current={active ? "true" : undefined}
      onClick={onClick}
      onContextMenu={onMenu}
      onDragOver={droppable ? (e) => {
        if (!e.dataTransfer.types.includes(DRAG)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = from ? "move" : "copy";
        setOver(true);
      } : undefined}
      onDragLeave={droppable ? () => setOver(false) : undefined}
      onDrop={droppable ? (e) => {
        e.preventDefault(); setOver(false);
        // Dropping while inside a folder moves it there; the folder you left
        // empties, and an empty suggestion removes itself.
        try { actions.addToFolder(JSON.parse(e.dataTransfer.getData(DRAG)), dropId, from); } catch {}
      } : undefined}
    >
      {caret}
      {Icon ? <Icon className={css.rowIcon} /> : null}
      {editing ? (
        <input
          className={css.rename}
          autoFocus
          defaultValue={label}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => { if (e.key === "Enter") onRename(e.target.value); if (e.key === "Escape") onRename(null); }}
          onBlur={(e) => onRename(e.target.value)}
        />
      ) : (
        <span className={css.rowLabel}>{label}</span>
      )}
      {proposed && !editing && <span className={css.suggested}>suggested</span>}
      {count != null && !editing && <span className={css.count}>{count}</span>}
    </button>
  );
}

export default function Sidebar({ selected, onSelect, onCollapse, onSettings, onAdd }) {
  const state = useStore();
  const n = counts(state);
  const update = state.update;
  const [open, setOpen] = useState(() => new Set(state.folders.map((f) => f.id)));
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState(null);
  const [renaming, setRenaming] = useState(null);

  const toggle = (id) =>
    setOpen((prev) => { const next = new Set(prev); next.has(id) ? next.delete(id) : next.add(id); return next; });

  const newFolder = async (parentId = null) => {
    const name = prompt(parentId ? "New subfolder name" : "New folder name");
    if (!name) return;
    const id = await actions.createFolder(name, parentId);
    if (parentId) setOpen((p) => new Set(p).add(parentId));
    if (id) onSelect(id, name);
  };

  const folderMenu = (e, f, isGroup) => {
    e.preventDefault(); e.stopPropagation();
    setMenu({
      x: e.clientX, y: e.clientY,
      items: [
        f.proposed && { label: "Accept suggestion", run: () => actions.acceptFolder(f.id) },
        isGroup && { label: "New subfolder…", run: () => newFolder(f.id) },
        { label: "Rename", run: () => setRenaming(f.id) },
        "-",
        { label: "Delete folder", danger: true, run: () => {
          if (confirm(`Delete “${f.name}”? Items stay in the library.`)) { actions.deleteFolder(f.id); if (selected === f.id) onSelect("all", "All"); }
        } },
      ].filter(Boolean),
    });
  };

  const rename = (id) => (value) => {
    setRenaming(null);
    if (value != null) actions.renameFolder(id, value);
  };

  const q = filter.trim().toLowerCase();
  const matches = (name) => !q || name.toLowerCase().includes(q);
  // Dragging out of the folder on screen is a move; out of All or a smart
  // folder is not (there is nothing to move out of).
  const from = selected?.startsWith("f-") ? selected : null;

  return (
    <nav className={css.sidebar}>
      <div className={css.titlebar}>
        <IconButton label="Add something (paste or type)" onClick={onAdd}><I.Plus /></IconButton>
        {update && <button className={css.update} title={`membox ${update} is ready`} onClick={onSettings}>Update</button>}
        <IconButton label="Settings" onClick={onSettings}><I.Gear /></IconButton>
        <IconButton label="Hide sidebar (⌘\\)" onClick={onCollapse}><I.Panel /></IconButton>
      </div>

      <button className={css.library}>
        <img className={css.libraryMark} src={mark} alt="" />
        <span className={css.libraryName}>membox</span>
        <I.Caret className={css.caret} />
      </button>

      <div className={css.scroll}>
        {collections.map((c) => (
          <Row key={c.id} icon={c.icon} label={c.name}
            count={c.id === "uncategorized" ? (n.uncategorized || undefined) : n[c.key]}
            active={selected === c.id} onClick={() => onSelect(c.id, c.name)} />
        ))}

        <div className={css.section}>Smart Folders</div>
        {smartFolders.map((f) => (
          <Row key={f.id} smart icon={I.Bolt} label={f.name} count={n.smart[f.id] ?? 0}
            active={selected === f.id} onClick={() => onSelect(f.id, f.name)} />
        ))}

        <div className={css.section}>
          Folders
          <button className={css.sectionAdd} aria-label="New folder" onClick={() => newFolder()}><I.Plus /></button>
        </div>
        <div className={css.tree}>
          {state.folders.map((f) => {
            // A suggestion with nothing in it is not a place yet: it lives on
            // the item (Inspector › Suggested, and the Needs review folder)
            // until it is accepted, and only then takes a row here.
            const empty = (x) => x.proposed && !n.folder(x.id);
            const kids = f.children.filter((c) => matches(c.name) && !empty(c));
            if (empty(f) && !kids.length) return null;
            if (!matches(f.name) && !kids.length) return null;
            const isOpen = open.has(f.id) || !!q;
            return (
              <div key={f.id} className={isOpen && kids.length ? css.group : undefined}>
                <Row icon={folderIcon(f)} label={f.name} count={n.folder(f.id)} active={selected === f.id} proposed={f.proposed}
                  dropId={f.id} from={from} onMenu={(e) => folderMenu(e, f, true)}
                  editing={renaming === f.id} onRename={rename(f.id)}
                  caret={<I.Caret className={css.caret} open={isOpen} onClick={(e) => { e.stopPropagation(); toggle(f.id); }} />}
                  onClick={() => onSelect(f.id, f.name)} />
                {isOpen && kids.map((c) => (
                  <Row key={c.id} icon={folderIcon(c)} label={c.name} count={n.folder(c.id)} indent proposed={c.proposed}
                    active={selected === c.id} dropId={c.id} from={from} onMenu={(e) => folderMenu(e, c, false)}
                    editing={renaming === c.id} onRename={rename(c.id)}
                    onClick={() => onSelect(c.id, c.name)} />
                ))}
              </div>
            );
          })}
        </div>
      </div>

      <div className={css.footer}>
        <div className={css.filter}>
          <I.Filter />
          <input placeholder="Filter" aria-label="Filter folders" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
      </div>

      {menu && <ContextMenu {...menu} onClose={() => setMenu(null)} />}
    </nav>
  );
}

export function IconButton({ children, label, active, onClick }) {
  return (
    <button className="iconBtn" title={label} aria-label={label} aria-pressed={active} onClick={onClick}>
      {children}
    </button>
  );
}
