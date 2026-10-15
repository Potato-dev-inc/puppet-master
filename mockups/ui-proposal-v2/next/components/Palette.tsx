"use client";
import { useEffect, useMemo, useRef, useState } from "react";

export type Cmd = { id: string; label: string; hint?: string; run: () => void };

export function Palette({ cmds, onClose }: { cmds: Cmd[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [i, setI] = useState(0);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.focus(), []);
  const list = useMemo(() => cmds.filter((c) => c.label.toLowerCase().includes(q.toLowerCase())), [cmds, q]);
  const run = (c?: Cmd) => { if (c) { onClose(); c.run(); } };

  return (
    <div className="scrim top" onClick={onClose}>
      <div className="palette float" role="dialog" aria-label="Command palette" onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          if (e.key === "ArrowDown") { e.preventDefault(); setI((x) => Math.min(x + 1, list.length - 1)); }
          if (e.key === "ArrowUp") { e.preventDefault(); setI((x) => Math.max(x - 1, 0)); }
          if (e.key === "Enter") run(list[i]);
        }}>
        <input ref={ref} value={q} onChange={(e) => { setQ(e.target.value); setI(0); }} placeholder="Type a command or agent name" aria-label="Command" />
        <div className="plist" role="listbox">
          {list.length === 0 && <div className="label3 pempty">No results</div>}
          {list.map((c, n) => (
            <button key={c.id} role="option" aria-selected={n === i} className={n === i ? "sel" : ""} onMouseMove={() => setI(n)} onClick={() => run(c)}>
              <span>{c.label}</span><span className="label3">{c.hint}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
