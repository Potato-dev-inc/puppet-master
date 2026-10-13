"use client";
import { useState } from "react";
import { Agent, WARM } from "@/lib/mock";
import { DotMatrix } from "./DotMatrix";
import { Sheet } from "./Sheet";

export function Tray({ agents, onClose, onPick, onSettings }: { agents: Agent[]; onClose: () => void; onPick: (id: string) => void; onSettings: () => void }) {
  const [sub, setSub] = useState<"new" | "panes" | null>("new");
  const [quit, setQuit] = useState(false);
  const need = agents.filter((a) => a.state === "needs").length;
  return (
    <div className="scrim clear" onClick={onClose}>
      <div className="tray float" role="menu" aria-label="Tray menu" onClick={(e) => e.stopPropagation()}>
        <div className="tray-head">{need ? `${need} need${need === 1 ? "s" : ""} you` : "Nothing needs you"}</div>
        <button role="menuitem" onClick={() => setSub(sub === "new" ? null : "new")}><span>New pane</span><kbd className="cap">▸</kbd></button>
        {sub === "new" && WARM.map((w) => (
          <button key={w.name} role="menuitem" className="subitem"><span>{w.name}</span><kbd className="cap">{w.ready ? "ready" : ""}</kbd></button>
        ))}
        <button role="menuitem" onClick={() => setSub(sub === "panes" ? null : "panes")}><span>Panes</span><kbd className="cap">▸</kbd></button>
        {sub === "panes" && agents.slice(0, 6).map((a) => (
          <button key={a.id} role="menuitem" className="subitem" onClick={() => { onPick(a.id); onClose(); }}>
            <DotMatrix state={a.state} /><span>{a.name} · {a.project}</span>
          </button>
        ))}
        <hr />
        <button role="menuitem" onClick={onSettings}><span>Settings…</span><kbd className="cap">⌘,</kbd></button>
        <button role="menuitem" onClick={() => setQuit(true)}><span>Quit Puppet Master</span><kbd className="cap">⌘Q</kbd></button>
      </div>
      {quit && <Sheet title="Quit Puppet Master?" body={`${agents.filter((a) => a.state !== "idle" && a.state !== "done").length} panes are active and will be closed.`} confirm="Quit" cancel="Cancel" onCancel={() => setQuit(false)} onConfirm={() => { setQuit(false); onClose(); }} />}
    </div>
  );
}
