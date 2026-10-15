"use client";
import { motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "./Icon";

export type Cmd = { id: string; group: "Agents" | "Actions" | "Settings"; icon: string; label: string; desc: string; hint?: string; run: () => void };
const ORDER = ["Agents", "Actions", "Settings"] as const;

export function Palette({ cmds, onClose }: { cmds: Cmd[]; onClose: () => void }) {
  const [raw, setRaw] = useState("");
  const [q, setQ] = useState("");
  const [i, setI] = useState(0);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.focus(), []);
  useEffect(() => { const t = setTimeout(() => { setQ(raw); setI(0); }, 150); return () => clearTimeout(t); }, [raw]);

  const flat = useMemo(() => {
    const f = cmds.filter((c) => (c.label + " " + c.desc).toLowerCase().includes(q.toLowerCase()));
    return ORDER.flatMap((g) => f.filter((c) => c.group === g));
  }, [cmds, q]);
  const run = (c?: Cmd) => { if (c) { onClose(); c.run(); } };

  return (
    <div className="scrim top dotgrid" onClick={onClose}>
      <motion.div className="palette float" role="dialog" aria-label="Command palette" onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: -8, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={{ duration: 0.28, ease: [0.32, 0.72, 0, 1] }}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          if (e.key === "ArrowDown") { e.preventDefault(); setI((x) => Math.min(x + 1, flat.length - 1)); }
          if (e.key === "ArrowUp") { e.preventDefault(); setI((x) => Math.max(x - 1, 0)); }
          if (e.key === "Enter") run(flat[i]);
        }}>
        <div className="phead"><span className="doto">Commands</span><kbd className="cap">esc</kbd></div>
        <div className="psearch"><Icon n="search" size={14} /><input ref={ref} value={raw} onChange={(e) => setRaw(e.target.value)} placeholder="Search agents, actions, settings" aria-label="Command" /></div>
        <div className="plist" role="listbox">
          {flat.length === 0 && <div className="label3 pempty">No results</div>}
          {ORDER.map((g) => {
            const rows = flat.filter((c) => c.group === g);
            if (!rows.length) return null;
            return (
              <div key={g}>
                <div className="caption">{g}</div>
                {rows.map((c) => {
                  const n = flat.indexOf(c);
                  return (
                    <motion.button key={c.id} role="option" aria-selected={n === i} onMouseMove={() => setI(n)} onClick={() => run(c)}
                      initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: n * 0.02, duration: 0.18 }}>
                      {n === i && <motion.span layoutId="phl" className="hl" transition={{ type: "spring", stiffness: 500, damping: 40 }} />}
                      <span className="pico"><Icon n={c.icon} size={14} /></span>
                      <span className="ptitle">{c.label}</span>
                      <span className="label3 pdesc ellipsis">{c.desc}</span>
                      {c.hint && <kbd className="cap">{c.hint}</kbd>}
                    </motion.button>
                  );
                })}
              </div>
            );
          })}
        </div>
      </motion.div>
    </div>
  );
}
