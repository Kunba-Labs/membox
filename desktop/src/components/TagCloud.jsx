import { useMemo } from "react";
import css from "./TagCloud.module.css";
import { useStore, allTags } from "../store.js";

// §2.5 — All Tags is a cloud: the word carries the count in its size, so the
// shape of the library is readable at a glance. Clicking one filters.
//
// Alphabetical, not ranked: a cloud sorted by frequency is just a list with
// odd typography. Size does the ranking, order does the finding.
export default function TagCloud({ onPick }) {
  const tags = useStore(allTags);
  const cloud = useMemo(() => [...tags].sort((a, b) => a[0].localeCompare(b[0])), [tags]);

  if (!tags.length) return <div className={css.empty}>No tags yet.</div>;

  const max = Math.max(...tags.map(([, n]) => n));
  // √ rather than linear: one tag on half the library would otherwise leave
  // every other word at the floor size.
  const weightOf = (n) => Math.sqrt(n / max);

  return (
    <div className={css.cloud}>
      <div className={css.words}>
        {cloud.map(([t, n]) => {
          const w = weightOf(n);
          return (
            <button
              key={t}
              className={css.tag}
              title={`${n} item${n === 1 ? "" : "s"}`}
              style={{
                fontSize: `${13 + w * 26}px`,
                fontWeight: 400 + Math.round(w * 3) * 100,
                opacity: 0.55 + w * 0.45,
              }}
              onClick={() => onPick(t)}
            >
              {t}
            </button>
          );
        })}
      </div>
      <div className={css.foot}>{tags.length} tags · biggest is what you save most</div>
    </div>
  );
}
