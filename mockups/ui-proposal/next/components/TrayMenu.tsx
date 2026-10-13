"use client";
import { useState } from "react";
import { Confirm } from "./Confirm";
import { Pane, STATE_META, WARM_AGENTS } from "@/lib/mock";
import s from "./home.module.css";

export function TrayMenu({ panes }: { panes: Pane[] }) {
  const [open, setOpen] = useState<"new" | "panes" | null>("new");
  const [quit, setQuit] = useState(false);
  const [done, setDone] = useState(false);
  const need = panes.filter((p) => p.state === "needs").length;

  return (
    <div className={s.menuWrap}>
      <div className={`glass ${s.menu}`} role="menu">
        <div className={s.menuHead}>
          <span className={s.badge} style={{ background: need ? "var(--flag-yellow)" : "var(--fill)", color: need ? "var(--ink-on-flag)" : "var(--ink-3)" }}>{need}</span>
          {need ? `${need} waiting for you` : "All quiet"}
        </div>
        <button role="menuitem" className={s.mi} onClick={() => setOpen(open === "new" ? null : "new")}>New pane <span>▸</span></button>
        {open === "new" && WARM_AGENTS.map((a) => (
          <button key={a.name} role="menuitem" className={s.sub}>{a.name}{a.ready && <em>(ready)</em>}</button>
        ))}
        <button role="menuitem" className={s.mi} onClick={() => setOpen(open === "panes" ? null : "panes")}>Panes <span>▸</span></button>
        {open === "panes" && panes.map((p) => (
          <button key={p.id} role="menuitem" className={s.sub}>
            <i style={{ background: STATE_META[p.state].color }} />{p.agent} · {p.project}
          </button>
        ))}
        <hr />
        <button role="menuitem" className={s.mi}>Settings…</button>
        <button role="menuitem" className={s.mi} onClick={() => setQuit(true)}>Quit Puppet Master</button>
      </div>
      {done && <p className={s.note}>Quit confirmed (mock only).</p>}
      {quit && (
        <Confirm danger title="Quit Puppet Master?"
          body={`${panes.filter((p) => p.state !== "idle").length} panes are active and will be closed.`}
          confirmLabel="Quit and close panes" cancelLabel="Stay"
          onCancel={() => setQuit(false)} onConfirm={() => { setQuit(false); setDone(true); }} />
      )}
    </div>
  );
}
