import { useEffect, useLayoutEffect, useRef } from "react";
import css from "./ContextMenu.module.css";

// items: [{label, run, danger}] with "-" for a separator.
export default function ContextMenu({ x, y, items, onClose }) {
  const ref = useRef(null);

  useEffect(() => {
    const close = (e) => { if (!ref.current?.contains(e.target)) onClose(); };
    const key = (e) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  // Keep it on screen.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (r.right > innerWidth) el.style.left = `${Math.max(4, x - r.width)}px`;
    if (r.bottom > innerHeight) el.style.top = `${Math.max(4, y - r.height)}px`;
  }, [x, y]);

  return (
    <div ref={ref} className={`${css.menu} ${css.surface}`} style={{ left: x, top: y }} role="menu">
      {items.map((it, i) =>
        it === "-" ? (
          <div key={i} className={css.sep} />
        ) : (
          <button key={i} role="menuitem" className={`${css.item} ${it.danger ? css.danger : ""}`}
            onClick={() => { onClose(); it.run(); }}>
            {it.label}
          </button>
        )
      )}
    </div>
  );
}
