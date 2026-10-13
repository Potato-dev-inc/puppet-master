"use client";
import Link from "next/link";
import { useMemo, useState } from "react";
import { Confirm } from "./Confirm";
import { ExampleBadge } from "./ExampleBadge";
import s from "./settings.module.css";

type Row =
  | { id: string; label: string; hint: string; kind: "toggle"; def: boolean }
  | { id: string; label: string; hint: string; kind: "select"; def: string; options: string[] }
  | { id: string; label: string; hint: string; kind: "text"; def: string }
  | { id: string; label: string; hint: string; kind: "status"; items: { name: string; ok: boolean; note: string }[] };

type Section = { id: string; title: string; rows: Row[] };

const SECTIONS: Section[] = [
  { id: "general", title: "General", rows: [
    { id: "startup", label: "Start with Windows", hint: "Launch to the tray, no window.", kind: "toggle", def: true },
    { id: "pos", label: "New pane position", hint: "Where pane windows open.", kind: "select", def: "Primary monitor", options: ["Primary monitor", "Last used monitor", "Left monitor"] },
    { id: "confirmclose", label: "Confirm before closing a running pane", hint: "Closing a pane window kills its process.", kind: "toggle", def: true },
    { id: "confirmquit", label: "Confirm before quitting", hint: "Quit closes every pane.", kind: "toggle", def: true },
    { id: "notify", label: "Notify when an agent needs you", hint: "Tray badge turns yellow.", kind: "toggle", def: true },
  ] },
  { id: "agents", title: "Agents & Preload", rows: [
    { id: "preload", label: "Keep agents warm (Preload)", hint: "Starts hidden panes so New pane is instant.", kind: "toggle", def: true },
    { id: "warm", label: "Warm agents", hint: "Live status of pre-built panes.", kind: "status", items: [
      { name: "Claude Code", ok: true, note: "ready" }, { name: "Codex", ok: true, note: "ready" },
      { name: "OpenCode", ok: false, note: "starting, 6s" }, { name: "Kilo", ok: true, note: "ready" } ] },
    { id: "defagent", label: "Default agent", hint: "Used by the tray's first New pane entry.", kind: "select", def: "Claude Code", options: ["Claude Code", "Codex", "OpenCode", "Kilo"] },
    { id: "cols", label: "Default pane width (columns)", hint: "Height is derived from the monitor.", kind: "text", def: "120" },
  ] },
  { id: "orch", title: "Orchestrator & Models", rows: [
    { id: "orchmodel", label: "Orchestrator model", hint: "Plans and delegates work to panes.", kind: "select", def: "claude-sonnet", options: ["claude-sonnet", "claude-opus", "gpt-5"] },
    { id: "workermodel", label: "Worker model", hint: "Default for delegated tasks.", kind: "select", def: "codex-mini", options: ["codex-mini", "claude-haiku", "kilo-auto"] },
    { id: "autoanswer", label: "Auto-answer safe prompts", hint: "Read-only commands only. Everything else waits for you.", kind: "toggle", def: false },
    { id: "maxpanes", label: "Max concurrent panes", hint: "Orchestrator will queue past this.", kind: "text", def: "8" },
  ] },
  { id: "keys", title: "Keys", rows: [
    { id: "anthropic", label: "Anthropic API key", hint: "Stored in the OS keychain.", kind: "text", def: "sk-ant-...e41f" },
    { id: "openai", label: "OpenAI API key", hint: "Stored in the OS keychain.", kind: "text", def: "" },
    { id: "ocrotate", label: "Rotate OpenCode keys on rate limit", hint: "Uses the key pool below.", kind: "toggle", def: true },
  ] },
  { id: "bridge", title: "Bridge & MCP", rows: [
    { id: "mcp", label: "MCP runtime", hint: "What agents connect to.", kind: "status", items: [
      { name: "Bridge", ok: true, note: "listening on 127.0.0.1:7421" }, { name: "MCP server", ok: true, note: "v0.9.2, 6 clients" },
      { name: "Last restart", ok: true, note: "3h ago" } ] },
    { id: "mcpauto", label: "Register MCP with new agents", hint: "Writes config on first spawn.", kind: "toggle", def: true },
    { id: "bport", label: "Bridge port", hint: "Restart required after change.", kind: "text", def: "7421" },
  ] },
  { id: "phone", title: "Phone", rows: [
    { id: "phoneon", label: "Phone access", hint: "Pair a phone via QR code.", kind: "toggle", def: true },
    { id: "phonesize", label: "Resize panes for phone", hint: "A pane shows a Reclaim banner while sized for the phone.", kind: "select", def: "Ask each time", options: ["Ask each time", "Always", "Never"] },
    { id: "devices", label: "Paired devices", hint: "Currently connected.", kind: "status", items: [{ name: "Pixel 8", ok: true, note: "connected" }, { name: "iPad", ok: false, note: "last seen 2d ago" }] },
  ] },
];

const LATER = ["Cost dashboard", "Team sync", "Scheduled runs", "Session replay", "Plugin marketplace", "Cloud panes"];

type Vals = Record<string, string | boolean>;
const initial = (): Vals => {
  const v: Vals = {};
  SECTIONS.forEach((sec) => sec.rows.forEach((r) => { if (r.kind !== "status") v[r.id] = r.def; }));
  return v;
};

