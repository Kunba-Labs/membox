import css from "./Toolbar.module.css";
import * as I from "../icons.jsx";
import { IconButton } from "./Sidebar.jsx";

export default function Toolbar({ title, count, size, onSize, query, onQuery, sort, onSort, searchRef, inTrash, onEmptyTrash, queue = 0, reading = 0, inspector, onToggleInspector, inset = false }) {
  return (
    <header className={`${css.toolbar} ${inset ? css.inset : ""}`}>
      <div className={css.nav}>
        <IconButton label="Back" onClick={() => history.back()}><I.ChevronLeft /></IconButton>
        <IconButton label="Forward" onClick={() => history.forward()}><I.ChevronRight /></IconButton>
      </div>
      <div className={css.title}>
        {title}
        <span className={css.titleCount}>{count}</span>
      </div>

      <div className={css.spacer} />
      <div className={css.slider}>
        <input type="range" min="150" max="420" value={size} aria-label="Tile size" onChange={(e) => onSize(+e.target.value)} />
      </div>
      <div className={css.spacer} />

      {reading > 0 && (
        <span className={css.queue} title="reading a paste"><I.Spinner className={css.spin} />reading</span>
      )}
      {queue > 0 && (
        <span className={css.queue} title={`${queue} enriching`}><I.Bolt />{queue}</span>
      )}
      {inTrash && (
        <button className={css.textBtn} onClick={onEmptyTrash}>Empty Trash</button>
      )}

      <label className={css.sort} title="Sort">
        <I.Sort />
        <select value={sort} onChange={(e) => onSort(e.target.value)} aria-label="Sort by">
          <option value="added">Date added</option>
          <option value="title">Title</option>
          <option value="rating">Rating</option>
          <option value="domain">Source</option>
        </select>
      </label>

      <div className={css.search}>
        <I.Search />
        <input
          ref={searchRef}
          placeholder="Search  ·  kind:video tag:travel rating:4"
          aria-label="Search library"
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && (onQuery(""), e.target.blur())}
        />
        {query && (
          <button className={css.clear} aria-label="Clear search" onClick={() => onQuery("")}><I.Close /></button>
        )}
      </div>
      <IconButton label={inspector ? "Hide inspector (⌘I)" : "Show inspector (⌘I)"} active={inspector} onClick={onToggleInspector}>
        <I.PanelRight />
      </IconButton>
    </header>
  );
}
