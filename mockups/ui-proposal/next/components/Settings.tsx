"use client";
import Link from "next/link";
import { useRef, useState } from "react";
import { Icon } from "./Icon";
import { Sheet } from "./Sheet";
import { SegTabs } from "./SegTabs";
import { ThemeToggle } from "./ThemeToggle";

type Item =
  | { id: string; label: string; sub?: string; kind: "switch"; def: boolean }
  | { id: string; label: string; sub?: string; kind: "popup"; def: string; opts: string[] }
  | { id: string; label: string; sub?: string; kind: "stepper"; def: number; min: number; max: number }
  | { id: string; label: string; sub?: string; kind: "text"; def: string }
  | { id: string; label: string; sub?: string; kind: "info"; value: string }
  | { id: string; label: string; sub?: string; kind: "seg"; def: string; opts: string[] }
  | { id: string; label: string; sub?: string; kind: "theme" };
type Danger = { id: string; label: string; sub: string; action: string; body: string };
type Section = { id: string; title: string; groups: { head?: string; items: Item[] }[]; danger?: Danger[] };

const S: Section[] = [
  { id: "general", title: "General", groups: [
    { items: [
      { id: "theme", label: "Appearance", sub: "Dark or light.", kind: "theme" },
      { id: "login", label: "Open at login", sub: "Starts in the menu bar, no window.", kind: "switch", def: true },
      { id: "notify", label: "Notify when an agent needs you", kind: "switch", def: true },
    ] },
    { head: "Windows", items: [
      { id: "pos", label: "New pane position", kind: "popup", def: "Primary display", opts: ["Primary display", "Last used display", "Left display"] },
      { id: "cc", label: "Confirm before closing a running pane", kind: "switch", def: true },
      { id: "cq", label: "Confirm before quitting", kind: "switch", def: true },
    ] },
  ] },
  { id: "agents", title: "Agents", groups: [
    { items: [
      { id: "preload", label: "Keep agents ready", sub: "Warm panes make New pane instant.", kind: "switch", def: true },
      { id: "def", label: "Default agent", kind: "popup", def: "Claude Code", opts: ["Claude Code", "Codex", "OpenCode", "Kilo"] },
      { id: "cols", label: "Default width", sub: "Columns", kind: "stepper", def: 120, min: 60, max: 240 },
    ] },
    { head: "Ready now", items: [
      { id: "w1", label: "Claude Code", kind: "info", value: "Ready" }, { id: "w2", label: "Codex", kind: "info", value: "Ready" },
      { id: "w3", label: "OpenCode", kind: "info", value: "Starting" }, { id: "w4", label: "Kilo", kind: "info", value: "Ready" },
    ] },
  ] },
  { id: "models", title: "Orchestrator & Models", groups: [
    { items: [
      { id: "om", label: "Orchestrator model", kind: "popup", def: "claude-sonnet", opts: ["claude-sonnet", "claude-opus", "gpt-5"] },
      { id: "wm", label: "Worker model", kind: "popup", def: "codex-mini", opts: ["codex-mini", "claude-haiku", "kilo-auto"] },
      { id: "mp", label: "Concurrent panes", sub: "Extra work waits in a queue.", kind: "stepper", def: 8, min: 1, max: 32 },
    ] },
    { items: [{ id: "aa", label: "Answer safe prompts automatically", sub: "Read-only commands only.", kind: "switch", def: false }] },
  ] },
  { id: "keys", title: "Keys", groups: [
    { items: [
      { id: "ak", label: "Anthropic", sub: "Stored in the system keychain.", kind: "text", def: "sk-ant-…e41f" },
      { id: "ok", label: "OpenAI", kind: "text", def: "" },
      { id: "rot", label: "Rotate OpenCode keys on rate limit", kind: "switch", def: true },
    ] },
  ], danger: [{ id: "rk", label: "Remove all keys", sub: "Agents lose access until you add keys again.", action: "Remove", body: "All stored API keys will be deleted from the keychain." }] },
  { id: "bridge", title: "Bridge & MCP", groups: [
    { items: [
      { id: "b1", label: "Bridge", kind: "info", value: "127.0.0.1:17321" },
      { id: "b2", label: "MCP server", kind: "info", value: "Running, 6 clients" },
      { id: "b3", label: "Last restart", kind: "info", value: "3 hours ago" },
    ] },
    { items: [
      { id: "reg", label: "Add MCP to new agents", kind: "switch", def: true },
      { id: "port", label: "Port", sub: "Restart required.", kind: "text", def: "17321" },
    ] },
  ], danger: [{ id: "un", label: "Uninstall MCP integration", sub: "Removes the config written into agents.", action: "Uninstall", body: "Agents will no longer see Puppet Master tools." }] },
  { id: "phone", title: "Phone", groups: [
    { items: [
      { id: "ph", label: "Allow phone access", kind: "switch", def: true },
      { id: "ps", label: "Resize panes for phone", kind: "seg", def: "Ask", opts: ["Ask", "Always", "Never"] },
    ] },
    { head: "Paired", items: [{ id: "d1", label: "Pixel 8", kind: "info", value: "Connected" }, { id: "d2", label: "iPad", kind: "info", value: "2 days ago" }] },
  ], danger: [{ id: "rv", label: "Revoke all devices", sub: "Phones must pair again.", action: "Revoke", body: "Every paired phone will be signed out." }] },
];

const LATER = ["Cost overview", "Team sync", "Scheduled runs", "Session replay", "Plugin library", "Cloud panes"];
type V = string | boolean | number;

