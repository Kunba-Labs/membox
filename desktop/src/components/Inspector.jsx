import { useState } from "react";
import css from "./Inspector.module.css";
import * as I from "../icons.jsx";
import { IconButton } from "./Sidebar.jsx";
import { kindLabel } from "../mock.js";
import { PAPERS, pads, paperVars } from "./Note.jsx";
import { useStore, actions, folderPath } from "../store.js";

// Commits on blur / Enter, so every keystroke doesn't hit the store.
function Field({ value, onCommit, multiline, ...rest }) {
  const [draft, setDraft] = useState(value);
  const [key, setKey] = useState(value);
  if (key !== value) { setKey(value); setDraft(value); }
  const commit = () => draft !== value && onCommit(draft);
  const Tag = multiline ? "textarea" : "input";
  return (
    <Tag
      {...rest}
      value={draft ?? ""}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !multiline) e.target.blur();
        if (e.key === "Escape") { setDraft(value); e.target.blur(); }
      }}
    />
  );
}

function Chips({ values, onRemove, onAdd, onKeep, folder, options }) {
  const [adding, setAdding] = useState(false);
  return (
    <div className={css.chips}>
      {values.map(({ id, label, auto }) => (
        <span key={id} className={`${css.chip} ${folder ? css.folderChip : ""} ${auto ? css.autoChip : ""}`}
          title={auto ? "Added by membox — click ✓ to keep it as yours" : undefined}>
          {auto && <I.Bolt className={css.autoMark} />}
          {label}
          {auto && onKeep && <button aria-label={`Keep ${label}`} onClick={() => onKeep(id)}><I.Check /></button>}
          <button aria-label={`Remove ${label}`} onClick={() => onRemove(id)}><I.Close /></button>
        </span>
      ))}
      {adding ? (
        options ? (
          <select
            autoFocus
            className={css.chipSelect}
            defaultValue=""
            onBlur={() => setAdding(false)}
            onChange={(e) => { if (e.target.value) onAdd(e.target.value); setAdding(false); }}
          >
            <option value="" disabled>Choose folder…</option>
            {options.map((o) => <option key={o.id} value={o.id} disabled={values.some((v) => v.id === o.id)}>{o.label}</option>)}
          </select>
        ) : (
          <input
            autoFocus
            className={css.chipInput}
            placeholder="tag"
            onBlur={(e) => { if (e.target.value.trim()) onAdd(e.target.value); setAdding(false); }}
            onKeyDown={(e) => {
              if (e.key === "Enter") { onAdd(e.target.value); e.target.value = ""; }
              if (e.key === "Escape") setAdding(false);
            }}
          />
        )
      ) : (
        <button className={css.chipAdd} aria-label="Add" onClick={() => setAdding(true)}><I.Plus /></button>
      )}
    </div>
  );
}

