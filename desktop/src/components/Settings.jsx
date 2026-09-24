import { useEffect, useState } from "react";
import css from "./Settings.module.css";
import * as I from "../icons.jsx";
import { useStore, actions, tauri } from "../store.js";

// §8.4 — the agent lane is the one thing that decides what leaves the machine,
// so it is chosen here, in plain words, with the cloud/local fact next to it.
// Four panes, because four things are configurable; a rail beats a scroll.
const TABS = [
  { id: "agent", name: "Agent", icon: I.Bolt, blurb: "Who reads what you save, and what that means for your data." },
  { id: "browser", name: "Browser", icon: I.Layout, blurb: "membox's own browser — the only one an agent may drive." },
  { id: "sync", name: "Sync", icon: I.Swap, blurb: "Your devices, through your iCloud. No membox server." },
  { id: "library", name: "Library", icon: I.Inbox, blurb: "Where everything lives, and how to start over." },
  { id: "look", name: "Look", icon: I.Panel, blurb: "Two designs. The difference is shape and softness as much as colour." },
];

const THEMES = [
  { id: "bauhaus", name: "Bauhaus", note: "Ink panels on one flat ground, ruled apart. Square, and three primaries used at full strength — blue selects, red files, yellow marks." },
  { id: "glass", name: "Glass", note: "The original: translucent panes blurred over a gradient, soft corners, a wash where Bauhaus puts a block." },
];

