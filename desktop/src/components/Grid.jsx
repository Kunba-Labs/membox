import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import css from "./Grid.module.css";
import * as I from "../icons.jsx";
import { kindLabel } from "../mock.js";
import { matchSnippet } from "../store.js";
import { paperVars } from "./Note.jsx";

const DRAG = "application/x-membox-items";

// Deterministic pseudo-waveform so music tiles don't reshuffle on every render.
const bars = (seed, n = 44) =>
  Array.from({ length: n }, (_, i) => {
    const v = Math.sin(seed * 3.7 + i * 0.8) * Math.cos(i * 0.31);
    return 18 + Math.abs(v) * 78;
  });

// A link whose screenshot hasn't landed yet. The tile takes the shape it will
// have once it does (§4.5's 4:5 frame), so the column doesn't jump.
const isPlaceholder = (item) => !["snippet", "note"].includes(item.kind) && !item.thumb && !item.pageShot;

// Still going, or finished? A spinner on something that gave up an hour ago is
// a lie the tile keeps telling.
const working = (item) => ["pending", "fetching", "enriching", "queued"].includes(item.status);

function Media({ item }) {
  if (isPlaceholder(item)) {
    return (
      <div className={`${css.media} ${css.placeholder}`} data-kind={item.kind}>
        <div className={`${css.waiting} ${!working(item) && item.error ? css.stopped : ""}`}>
          {working(item) ? (
            <><I.Spinner />{item.status}</>
          ) : item.error ? (
            <><I.Warn />{item.error}</>
          ) : (
            "no preview"
          )}
        </div>
        <span className={css.badge}>{kindLabel[item.kind] || item.kind.toUpperCase()}</span>
        {item.domain && <span className={css.domain}>{item.domain}</span>}
      </div>
    );
  }

  // A sticky looks like one: paper, the title, and what it points at (§9).
  if (item.kind === "note") {
    const links = item.meta?.links?.length ?? 0;
    return (
      <div className={`${css.media} ${css.sticky}`} style={paperVars(item)}>
        <div className={css.stickyBody} dangerouslySetInnerHTML={{ __html: item.bodyHtml || "" }} />
        {!item.bodyHtml && <div className={css.stickyEmpty}>empty note</div>}
        {links > 0 && <span className={css.stickyLinks}>{links} linked</span>}
      </div>
    );
  }

  if (item.kind === "youtube_music") {
    return (
      <div className={css.media} style={{ aspectRatio: "3 / 2" }}>
        <div className={css.wave}>
          {bars(parseInt(item.id.slice(2), 10) || 1).map((v, i) => <i key={i} style={{ height: `${v}%` }} />)}
        </div>
        <span className={css.badge}>MUSIC</span>
        {item.duration && <span className={css.duration}>{item.duration}</span>}
      </div>
    );
  }

  // A playlist is its videos: four posters in a 2×2, and how many there are.
  if (item.kind === "youtube_playlist" && item.meta?.playlist?.length) {
    const four = item.meta.playlist.slice(0, 4);
    return (
      <div className={`${css.media} ${css.playlist}`} style={{ aspectRatio: "16 / 9" }} data-n={four.length}>
        {four.map((e) => <img key={e.id} src={e.thumb} alt="" loading="lazy" decoding="async" draggable={false} />)}
        <span className={css.badge}>PLAYLIST</span>
        <span className={css.duration}><I.Play /> {item.meta.count ?? item.meta.playlist.length} videos</span>
      </div>
    );
  }

  if (item.kind === "snippet") {
    // Text: the readable body, small.
    return (
      <div className={css.media}>
        <div className={`${css.snippet} ${item.tags?.includes("code") ? css.code : ""}`}>{item.summary || item.url || item.title}</div>
        <span className={css.badge}>{item.tags?.includes("code") ? "CODE" : kindLabel[item.kind] || item.kind.toUpperCase()}</span>
      </div>
    );
  }

  // Webpages get one fixed frame so the hover scroll is a constant offset:
  // page shot is 1:3, frame is 4:5, travel is exactly -140% — §4.5.
  if (["webpage", "hn", "reddit"].includes(item.kind) && item.pageShot) {
    // At rest the tile is the viewport shot (what the page looks like); on
    // hover the full-page shot fades in and pans — §4.5. Scroll-animated
    // sites whose tall capture is mostly background still get a good tile.
    // The pan shot is a whole page tall — decoding one per visible tile is most
    // of what a wall of webpages costs. It arrives when the pointer does.
    return (
      <div
        className={`${css.media} ${css.scrollShot}`}
        onMouseEnter={(e) => {
          const img = e.currentTarget.querySelector("img[data-src]");
          if (img) { img.src = img.dataset.src; delete img.dataset.src; }
        }}
      >
        {item.thumb && <img className={css.base} src={item.thumb} alt="" loading="lazy" decoding="async" draggable={false} />}
        <img
          className={css.pan} data-src={item.pageShot} alt="" decoding="async" draggable={false}
          onLoad={(e) => { if (e.target.naturalHeight / e.target.naturalWidth < 1.3) e.target.classList.add(css.short); }}
        />
        <span className={css.badge}>{kindLabel[item.kind] || "WEB"}</span>
        <span className={css.domain}>{item.domain}</span>
      </div>
    );
  }

  // A thumbnail is 200px of a 2560px picture. Hovering one looks closer at the
  // part the pointer is over — the origin follows the mouse, so it reads as
  // panning across the image rather than a zoom onto its middle.
  return (
    <div
      className={`${css.media} ${css.zoom}`}
      style={{ aspectRatio: item.aspect }}
      onMouseMove={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        e.currentTarget.style.setProperty("--zx", `${((e.clientX - r.left) / r.width) * 100}%`);
        e.currentTarget.style.setProperty("--zy", `${((e.clientY - r.top) / r.height) * 100}%`);
      }}
    >
      <img src={item.thumb} alt="" loading="lazy" decoding="async" draggable={false} />
      <span className={css.badge}>{kindLabel[item.kind] || item.kind.toUpperCase()}</span>
      {/* A one-screen page has no pan shot, so it lands here — it still says where it came from. */}
      {item.domain && item.domain !== "note" && <span className={css.domain}>{item.domain}</span>}
      {item.duration && <span className={css.duration}>{item.duration}</span>}
      {item.kind === "youtube_video" && <span className={css.play}><I.Play /></span>}
    </div>
  );
}