export function SettingsApp() {
  const [saved, setSaved] = useState<Vals>(initial);
  const [draft, setDraft] = useState<Vals>(initial);
  const [sec, setSec] = useState("general");
  const [q, setQ] = useState("");
  const [guard, setGuard] = useState(false);
  const [toast, setToast] = useState("");

  const dirtyIds = Object.keys(draft).filter((k) => draft[k] !== saved[k]);
  const dirty = dirtyIds.length > 0;
  const query = q.trim().toLowerCase();

  const results = useMemo(() => {
    if (!query) return null;
    return SECTIONS.map((sc) => ({ sc, rows: sc.rows.filter((r) => (r.label + " " + r.hint).toLowerCase().includes(query)) })).filter((x) => x.rows.length);
  }, [query]);

  const visible = results ?? [{ sc: SECTIONS.find((x) => x.id === sec)!, rows: SECTIONS.find((x) => x.id === sec)!.rows }];
  const set = (id: string, v: string | boolean) => setDraft((d) => ({ ...d, [id]: v }));
  const save = () => { setSaved(draft); setToast("Saved " + dirtyIds.length + " change" + (dirtyIds.length > 1 ? "s" : "")); setTimeout(() => setToast(""), 2200); };
  const countDirty = (sc: Section) => sc.rows.filter((r) => dirtyIds.includes(r.id)).length;

  return (
    <div className={s.win}>
      <aside className={s.side}>
        <Link href="/" className="back">Index</Link>
        <input className={s.search} placeholder="Search settings" aria-label="Search settings" value={q} onChange={(e) => setQ(e.target.value)} />
        <nav>
          {SECTIONS.map((x) => (
            <button key={x.id} className={`${s.nav} ${!query && sec === x.id ? s.on : ""}`} onClick={() => { setSec(x.id); setQ(""); }}>
              {x.title}{countDirty(x) > 0 && <span className={s.dot} aria-label="unsaved changes" />}
            </button>
          ))}
        </nav>
        <details className={s.later}>
          <summary>Coming later</summary>
          <ul>{LATER.map((l) => <li key={l}>{l}</li>)}</ul>
        </details>
        <div style={{ marginTop: "auto" }}><ExampleBadge /></div>
      </aside>

      <main className={s.main}>
        <header className={s.head}>
          <h1>{results ? `Results for "${q}"` : SECTIONS.find((x) => x.id === sec)!.title}</h1>
          <button className="cap ghost" onClick={() => (dirty ? setGuard(true) : (window.location.href = "/"))}>Close</button>
        </header>
        {results && results.length === 0 && <p className={s.empty}>No settings match. Try a shorter word.</p>}
        {visible.map(({ sc, rows }) => (
          <section key={sc.id} className={`glass ${s.card}`}>
            {results && <h2 className={s.sub}>{sc.title}</h2>}
            {rows.map((r) => (
              <div key={r.id} className={s.row}>
                <div className={s.txt}>
                  <label htmlFor={r.id}>{r.label}</label>
                  <span>{r.hint}</span>
                </div>
                <div className={s.ctl}>
                  {r.kind === "toggle" && (
                    <button id={r.id} role="switch" aria-checked={draft[r.id] as boolean} className={`${s.sw} ${draft[r.id] ? s.swOn : ""}`} onClick={() => set(r.id, !draft[r.id])}><i /></button>
                  )}
                  {r.kind === "select" && (
                    <select id={r.id} className={s.input} value={draft[r.id] as string} onChange={(e) => set(r.id, e.target.value)}>
                      {r.options.map((o) => <option key={o}>{o}</option>)}
                    </select>
                  )}
                  {r.kind === "text" && <input id={r.id} className={s.input} value={draft[r.id] as string} onChange={(e) => set(r.id, e.target.value)} />}
                  {r.kind === "status" && (
                    <ul className={s.status}>
                      {r.items.map((i) => (
                        <li key={i.name}><i style={{ background: i.ok ? "var(--phase-tool)" : "var(--flag-yellow)" }} /><b>{i.name}</b><span>{i.note}</span></li>
                      ))}
                    </ul>
                  )}
                  {r.kind !== "status" && dirtyIds.includes(r.id) && <em className={s.edited}>Edited</em>}
                </div>
              </div>
            ))}
          </section>
        ))}
        <div style={{ height: 90 }} />
      </main>

      {(dirty || toast) && (
        <div className={`glass ${s.bar}`} role="status">
          {dirty ? (
            <>
              <span><b>{dirtyIds.length}</b> unsaved change{dirtyIds.length > 1 ? "s" : ""}</span>
              <button className="cap ghost" onClick={() => setDraft(saved)}>Discard</button>
              <button className="cap go" onClick={save}>Save changes</button>
            </>
          ) : <span style={{ color: "var(--phase-tool)" }}>{toast}</span>}
        </div>
      )}

      {guard && (
        <Confirm title="Discard unsaved changes?" body={`${dirtyIds.length} setting${dirtyIds.length > 1 ? "s" : ""} changed and not saved.`} confirmLabel="Discard and close" cancelLabel="Keep editing" danger
          onCancel={() => setGuard(false)} onConfirm={() => { setDraft(saved); setGuard(false); }} />
      )}
    </div>
  );
}