export default function Settings({ onClose }) {
  const settings = useStore((s) => s.settings) || {};
  const update = useStore((s) => s.update);
  const [tab, setTab] = useState(update ? "library" : "agent");
  const [version, setVersion] = useState(null);
  const [upd, setUpd] = useState(null); // null · "checking" · "installing" · "current" · error text
  const [agents, setAgents] = useState([]);
  const [mcp, setMcp] = useState(null);
  const [sync, setSync] = useState(null);
  const [paths, setPaths] = useState(null);
  const [syncing, setSyncing] = useState(false);
  const [copied, setCopied] = useState(false);

  const checkUpdate = () => {
    setUpd("checking");
    actions.checkUpdate().then((v) => setUpd(v ? null : "current"), (e) => setUpd(String(e)));
  };
  const installUpdate = () => {
    setUpd("installing");
    actions.installUpdate().catch((e) => setUpd(String(e))); // success relaunches
  };

  const refreshSync = () => actions.syncStatus().then(setSync);
  const syncNow = async () => {
    setSyncing(true);
    try { setSync(await actions.sync()); } finally { setSyncing(false); }
  };

  useEffect(() => {
    actions.agents().then(setAgents);
    actions.mcp().then(setMcp);
    actions.paths().then(setPaths).catch(() => {});
    refreshSync();
    if (tauri) actions.version().then(setVersion);
    const key = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const lane = settings.agent || "off";
  const laneName = lane === "off" ? "Off" : agents.find((a) => a.key === lane)?.name || lane;
  const threshold = settings.autoFileThreshold ?? 0.7;
  const here = TABS.find((t) => t.id === tab);
  const theme = settings.theme || "bauhaus";

  return (
    <div className={css.backdrop} onMouseDown={onClose}>
      <div className={css.sheet} onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <nav className={css.rail}>
          <div className={css.railTitle}>Settings</div>
          {TABS.map((t) => (
            <button key={t.id} className={`${css.tab} ${tab === t.id ? css.tabOn : ""}`} onClick={() => setTab(t.id)}>
              <t.icon />
              {t.name}
              {t.id === "agent" && <span className={css.tabValue}>{laneName}</span>}
              {t.id === "sync" && settings.syncEnabled && <span className={css.tabDot} title="on" />}
              {t.id === "library" && update && <span className={css.tabDot} title="update waiting" />}
            </button>
          ))}
        </nav>

        <section className={css.pane}>
          <header className={css.paneHead}>
            <h2>{here.name}</h2>
            <p>{here.blurb}</p>
            <button className={css.close} aria-label="Close" onClick={onClose}><I.Close /></button>
          </header>

          <div className={css.body}>
            {tab === "look" && (
              <div className={css.lanes}>
                {THEMES.map((t) => (
                  <label key={t.id} className={`${css.lane} ${theme === t.id ? css.on : ""}`}>
                    <input type="radio" name="theme" checked={theme === t.id} onChange={() => actions.settings({ theme: t.id })} />
                    <span className={css.laneName}>{t.name}</span>
                    <span className={css.themeMark} data-theme-mark={t.id} aria-hidden="true">
                      <i /><i /><i />
                    </span>
                    <span className={css.laneNote}>{t.note}</span>
                  </label>
                ))}
              </div>
            )}

            {tab === "agent" && (
              <>
                <div className={css.lanes}>
                  <label className={`${css.lane} ${lane === "off" ? css.on : ""}`}>
                    <input type="radio" name="lane" checked={lane === "off"} onChange={() => actions.settings({ agent: "off" })} />
                    <span className={css.laneName}>Off</span>
                    <span className={css.laneNote}>Capture and screenshot only. You file things yourself.</span>
                  </label>
                  {agents.filter((a) => a.key !== "off").map((a) => (
                    <label key={a.key} className={`${css.lane} ${lane === a.key ? css.on : ""} ${a.installed ? "" : css.missing}`}>
                      <input type="radio" name="lane" disabled={!a.installed} checked={lane === a.key} onChange={() => actions.settings({ agent: a.key })} />
                      <span className={css.laneName}>{a.name}</span>
                      <span className={`${css.pill} ${a.sendsToCloud ? css.cloud : css.local}`}>
                        <i /> {a.sendsToCloud ? "cloud" : "local"}
                      </span>
                      <span className={css.laneNote}>
                        {a.installed ? <code>{a.path}</code> : <>not installed — <a href={a.installUrl} target="_blank" rel="noreferrer">install</a></>}
                      </span>
                    </label>
                  ))}
                </div>

                {lane === "local" && (
                  <div className={css.field}>
                    <div className={css.fieldLabel}>
                      Model
                      <small>Any tag `ollama list` knows.</small>
                    </div>
                    <input className={css.text} defaultValue={settings.localModel} onBlur={(e) => actions.settings({ localModel: e.target.value })} placeholder="qwen2.5:7b" />
                  </div>
                )}

                {lane !== "off" && (
                  <div className={css.field}>
                    <div className={css.fieldLabel}>
                      At once
                      <small>How many agent runs in parallel. They are separate processes on your machine; the browser is still one page at a time.</small>
                    </div>
                    <div className={css.slider}>
                      <input
                        type="range" min="1" max="8" step="1"
                        defaultValue={settings.agentConcurrency ?? 3}
                        onChange={(e) => actions.settings({ agentConcurrency: +e.target.value })}
                      />
                      <span className={css.val}>{settings.agentConcurrency ?? 3}×</span>
                    </div>
                  </div>
                )}

                <div className={css.field}>
                  <div className={css.fieldLabel}>
                    Auto-file
                    <small>Below this, the agent's folder is a suggestion in the inspector instead of a move.</small>
                  </div>
                  <div className={css.slider}>
                    <input
                      type="range" min="0" max="1" step="0.05"
                      defaultValue={threshold}
                      onChange={(e) => actions.settings({ autoFileThreshold: +e.target.value })}
                    />
                    <span className={css.val}>{Math.round(threshold * 100)}%</span>
                  </div>
                </div>
              </>
            )}

            {tab === "browser" && (
              <>
                <p className={css.prose}>
                  Pages are fetched in a hidden WebKit window that belongs to membox — one cookie jar, no Chrome, nothing spawned behind your back.
                  An agent reaches it over a loopback MCP endpoint with a token that changes every run, and every call it makes is recorded against the item it was enriching.
                </p>
                <div className={css.field}>
                  <div className={css.fieldLabel}>Endpoint<small>127.0.0.1 only.</small></div>
                  <button
                    className={css.mono}
                    title="Copy"
                    onClick={() => { navigator.clipboard?.writeText(mcp?.url || ""); setCopied(true); setTimeout(() => setCopied(false), 1200); }}
                  >
                    {copied ? "copied" : mcp?.url || "not running"}
                  </button>
                </div>
                {tauri && (
                  <div className={css.field}>
                    <div className={css.fieldLabel}>Window<small>Watch a fetch happen, for when a page misbehaves.</small></div>
                    <button className={css.btn} onClick={() => import("@tauri-apps/api/core").then(({ invoke }) => invoke("show_browser", { visible: true }))}>
                      Show browser
                    </button>
                  </div>
                )}
              </>
            )}

            {tab === "sync" && (
              tauri ? (
                <>
                  <label className={`${css.lane} ${settings.syncEnabled ? css.on : ""}`}>
                    <input type="checkbox" checked={!!settings.syncEnabled} onChange={(e) => actions.settings({ syncEnabled: e.target.checked }).then(refreshSync)} />
                    <span className={css.laneName}>Keep this library in sync across my devices</span>
                    <span className={`${css.pill} ${css.local}`}><i /> your iCloud</span>
                    <span className={css.laneNote}><code>{sync?.dir || "iCloud Drive not available on this Mac"}</code></span>
                  </label>
                  <p className={css.prose}>
                    Each device writes its own snapshot into that folder and merges the others'. The database itself never moves.
                    Items, folders, tags and thumbnails travel; full-page shots stay where they were taken.
                  </p>
                  <div className={css.field}>
                    <div className={css.fieldLabel}>
                      Status
                      <small>
                        {sync?.error ? <span className={css.bad}>{sync.error}</span>
                          : sync?.lastImport ? `Merged ${new Date(sync.lastImport).toLocaleTimeString()} from ${sync.devices} other device${sync.devices === 1 ? "" : "s"}.`
                            : "Nothing merged yet."}
                      </small>
                    </div>
                    <button className={css.btn} disabled={syncing || !settings.syncEnabled} onClick={syncNow}>{syncing ? "Syncing…" : "Sync now"}</button>
                  </div>
                </>
              ) : (
                <p className={css.prose}>Sync needs the desktop app — this is the browser preview.</p>
              )
            )}

            {tab === "library" && (
              <>
                {tauri && (
                  <div className={css.field}>
                    <div className={css.fieldLabel}>
                      Version
                      <small>
                        membox {version ?? "…"}
                        {update ? ` — ${update} is ready.`
                          : upd === "current" ? " — up to date."
                            : upd && upd !== "checking" && upd !== "installing" ? <> — <span className={css.bad}>{upd}</span></> : ""}
                      </small>
                    </div>
                    {update ? (
                      <button className={css.btn} disabled={upd === "installing"} onClick={installUpdate}>
                        {upd === "installing" ? "Updating…" : "Update and restart"}
                      </button>
                    ) : (
                      <button className={css.btn} disabled={upd === "checking"} onClick={checkUpdate}>
                        {upd === "checking" ? "Checking…" : "Check for updates"}
                      </button>
                    )}
                  </div>
                )}
                <div className={css.field}>
                  <div className={css.fieldLabel}>
                    Where it lives
                    <small><code>{paths?.data ?? "…"}</code></small>
                  </div>
                </div>
                <div className={css.field}>
                  <div className={css.fieldLabel}>
                    Log
                    <small>Every fetch, every agent run, with timings — read this when something filed itself strangely.</small>
                  </div>
                  <button
                    className={css.mono}
                    title="Copy the path"
                    onClick={() => { navigator.clipboard?.writeText(paths?.log || ""); setCopied(true); setTimeout(() => setCopied(false), 1200); }}
                  >
                    {copied ? "copied" : "membox.log"}
                  </button>
                </div>
                <div className={css.field}>
                  <div className={css.fieldLabel}>
                    Backups
                    <small>Snapshots live outside the library. <code>bin/backup</code> takes one, <code>bin/restore</code> puts it back.</small>
                  </div>
                </div>
                <div className={css.field}>
                  <div className={css.fieldLabel}>
                    Start over
                    <small>Deletes every item and folder in this library. The backups stay.</small>
                  </div>
                  <button
                    className={`${css.btn} ${css.danger}`}
                    onClick={() => confirm(tauri ? "Delete every item and folder and start over?" : "Reset the demo library?") && actions.reset().then(onClose)}
                  >
                    {tauri ? "Start over" : "Reset demo data"}
                  </button>
                </div>
              </>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