function useColumnCount(ref, size, onCols) {
  const [n, setN] = useState(4);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const w = el.clientWidth - 24;
      const cols = Math.max(1, Math.floor((w + 10) / (size + 10)));
      setN(cols);
      onCols?.(cols);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref, size, onCols]);
  return n;
}

export default function Grid({ items, size, selected, focusId, onSelect, onOpen, onMenu, onCols, inTrash, query = "" }) {
  const scrollRef = useRef(null);
  const cols = useColumnCount(scrollRef, size, onCols);

  // Round-robin so tile order is library order — §6.3.
  const columns = useMemo(() => {
    const out = Array.from({ length: cols }, () => []);
    items.forEach((item, i) => out[i % cols].push(item));
    return out;
  }, [items, cols]);

  // Keep the keyboard-focused tile in view.
  useEffect(() => {
    if (!focusId) return;
    scrollRef.current?.querySelector(`[data-id="${focusId}"]`)?.scrollIntoView({ block: "nearest" });
  }, [focusId]);

  // Dragging one of a selection takes the whole selection; dragging anything
  // else takes just it (and selects it, so what moves is what you can see).
  const startDrag = (e, id) => {
    const ids = selected.has(id) ? [...selected] : [id];
    if (!selected.has(id)) onSelect(id, {});
    e.dataTransfer.setData(DRAG, JSON.stringify(ids));
    e.dataTransfer.effectAllowed = "copyMove";
    if (ids.length > 1) {
      // A stack with a count, instead of one tile standing in for twelve.
      const badge = document.createElement("div");
      badge.textContent = `${ids.length} items`;
      badge.style.cssText =
        "position:fixed;top:-999px;left:-999px;padding:7px 12px;border-radius:8px;font:600 12px -apple-system,system-ui,sans-serif;" +
        "color:#f2f1ee;background:#2d51e0";
      document.body.appendChild(badge);
      e.dataTransfer.setDragImage(badge, 12, 12);
      setTimeout(() => badge.remove(), 0);
    }
  };

  return (
    <div className={css.scroll} ref={scrollRef}>
      {!items.length ? (
        <div className={css.empty}>
          <div className={css.emptyMark}>⌘V</div>
          {inTrash ? "Trash is empty." : "Nothing here yet — paste a link, an image or some text."}
        </div>
      ) : (
        <div className={css.grid} style={{ "--tile-h": `${Math.round(size * 1.25) + 30}px` }} role="listbox" aria-multiselectable="true" aria-label="Library">
          {columns.map((column, ci) => (
            <div className={css.column} key={ci}>
              {column.map((item) => (
                <div
                  key={item.id}
                  data-id={item.id}
                  className={`${css.tile} ${item.id === focusId ? css.focus : ""}`}
                  role="option"
                  tabIndex={-1}
                  draggable
                  aria-selected={selected.has(item.id)}
                  aria-label={`${item.title}, ${kindLabel[item.kind] || item.kind}`}
                  onClick={(e) => onSelect(item.id, { meta: e.metaKey || e.ctrlKey, shift: e.shiftKey })}
                  onDoubleClick={() => onOpen(item.id)}
                  onContextMenu={(e) => onMenu(e, item.id)}
                  onDragStart={(e) => startDrag(e, item.id)}
                >
                  <Media item={item} />
                  {query && matchSnippet(item, query) && <div className={css.why}>{matchSnippet(item, query)}</div>}
                  <div className={css.caption}>
                    <span className={css.captionText}>{item.title}</span>
                    {item.rating > 0 && <span className={css.ratingDot} title={`${item.rating} stars`}>★{item.rating}</span>}
                    {!isPlaceholder(item) && working(item) && (
                      <span className={css.pending}><I.Spinner />{item.status}</span>
                    )}
                    {!isPlaceholder(item) && !working(item) && item.error && (
                      <span className={css.stoppedChip} title={item.error}><I.Warn />failed</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
