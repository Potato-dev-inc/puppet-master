"use client";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { Agent, STATE_TEXT } from "@/lib/mock";
import { Composer, Mode } from "./Composer";
import { DotMatrix } from "./DotMatrix";
import { Icon } from "./Icon";
import { SegTabs, TabContent } from "./SegTabs";
import * as sp from "./Spawn";
import { ThinkingLabel } from "./ThinkingLabel";

export type PaneActions = {
  answer: (a: string) => void;
  usePhone: () => void;
  interrupt: () => void;
  close: () => void;
  popOut?: () => void;
  send: (text: string, mode: Mode) => void;
  ready?: () => void;
  toast?: (m: string) => void;
};

const TABS = ["Terminal", "Transcript", "Receipt"] as const;
type Tab = (typeof TABS)[number];
const busy = (a: Agent) => ["running", "tool", "thinking", "needs", "writing"].includes(a.state);

export function PaneView({ agent, act, onCommand }: { agent: Agent; act: PaneActions; onCommand?: () => void }) {
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [note, setNote] = useState("");
  const [tab, setTab] = useState<Tab>("Terminal");
  const menuRef = useRef<HTMLDivElement>(null);
  const flash = (m: string) => { if (act.toast) act.toast(m); else { setNote(m); setTimeout(() => setNote(""), 1400); } };

  useEffect(() => {
    if (!menu) return;
    const h = (e: MouseEvent) => { if (!menuRef.current?.contains(e.target as Node)) setMenu(false); };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [menu]);

  const items: [string, string, () => void][] = [
    ["Reflow to window", "", () => flash("Reflowed")],
    ["Copy output", "⌘C", () => flash("Copied")],
    ["Find", "⌘F", () => flash("Find opened")],
    ["Pop out", "⇧⌘O", () => (act.popOut ? act.popOut() : flash("Already popped out"))],
    ["Interrupt", "⌃C", () => act.interrupt()],
    ["Close pane", "⌘W", () => (busy(agent) ? setConfirm(true) : act.close())],
  ];

  if (agent.spawning) {
    return (
      <section className="paneview">
        <header className="topbar"><b className="t-name">{agent.name}</b><span className="label3">{agent.project}</span><span className="state"><DotMatrix state="running" />Starting</span></header>
        <sp.Spawn name={agent.name} onReady={() => act.ready?.()} />
      </section>
    );
  }

  return (
    <section className="paneview">
      <header className="topbar">
        <b className="t-name">{agent.name}</b>
        <span className="label3">{agent.project}</span>
        <span className="state"><DotMatrix state={agent.state} />{agent.state === "thinking" ? <ThinkingLabel /> : STATE_TEXT[agent.state]}</span>
        <span className="sp" />
        {note && <span className="label3">{note}</span>}
        <span className="label3 tnum">{agent.cols}×{agent.rows}</span>
        {onCommand && <button className="btn cmdbtn" onClick={onCommand}>Commands <kbd className="cap">⌘K</kbd></button>}
        <div ref={menuRef} className="rel">
          <button className="icon-btn" aria-label="More" aria-expanded={menu} onClick={() => setMenu(!menu)}><Icon n="more" /></button>
          <AnimatePresence>
            {menu && (
              <motion.div className="menu float" role="menu" initial={{ opacity: 0, y: -4, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.14 }}>
                {items.map(([l, k, f], i) => (
                  <div key={l}>
                    {i === 4 && <hr />}
                    <button role="menuitem" onClick={() => { setMenu(false); f(); }}><span>{l}</span>{k && <kbd className="cap">{k}</kbd>}</button>
                  </div>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </header>

      <div className="tabsbar"><SegTabs value={tab} options={TABS} onChange={setTab} label="Pane view" /></div>

      {agent.phone && (
        <div className="inline-row">
          <span>Sized for your phone, {agent.cols}×{agent.rows} here.</span>
          <button className="btn" onClick={act.usePhone}>Use here</button>
        </div>
      )}

      <div className="viewarea">
        <TabContent k={tab}>
          {tab === "Terminal" && (
            <div className="term mono" tabIndex={0} aria-label="Terminal">
              {agent.lines.map((l, i) => <div key={i} className={`tl ${l.k}`}>{l.k === "bullet" ? "● " : ""}{l.s}</div>)}
              {!agent.question && <div className="tl prompt"><span className="caret" /></div>}
            </div>
          )}
          {tab === "Transcript" && (
            <div className="term transcript">
              {agent.lines.filter((l) => l.k === "bullet" || l.k === "user").map((l, i) => (
                <div key={i} className="turn"><span className="label3">{l.k === "user" ? "You" : agent.name}</span><div>{l.s.replace(/^› /, "")}</div></div>
              ))}
            </div>
          )}
          {tab === "Receipt" && (
            <div className="term receipt">
              {([
                ["Finished", agent.state === "done" || agent.state === "idle" ? "Yes" : "No"],
                ["Checks passed", agent.state === "error" ? "2 of 4" : "3 of 4"],
                ["Files touched", "3"],
              ] as const).map(([k, v]) => <div key={k} className="rrow"><span className="label2">{k}</span><span className="tnum">{v}</span></div>)}
              <div className="caption" style={{ padding: "16px 0 4px" }}>Files</div>
              {["src-tauri/src/tray.rs", "src/tray.test.rs", "docs/UI-INVENTORY.md"].map((f) => <div key={f} className="rrow mono"><span>{f}</span><span className="label3">edited</span></div>)}
            </div>
          )}
        </TabContent>
      </div>

      <AnimatePresence>
        {agent.question && (
          <motion.div className="qrow" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 8 }} transition={{ duration: 0.2 }}>
            <span className="q">{agent.question.text}</span>
            <div className="seg" role="group" aria-label="Answers">
              {agent.question.answers.map((a, i) => <button key={a} className={i === 0 ? "primary" : ""} onClick={() => act.answer(a)}>{a}</button>)}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <Composer agentName={agent.name} onSend={act.send} />
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
