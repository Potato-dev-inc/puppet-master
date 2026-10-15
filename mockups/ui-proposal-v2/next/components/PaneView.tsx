"use client";
import { useEffect, useRef, useState } from "react";
import { Agent, STATE_TEXT } from "@/lib/mock";
import { Glyph } from "./Glyph";
import { Icon } from "./Icon";

export type PaneActions = {
  answer: (a: string) => void;
  usePhone: () => void;
  interrupt: () => void;
  close: () => void;
  popOut?: () => void;
  send: (text: string) => void;
};

const running = (a: Agent) => a.state === "running" || a.state === "tool" || a.state === "thinking" || a.state === "needs";

export function PaneView({ agent, act }: { agent: Agent; act: PaneActions }) {
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [note, setNote] = useState("");
  const [text, setText] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const flash = (m: string) => { setNote(m); setTimeout(() => setNote(""), 1400); };

  useEffect(() => {
    if (!menu) return;
    const h = (e: MouseEvent) => { if (!menuRef.current?.contains(e.target as Node)) setMenu(false); };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [menu]);

  const items: [string, string, () => void, boolean?][] = [
    ["Reflow to window", "", () => flash("Reflowed")],
    ["Copy output", "⌘C", () => flash("Copied")],
    ["Find", "⌘F", () => flash("Find opened")],
    ["Pop out", "⇧⌘O", () => (act.popOut ? act.popOut() : flash("Already popped out"))],
    ["Interrupt", "⌃C", () => act.interrupt()],
    ["Close pane", "⌘W", () => (running(agent) ? setConfirm(true) : act.close())],
  ];

  return (
    <section className="paneview">
      <header className="topbar">
        <b className="t-name">{agent.name}</b>
        <span className="label3">{agent.project}</span>
        <span className="state"><Glyph s={agent.state} />{STATE_TEXT[agent.state]}</span>
        <span className="sp" />
        {note && <span className="label3">{note}</span>}
        <span className="label3 tnum">{agent.cols}×{agent.rows}</span>
        <div ref={menuRef} className="rel">
          <button className="icon-btn" aria-label="More" aria-expanded={menu} onClick={() => setMenu(!menu)}><Icon n="more" /></button>
          {menu && (
            <div className="menu float" role="menu">
              {items.map(([l, k, f, ], i) => (
                <div key={l}>
                  {i === 4 && <hr />}
                  <button role="menuitem" onClick={() => { setMenu(false); f(); }}>
                    <span>{l}</span><kbd>{k}</kbd>
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </header>

      {agent.phone && (
        <div className="inline-row">
          <span>Sized for your phone, {agent.cols}×{agent.rows} here.</span>
          <button className="btn" onClick={act.usePhone}>Use here</button>
        </div>
      )}

      <div className="term mono" tabIndex={0} aria-label="Terminal">
        {agent.lines.map((l, i) => <div key={i} className={`tl ${l.k}`}>{l.k === "bullet" ? "● " : ""}{l.s}</div>)}
        {!agent.question && <div className="tl prompt">{agent.state === "idle" ? "" : ""}<span className="caret" /></div>}
      </div>

      {agent.question && (
        <div className="qrow">
          <span className="q">{agent.question.text}</span>
          <div className="seg" role="group" aria-label="Answers">
            {agent.question.answers.map((a, i) => (
              <button key={a} className={i === 0 ? "primary" : ""} onClick={() => act.answer(a)}>{a}</button>
            ))}
          </div>
        </div>
      )}

      <form className="composer" onSubmit={(e) => { e.preventDefault(); if (text.trim()) { act.send(text.trim()); setText(""); } }}>
        <input value={text} onChange={(e) => setText(e.target.value)} placeholder={`Reply to ${agent.name}…`} aria-label="Reply" />
        <button className="send" aria-label="Send" disabled={!text.trim()}><Icon n="up" size={14} /></button>
      </form>
      <div className="example">Example data</div>

      {confirm && (
        <div className="scrim" onClick={() => setConfirm(false)}>
          <div className="sheet float" role="alertdialog" onClick={(e) => e.stopPropagation()}>
            <h3>Close this pane?</h3>
            <p>{agent.name} is still working in {agent.project}. Closing stops it.</p>
            <div className="sheet-actions">
              <button className="btn" autoFocus onClick={() => setConfirm(false)}>Keep running</button>
              <button className="btn primary" onClick={() => { setConfirm(false); act.close(); }}>Close pane</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
