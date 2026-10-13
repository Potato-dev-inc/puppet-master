"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { AGENTS, Agent, group, STATE_TEXT } from "@/lib/mock";
import { Glyph } from "./Glyph";
import { Icon } from "./Icon";
import { PaneActions, PaneView } from "./PaneView";
import { Cmd, Palette } from "./Palette";
import { Tray } from "./Tray";

export function Workspace() {
  const [agents, setAgents] = useState<Agent[]>(AGENTS);
  const [sel, setSel] = useState<string | null>(null);
  const [pal, setPal] = useState(false);
  const [tray, setTray] = useState(false);
  const [toast, setToast] = useState("");
  const flash = (m: string) => { setToast(m); setTimeout(() => setToast(""), 1600); };

  useEffect(() => {
    const k = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); setPal((p) => !p); } };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  const patch = (id: string, f: (a: Agent) => Agent) => setAgents((as) => as.map((a) => (a.id === id ? f(a) : a)));
  const answer = (id: string, ans: string) =>
    patch(id, (a) => ({ ...a, state: "running", elapsed: "now", question: undefined, lines: [...a.lines, { k: "user", s: `› ${ans}` }, { k: "bullet", s: "Continuing." }] }));
  const interrupt = (id: string) => patch(id, (a) => ({ ...a, state: "idle", question: undefined, lines: [...a.lines, { k: "dim", s: "Interrupted." }] }));
  const addAgent = (name: string) => {
    const id = "n" + Date.now();
    setAgents((as) => [{ id, name, project: "new session", state: "idle", elapsed: "now", cols: 120, rows: 36, lines: [{ k: "prompt", s: "% " + name.toLowerCase() }] }, ...as]);
    setSel(id);
  };

  const cur = agents.find((a) => a.id === sel) ?? null;
  const groups = (["Needs you", "Working", "Idle"] as const).map((g) => ({ g, items: agents.filter((a) => group(a) === g) })).filter((x) => x.items.length);
  const waiting = agents.filter((a) => a.question);

  const actions = (a: Agent): PaneActions => ({
    answer: (x) => answer(a.id, x),
    usePhone: () => patch(a.id, (p) => ({ ...p, phone: false })),
    interrupt: () => interrupt(a.id),
    close: () => { setAgents((as) => as.filter((x) => x.id !== a.id)); setSel(null); },
    popOut: () => flash("Opened in a separate window"),
    send: (t) => patch(a.id, (p) => ({ ...p, state: "running", lines: [...p.lines, { k: "user", s: `› ${t}` }] })),
  });

  const cmds: Cmd[] = useMemo(() => [
    ...agents.map((a) => ({ id: "go" + a.id, label: `Switch to ${a.name} · ${a.project}`, hint: STATE_TEXT[a.state], run: () => setSel(a.id) })),
    { id: "new", label: "New agent: Claude Code", hint: "ready", run: () => addAgent("Claude Code") },
    { id: "newc", label: "New agent: Codex", hint: "ready", run: () => addAgent("Codex") },
    { id: "inbox", label: "Show inbox", run: () => setSel(null) },
    { id: "int", label: "Interrupt current agent", hint: "⌃C", run: () => sel && interrupt(sel) },
    { id: "set", label: "Open settings", hint: "⌘,", run: () => { window.location.href = "/settings"; } },
  // eslint-disable-next-line react-hooks/exhaustive-deps
  ], [agents, sel]);

  return (
    <div className="ws">
      <aside className="side">
        <div className="side-top">
          <span className="t-name">Agents</span>
          <button className="icon-btn" aria-label="New agent" onClick={() => addAgent("Claude Code")}><Icon n="plus" /></button>
        </div>
        <nav className="side-scroll" aria-label="Agents">
          <button className={`row ${sel === null ? "active" : ""}`} onClick={() => setSel(null)}>
            <Icon n="inbox" size={14} /><span className="grow">Inbox</span>{waiting.length > 0 && <span className="label3 tnum">{waiting.length}</span>}
          </button>
          {groups.map(({ g, items }) => (
            <div key={g}>
              <div className="caption">{g}</div>
              {items.map((a) => (
                <button key={a.id} className={`row ${sel === a.id ? "active" : ""} ${a.state === "needs" ? "needs" : ""}`} onClick={() => setSel(a.id)}>
                  <Glyph s={a.state} />
                  <span className="name">{a.name}</span>
                  <span className="label3 grow ellipsis">{a.project}</span>
                  <span className="label3 tnum">{a.elapsed}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="side-bottom">
          <Link href="/settings" className="row"><Icon n="gear" size={14} /><span>Settings</span><kbd className="grow r">⌘,</kbd></Link>
          <div className="conn"><span className="dot-ok" />Bridge · 127.0.0.1:17321</div>
          <div className="example pad">Example data</div>
        </div>
      </aside>

      <main className="main">
        {cur ? (
          <PaneView key={cur.id} agent={cur} act={actions(cur)} />
        ) : (
          <section className="inbox">
            <header className="topbar"><b className="t-name">Inbox</b><span className="label3">{waiting.length ? `${waiting.length} waiting` : "Nothing waiting"}</span><span className="sp" />
              <button className="btn" onClick={() => setPal(true)}>Commands <kbd>⌘K</kbd></button></header>
            <div className="inbox-body">
              {waiting.length === 0 && <p className="label3 empty">All caught up. Agents will appear here when they need an answer.</p>}
              {waiting.map((a) => (
                <div key={a.id} className="irow">
                  <div className="grow">
                    <button className="link" onClick={() => setSel(a.id)}><b>{a.name}</b> <span className="label3">{a.project} · {a.elapsed}</span></button>
                    <div className="q">{a.question!.text}</div>
                    <div className="mono tail">{a.lines.slice(-2).map((l, i) => <div key={i}>{l.s}</div>)}</div>
                  </div>
                  <div className="seg">{a.question!.answers.map((x, i) => <button key={x} className={i === 0 ? "primary" : ""} onClick={() => answer(a.id, x)}>{x}</button>)}</div>
                </div>
              ))}
            </div>
          </section>
        )}
      </main>

      <button className="btn demo-tray" onClick={() => setTray(true)}>Tray</button>
      {toast && <div className="toast float">{toast}</div>}
      {pal && <Palette cmds={cmds} onClose={() => setPal(false)} />}
      {tray && <Tray agents={agents} onClose={() => setTray(false)} onPick={setSel} onSettings={() => { window.location.href = "/settings"; }} />}
    </div>
  );
}
