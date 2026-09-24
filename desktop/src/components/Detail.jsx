import { useEffect } from "react";
import css from "./Detail.module.css";
import * as I from "../icons.jsx";
import { kindLabel } from "../mock.js";
import { useStore, folderPath } from "../store.js";
import Note from "./Note.jsx";

// §6.6 — the full-screen item view.
export default function Detail({ id, onClose, onOpen }) {
  const item = useStore((s) => s.items.find((i) => i.id === id));
  const folders = useStore((s) => s.folders);

  useEffect(() => {
    const key = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  if (!item) return null;

  // A sticky opens as itself: the editor is the view (§9).
  if (item.kind === "note") {
    return (
      <div className={css.backdrop} onClick={onClose}>
        <div className={`${css.sheet} ${css.noteSheet}`} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={item.title}>
          <button className={css.close} aria-label="Close" onClick={onClose}><I.Close /></button>
          <Note id={item.id} onOpenItem={onOpen} />
        </div>
      </div>
    );
  }

  return (
    <div className={css.backdrop} onClick={onClose}>
      <div className={`${css.sheet} ${css.surface}`} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={item.title}>
        <button className={css.close} aria-label="Close" onClick={onClose}><I.Close /></button>

        <div className={css.preview}>
          {item.thumb || item.pageShot ? (
            <img src={item.pageShot || item.thumb} alt="" />
          ) : item.bodyHtml ? (
            // The pasted markup, kept verbatim (§1.2), rendered inert.
            <iframe title="Original markup" sandbox="" srcDoc={frame(item.bodyHtml)} />
          ) : (
            <pre className={css.text}>{item.summary}</pre>
          )}
        </div>

        <div className={css.side}>
          <div className={css.kind}>{kindLabel[item.kind] || item.kind}</div>
          <h2 className={css.title}>{item.title}</h2>
          {item.url && (
            <a className={css.url} href={item.url} target="_blank" rel="noreferrer">
              <I.Link /> {item.url}
            </a>
          )}

          {item.summary && <p className={css.summary}>{item.summary}</p>}

          <dl className={css.meta}>
            {item.folderIds.length > 0 && <><dt>Folders</dt><dd>{item.folderIds.map((f) => folderPath(folders, f)).join(", ")}</dd></>}
            {item.tags.length > 0 && <><dt>Tags</dt><dd>{item.tags.join(", ")}</dd></>}
            {item.duration && <><dt>Duration</dt><dd>{item.duration}</dd></>}
            <dt>Source</dt><dd>{item.domain}</dd>
            <dt>Status</dt><dd>{item.status}</dd>
          </dl>

          <div className={css.agentHead}><I.Bolt /> Why it was filed here</div>
          <p className={css.agent}>
            {item.agentReason
              ? item.agentReason + (item.confidence != null ? ` (confidence ${Math.round(item.confidence * 100)}%)` : "")
              : item.folderIds.length
              ? `Filed under ${folderPath(folders, item.folderIds[0])}.`
              : "Not yet filed. Run Re-enrich from the inspector, or drop it on a folder."}
          </p>
          {item.notes && <><div className={css.agentHead}>Notes</div><p className={css.agent}>{item.notes}</p></>}
          {item.meta?.playlist?.length ? (
            <>
              <div className={css.agentHead}>{item.meta.count ?? item.meta.playlist.length} videos</div>
              <ol className={css.playlist}>
                {item.meta.playlist.map((e) => (
                  <li key={e.id}>
                    <a href={`https://youtube.com/watch?v=${e.id}&list=${new URL(item.url).searchParams.get("list")}`} target="_blank" rel="noreferrer">
                      <img src={e.thumb} alt="" loading="lazy" decoding="async" />
                      <span>{e.title}</span>
                      {e.duration && <small>{e.duration}</small>}
                    </a>
                  </li>
                ))}
              </ol>
            </>
          ) : (
            item.transcript && <><div className={css.agentHead}>Transcript</div><pre className={css.transcript}>{item.transcript}</pre></>
          )}
        </div>
      </div>
    </div>
  );
}

const frame = (html) =>
  `<!doctype html><meta charset=utf-8><style>body{margin:16px;font:14px/1.5 -apple-system,system-ui;color:#f2f1ee;background:#0d0d10}a{color:#2d51e0}img{max-width:100%}</style>${html}`;
