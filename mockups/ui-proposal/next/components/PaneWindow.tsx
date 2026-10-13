"use client";
import Link from "next/link";
import { useState } from "react";
import { Confirm } from "./Confirm";
import { ExampleBadge } from "./ExampleBadge";
import { StateChip } from "./StateChip";
import { PaneState, STATE_META, TERMINAL_FEED } from "@/lib/mock";
import s from "./pane.module.css";

const STATES: PaneState[] = ["needs", "thinking", "tool", "running", "idle", "error"];

export function PaneWindow() {
  const [state, setState] = useState<PaneState>("needs");
  const [phone, setPhone] = useState(true);
  const [closing, setClosing] = useState(false);
  const [note, setNote] = useState("");
  const running = state !== "idle" && state !== "error";
  const cols = phone ? 46 : 120, rows = phone ? 38 : 36;
  const flash = (m: string) => { setNote(m); setTimeout(() => setNote(""), 1600); };

  const act = [
    ["Reflow", () => flash("Reflowed to window")],
    ["Copy", () => flash("Copied last output")],
    ["Search", () => flash("Search opened")],
  ] as const;

  return (
    <div className={s.win}>
      <div className={s.frame} data-state={state}>
        <header className={`glass ${s.strip}`}>
          <b className={s.agent}>Claude Code</b>
          <span className={s.proj}>puppet-master</span>
          <StateChip state={state} since="2m" />
          <span className={`mono ${s.size}`}>{cols}×{rows}</span>
          <span className={s.sp} />
          {act.map(([l, f]) => <button key={l} className="cap sm" onClick={f}>{l}</button>)}
          <button className="cap sm" disabled={!running} onClick={() => flash("Interrupt sent (Esc)")}>Interrupt</button>
          <button className="cap sm danger" onClick={() => (running ? setClosing(true) : flash("Pane closed"))}>Close</button>
        </header>

        {phone && (
          <div className={s.banner}>
            <span>
              <b>Sized for your phone.</b> This pane is {cols}×{rows} while your phone is connected, so the desktop view is narrow.
            </span>
            <button className="cap sm go" onClick={() => setPhone(false)}>Reclaim</button>
          </div>
        )}

        <pre className={`mono ${s.term}`} style={{ maxWidth: phone ? "46ch" : undefined }}>
          {TERMINAL_FEED.map((l, i) => <div key={i} className={s[l.t]}>{l.s}</div>)}
          <div className={s.cursor}>{state === "needs" ? "" : "> "}<i /></div>
        </pre>
        {note && <div className={`glass ${s.toast}`}>{note}</div>}
      </div>

      <aside className={s.demo}>
        <Link href="/" className="back">Index</Link>
        <ExampleBadge />
        <p>Preview a state</p>
        {STATES.map((x) => (
          <button key={x} className={`cap sm ${state === x ? "go" : ""}`} onClick={() => setState(x)} style={{ justifyContent: "flex-start" }}>
            <span style={{ width: 8, height: 8, borderRadius: 4, background: STATE_META[x].color }} />{STATE_META[x].label}
          </button>
        ))}
        <button className="cap sm" onClick={() => setPhone(true)}>Phone connects</button>
      </aside>

      {closing && (
        <Confirm danger title="Close a running pane?" confirmLabel="Close and kill process" cancelLabel="Keep running"
          body="Claude Code is still working in puppet-master. Closing will stop it and discard unsent input."
          onCancel={() => setClosing(false)} onConfirm={() => { setClosing(false); setState("idle"); flash("Pane closed"); }} />
      )}
    </div>
  );
}