export function Settings() {
  const [sec, setSec] = useState("general");
  const [q, setQ] = useState("");
  const [vals, setVals] = useState<Record<string, V>>(() => {
    const v: Record<string, V> = {};
    S.forEach((s) => s.groups.forEach((g) => g.items.forEach((i) => { if ("def" in i) v[i.id] = i.def as V; })));
    return v;
  });
  const [saved, setSaved] = useState(false);
  const [ask, setAsk] = useState<Danger | null>(null);
  const [loading, setLoading] = useState(false);
  const go = (id: string) => { setSec(id); setQ(""); setLoading(true); setTimeout(() => setLoading(false), 300); };
  const timer = useRef<ReturnType<typeof setTimeout>>();

  const set = (id: string, v: V) => {
    setVals((x) => ({ ...x, [id]: v }));
    setSaved(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setSaved(false), 1500);
  };

  const query = q.trim().toLowerCase();
  const hit = (i: Item) => (i.label + " " + (i.sub ?? "")).toLowerCase().includes(query);
  const cur = S.find((s) => s.id === sec);
  const results = query
    ? S.map((s) => ({ s, items: s.groups.flatMap((g) => g.items).filter(hit) })).filter((x) => x.items.length)
    : null;

  const ctl = (i: Item) => {
    const v = vals[i.id];
    if (i.kind === "switch")
      return <button role="switch" aria-checked={!!v} aria-label={i.label} className={`switch ${v ? "on" : ""}`} onClick={() => set(i.id, !v)}><i /></button>;
    if (i.kind === "theme") return <ThemeToggle label />;
    if (i.kind === "seg") return <SegTabs value={v as string} options={i.opts} onChange={(x) => set(i.id, x)} label={i.label} />;
    if (i.kind === "popup")
      return <select className="popup" aria-label={i.label} value={v as string} onChange={(e) => set(i.id, e.target.value)}>{i.opts.map((o) => <option key={o}>{o}</option>)}</select>;
    if (i.kind === "stepper")
      return (
        <span className="stepper"><span className="tnum">{v as number}</span>
          <button aria-label="Decrease" onClick={() => set(i.id, Math.max(i.min, (v as number) - 4))}>−</button>
          <button aria-label="Increase" onClick={() => set(i.id, Math.min(i.max, (v as number) + 4))}>+</button></span>
      );
    if (i.kind === "text")
      return <input className="field" aria-label={i.label} value={v as string} placeholder="Not set" onChange={(e) => set(i.id, e.target.value)} />;
    return <span className="label2">{i.value}</span>;
  };

  const list = (items: Item[]) => (
    <ul className="group">
      {items.map((i) => (
        <li key={i.id}><div className="lbl"><span>{i.label}</span>{i.sub && <small>{i.sub}</small>}</div>{ctl(i)}</li>
      ))}
    </ul>
  );

  return (
    <div className="setwin">
      <aside className="side">
        <div className="side-top"><Link href="/workspace" className="icon-btn" aria-label="Back to workspace"><Icon n="back" /></Link></div>
        <div className="searchbox"><Icon n="search" size={14} /><input placeholder="Search" aria-label="Search settings" value={q} onChange={(e) => setQ(e.target.value)} /></div>
        <nav className="side-scroll">
          {S.map((s) => (
            <button key={s.id} className={`row ${!query && sec === s.id ? "active" : ""}`} onClick={() => go(s.id)}>{s.title}</button>
          ))}
        </nav>
        <div className="side-bottom">
          <button className={`row ${!query && sec === "later" ? "active" : ""}`} onClick={() => go("later")}><Icon n="later" size={14} /><span>Coming later</span></button>
          <div className="example pad">Example data</div>
        </div>
      </aside>
      <main className="setmain">
        <header className="titlebar">
          <h1>{query ? "Search results" : sec === "later" ? "Coming later" : cur!.title}</h1>
          <span className={`saved label3 ${saved ? "show" : ""}`} aria-live="polite">Saved</span>
        </header>
        <div className="setbody">
          {results && results.length === 0 && <p className="label3">No settings match “{q}”.</p>}
          {results?.map(({ s, items }) => (<div key={s.id}><h2>{s.title}</h2>{list(items)}</div>))}
          {!query && !loading && sec === "later" && (
            <ul className="group">{LATER.map((l) => <li key={l}><div className="lbl"><span>{l}</span></div><span className="label3">Not available yet</span></li>)}</ul>
          )}
          {!query && loading && <ul className="group skel">{[0, 1, 2].map((n) => <li key={n}><span className="sk" style={{ width: 120 + n * 24 }} /><span className="sk" style={{ width: 56 }} /></li>)}</ul>}
          {!query && !loading && cur && (
            <>
              {cur.groups.map((g, n) => (<div key={n}>{g.head && <h2>{g.head}</h2>}{list(g.items)}</div>))}
              {cur.danger && (
                <div className="dgroup">
                  <ul className="group">
                    {cur.danger.map((d) => (
                      <li key={d.id}><div className="lbl"><span>{d.label}</span><small>{d.sub}</small></div>
                        <button className="btn" onClick={() => setAsk(d)}>{d.action}…</button></li>
                    ))}
                  </ul>
                </div>
              )}
            </>
          )}
        </div>
      </main>
      {ask && <Sheet title={`${ask.label}?`} body={ask.body + " This can’t be undone."} confirm={ask.action} onCancel={() => setAsk(null)} onConfirm={() => { setAsk(null); setSaved(true); setTimeout(() => setSaved(false), 1500); }} />}
    </div>
  );
}