export default function Inspector({ item, count }) {
  const folders = useStore((s) => s.folders);

  if (!item)
    return (
      <aside className={css.inspector}>
        <div className={css.empty}>Select an item to see its details.</div>
      </aside>
    );

  const allFolders = folders.flatMap((g) => [
    { id: g.id, label: g.name },
    ...g.children.map((c) => ({ id: c.id, label: `${g.name} › ${c.name}` })),
  ]);

  return (
    <aside className={css.inspector}>
      <div className={css.head}>
        {count > 1 && <span className={css.multi}>{count} selected</span>}
      </div>

      <div className={css.scroll}>
        <div className={css.preview} style={item.kind === "note" ? paperVars(item) : undefined}>
          {item.kind === "note" ? (
            <div className={css.paperPeek} dangerouslySetInnerHTML={{ __html: item.bodyHtml || "" }} />
          ) : item.thumb || item.pageShot ? (
            <img src={item.thumb || item.pageShot} alt="" />
          ) : (
            <div className={css.previewText}>{item.summary || item.url}</div>
          )}
          <span className={css.badge}>{kindLabel[item.kind] || item.kind.toUpperCase()}</span>
        </div>

        {/* On a sticky these are the pad it can be torn from, not the colours
            somebody's screenshot happened to contain. */}
        {item.kind === "note" ? (
          <div className={css.palette}>
            {Object.keys(PAPERS).map((c) => (
              <button
                key={c}
                className={(item.meta?.color || "yellow") === c ? css.paperOn : undefined}
                style={{ background: pads()[c][0] }}
                title={c}
                aria-label={`${c} paper`}
                onClick={() => actions.update(item.id, { color: c })}
              />
            ))}
          </div>
        ) : (
          <div className={css.palette}>
            {item.palette.map((c, i) => <i key={i} style={{ background: c }} />)}
          </div>
        )}

        <Field key={item.id + "t"} value={item.title} aria-label="Title" onCommit={(v) => actions.update(item.id, { title: v })} />
        <Field key={item.id + "n"} value={item.notes || ""} multiline rows={4} placeholder="Notes…" aria-label="Notes" onCommit={(v) => actions.update(item.id, { notes: v })} />

        {item.url && (
          <div className={css.urlRow}>
            <input readOnly value={item.url} aria-label="Source URL" onFocus={(e) => e.target.select()} />
            <button aria-label="Open source" onClick={() => window.open(item.url, "_blank")}><I.Link /></button>
          </div>
        )}

        {/* The thread is the other half of a Hacker News or Reddit save — §3.5. */}
        {(item.meta?.hn?.url || item.meta?.reddit?.url) && (() => {
          const t = item.meta.hn ?? item.meta.reddit;
          const Mark = item.meta.hn ? I.HN : I.Reddit;
          return (
            <button className={css.discussion} onClick={() => window.open(t.url, "_blank")}>
              <Mark />
              <span>
                Discussion on {item.meta.hn ? "Hacker News" : `r/${t.subreddit}`}
                <small>{t.points ?? 0} points · {t.comments ?? 0} comments</small>
              </span>
            </button>
          );
        })()}

        <div className={css.label}>Tags</div>
        <Chips
          values={item.tags.map((t) => ({ id: t, label: t, auto: (item.autoTags || []).includes(t) }))}
          onAdd={(t) => actions.addTag(item.id, t)}
          onKeep={(t) => actions.addTag(item.id, t)}
          onRemove={(t) => actions.removeTag(item.id, t)}
        />

        <div className={css.label}>Folders</div>
        <Chips
          folder
          values={item.folderIds.map((f) => ({ id: f, label: folderPath(folders, f) || "?" }))}
          options={allFolders}
          onAdd={(f) => actions.addToFolder([item.id], f)}
          onRemove={(f) => actions.removeFromFolder(item.id, f)}
        />

        {item.summary && item.kind !== "snippet" && (
          <>
            <div className={css.divider} />
            <div className={css.agentHead}><I.Bolt />{item.status === "ready" ? "Summary" : item.status + "…"}</div>
            <div className={css.agent}>{item.summary}</div>
          </>
        )}

        {item.agentReason && (
          <div className={css.agent}>
            <b>Filed by agent</b>{item.confidence != null && ` · ${Math.round(item.confidence * 100)}%`}<br />
            {item.agentReason}
          </div>
        )}

        {item.meta?.suggestedFolderId && !item.folderIds.includes(item.meta.suggestedFolderId) && (
          <div className={css.suggest}>
            <span>Suggested: <b>{folderPath(folders, item.meta.suggestedFolderId) || "new folder"}</b></span>
            <button className={css.btn} onClick={() => actions.addToFolder([item.id], item.meta.suggestedFolderId)}>Accept</button>
          </div>
        )}

        {item.error && <div className={css.error}>{item.error}</div>}

        {item.transcript && (
          <>
            <div className={css.divider} />
            <div className={css.label}>Transcript</div>
            <pre className={css.transcript}>{item.transcript}</pre>
          </>
        )}

        <div className={css.divider} />
        <div className={css.label}>Properties</div>
        <dl className={css.props}>
          <dt>Rating</dt>
          <dd>
            <span className={css.stars} role="radiogroup" aria-label="Rating">
              {[1, 2, 3, 4, 5].map((n) => (
                <button key={n} role="radio" aria-checked={item.rating === n} aria-label={`${n} stars`}
                  className={n <= item.rating ? undefined : css.off}
                  onClick={() => actions.setRating(item.id, n)}>
                  <I.Star filled={n <= item.rating} />
                </button>
              ))}
            </span>
          </dd>
          {item.duration && <><dt>Duration</dt><dd>{item.duration}</dd></>}
          <dt>Source</dt><dd>{item.domain}</dd>
          <dt>Dimensions</dt><dd>{item.dimensions}</dd>
          <dt>Size</dt><dd>{item.size}</dd>
          <dt>Type</dt><dd>{kindLabel[item.kind] || item.kind}</dd>
          <dt>Captured</dt><dd>{fmt(item.addedAt)}</dd>
          <dt>Status</dt><dd>{item.status}{item.userEdited ? " · edited" : ""}</dd>
        </dl>

        <div className={css.divider} />
        <div className={css.actions}>
          <button className={css.btn} onClick={() => actions.reenrich([item.id])}>
            <I.Bolt /> Re-enrich
          </button>
          {item.trashed ? (
            <button className={css.btn} onClick={() => actions.trash([item.id], false)}>Put back</button>
          ) : (
            <button className={`${css.btn} ${css.danger}`} onClick={() => actions.trash([item.id])}><I.Trash /> Trash</button>
          )}
        </div>
      </div>
    </aside>
  );
}

const fmt = (iso) => {
  const d = new Date(iso);
  return isNaN(d) ? iso : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
};
