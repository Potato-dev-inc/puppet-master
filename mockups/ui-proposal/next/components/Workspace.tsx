"use client";
import Link from "next/link";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { AGENTS, Agent, fmt, group, STATE_TEXT, WARM } from "@/lib/mock";
import { DotMatrix } from "./DotMatrix";
import { Icon } from "./Icon";
import { PaneActions, PaneView } from "./PaneView";
import { Cmd, Palette } from "./Palette";
import { ThemeToggle } from "./ThemeToggle";
import { Tray } from "./Tray";

type Toast = { id: number; msg: string; undo?: () => void };

function Dynamic({ words }: { words: string[] }) {
  const [i, setI] = useState(0);
  useEffect(() => { const t = setInterval(() => setI((x) => (x + 1) % words.length), 3000); return () => clearInterval(t); }, [words.length]);
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.span key={words[i % words.length]} initial={{ opacity: 0, y: 6, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
        exit={{ opacity: 0, y: -6, filter: "blur(4px)" }} transition={{ duration: 0.22 }} style={{ display: "inline-block" }}>{words[i % words.length]}</motion.span>
    </AnimatePresence>
  );
}

export function Workspace() {
  const [agents, setAgents] = useState<Agent[]>(AGENTS);
  const [sel, setSel] = useState<string | null>(null);
  const [pal, setPal] = useState(false);
  const [tray, setTray] = useState(false);
  const [plus, setPlus] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [tick, setTick] = useState(0);
  const plusRef = useRef<HTMLDivElement>(null);

  useEffect(() => { const t = setInterval(() => setTick((x) => x + 1), 1000); return () => clearInterval(t); }, []);
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); setPal((p) => !p); } };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  useEffect(() => {
    if (!plus) return;
    const h = (e: MouseEvent) => { if (!plusRef.current?.contains(e.target as Node)) setPlus(false); };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [plus]);

  const toast = (msg: string, undo?: () => void) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, msg, undo }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4000);
  };
  const patch = (id: string, f: (a: Agent) => Agent) => setAgents((as) => as.map((a) => (a.id === id ? f(a) : a)));
  const answer = (a: Agent, ans: string) => {
    const before = a;
    patch(a.id, (x) => ({ ...x, state: "running", sec: 0, question: undefined, lines: [...x.lines, { k: "user", s: `› ${ans}` }, { k: "bullet", s: "Continuing." }] }));
    toast(`Answered ${a.name}: ${ans}`, () => patch(a.id, () => before));
  };
  const interrupt = (id: string) => patch(id, (a) => ({ ...a, state: "idle", question: undefined, lines: [...a.lines, { k: "dim", s: "Interrupted." }] }));
  const addAgent = (name: string) => {
    const id = "n" + Date.now();
    setAgents((as) => [{ id, name, project: "new session", state: "idle", sec: 0, cols: 120, rows: 36, spawning: true, lines: [{ k: "prompt", s: "% " + name.toLowerCase() }] }, ...as]);
    setSel(id); setPlus(false);
  };

  const cur = agents.find((a) => a.id === sel) ?? null;
  const groups = (["Needs you", "Working", "Idle"] as const).map((g) => ({ g, items: agents.filter((a) => group(a) === g) })).filter((x) => x.items.length);
  const waiting = agents.filter((a) => a.question);
  const count = String(waiting.length).padStart(2, "0");
  const oldest = waiting.length ? Math.max(...waiting.map((a) => a.sec)) + tick : 0;

  const actions = (a: Agent): PaneActions => ({
    answer: (x) => answer(a, x),
    usePhone: () => patch(a.id, (p) => ({ ...p, phone: false })),
    interrupt: () => interrupt(a.id),
    close: () => { setAgents((as) => as.filter((x) => x.id !== a.id)); setSel(null); },
    popOut: () => toast("Opened in a separate window"),
    ready: () => patch(a.id, (p) => ({ ...p, spawning: false, lines: [...p.lines, { k: "dim", s: "Ready." }] })),
    toast: (m) => toast(m),
    send: (t, mode) => patch(a.id, (p) => ({ ...p, state: "running", lines: [...p.lines, { k: "user", s: `› ${t}` }, { k: "dim", s: mode === "steer" ? "(steered current turn)" : "(queued as next turn)" }] })),
  });

  const cmds: Cmd[] = useMemo(() => [
    ...agents.map((a) => ({ id: "go" + a.id, group: "Agents" as const, icon: "terminal", label: `${a.name} · ${a.project}`, desc: STATE_TEXT[a.state], run: () => setSel(a.id) })),
    { id: "new", group: "Actions" as const, icon: "plus", label: "New agent: Claude Code", desc: "Warm standby is ready", hint: "⌘N", run: () => addAgent("Claude Code") },
    { id: "newc", group: "Actions" as const, icon: "plus", label: "New agent: Codex", desc: "Warm standby is ready", run: () => addAgent("Codex") },
    { id: "inbox", group: "Actions" as const, icon: "inbox", label: "Show inbox", desc: "Everything waiting on you", run: () => setSel(null) },
    { id: "int", group: "Actions" as const, icon: "stop", label: "Interrupt current agent", desc: "Sends Ctrl+C to the pane", hint: "⌃C", run: () => sel && interrupt(sel) },
    { id: "set", group: "Settings" as const, icon: "gear", label: "Open settings", desc: "Preferences and keys", hint: "⌘,", run: () => { window.location.href = "/settings"; } },
    { id: "set2", group: "Settings" as const, icon: "user", label: "Phone pairing", desc: "Settings, Phone", run: () => { window.location.href = "/settings"; } },
  // eslint-disable-next-line react-hooks/exhaustive-deps
  ], [agents, sel]);

  return (
    <div className="ws">
      <aside className="side">
        <div className="side-top">
          <span className="wordmark" data-text="Puppet Master"><span>Puppet Master</span></span>
          <div ref={plusRef} className="rel">
            <button className="icon-btn" aria-label="New agent" aria-expanded={plus} data-tip="New agent" onClick={() => setPlus(!plus)}><Icon n="plus" /></button>
            <AnimatePresence>
              {plus && (
                <motion.div className="menu float" role="menu" initial={{ opacity: 0, y: -4, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.14 }}>
                  {WARM.map((w) => <button key={w.name} role="menuitem" onClick={() => addAgent(w.name)}><span>{w.name}</span><span className="label3">{w.ready ? "ready" : "cold"}</span></button>)}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
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
                  <DotMatrix state={a.state} />
                  <span className="name">{a.name}</span>
                  <span className="label3 grow ellipsis">{a.project}</span>
                  <span className="label3 tnum">{a.spawning ? "" : fmt(a.sec + (a.state === "done" || a.state === "idle" ? 0 : tick))}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="side-bottom">
          <div className="foot">
            <Link href="/settings" className="row"><Icon n="gear" size={14} /><span>Settings</span><span className="sp" /><kbd className="cap">⌘,</kbd></Link>
            <ThemeToggle />
          </div>
          <div className="conn"><span className="dot-ok" />Bridge · 127.0.0.1:17321</div>
          <div className="example pad">Example data</div>
        </div>
      </aside>

      <main className="main">
        {cur ? (
          <PaneView key={cur.id} agent={cur} act={actions(cur)} onCommand={() => setPal(true)} />
        ) : (
          <section className="inbox">
            <header className="topbar"><b className="t-name">Inbox</b><span className="sp" />
              <button className="btn cmdbtn" onClick={() => setPal(true)}>Commands <kbd className="cap">⌘K</kbd></button></header>
            {waiting.length === 0 ? (
              <div className="allclear dotgrid">
                <DotMatrix state="thinking" size={5} px={40} />
                <div className="doto big">All clear</div>
                <p className="label3">Agents will show up here when they need an answer.</p>
              </div>
            ) : (
              <div className="inbox-body">
                <div className="hero">
                  <span className="doto huge" aria-label={`${waiting.length} waiting`}>{count}</span>
                  <div className="herosub">
                    <Dynamic words={["waiting on you", `longest wait ${fmt(oldest)}`, `${agents.filter((a) => group(a) === "Working").length} agents still working`]} />
                  </div>
                </div>
                <AnimatePresence initial={false}>
                  {waiting.map((a) => (
                    <motion.div key={a.id} className="irow" layout exit={{ opacity: 0, height: 0 }} transition={{ duration: 0.2 }}>
                      <div className="grow">
                        <button className="link" onClick={() => setSel(a.id)}><b>{a.name}</b> <span className="label3">{a.project} · <span className="tnum">{fmt(a.sec + tick)}</span></span></button>
                        <div className="q">{a.question!.text}</div>
                        <div className="mono tail">{a.lines.slice(-2).map((l, i) => <div key={i}>{l.s}</div>)}</div>
                      </div>
                      <div className="seg">{a.question!.answers.map((x, i) => <button key={x} className={i === 0 ? "primary" : ""} onClick={() => answer(a, x)}>{x}</button>)}</div>
                    </motion.div>
                  ))}
                </AnimatePresence>
              </div>
            )}
          </section>
        )}
      </main>

      <button className="btn demo-tray" onClick={() => setTray(true)}>Tray</button>
      <div className="toasts" aria-live="polite">
        <AnimatePresence>
          {toasts.map((t) => (
            <motion.div key={t.id} className="toast float" layout initial={{ opacity: 0, y: 12, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: 8 }} transition={{ duration: 0.2 }}>
              <span>{t.msg}</span>
              {t.undo && <button className="plain" onClick={() => { t.undo!(); setToasts((x) => x.filter((y) => y.id !== t.id)); }}>Undo</button>}
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
      {pal && <Palette cmds={cmds} onClose={() => setPal(false)} />}
      {tray && <Tray agents={agents} onClose={() => setTray(false)} onPick={setSel} onSettings={() => { window.location.href = "/settings"; }} />}
    </div>
  );
}
