"use client";
import { useEffect, useState } from "react";
import { DotMatrix } from "./DotMatrix";
import { SPAWN_LOG } from "@/lib/mock";

export function Spawn({ name, onReady }: { name: string; onReady: () => void }) {
  const [n, setN] = useState(1);
  useEffect(() => {
    const t = SPAWN_LOG.map((_, i) => setTimeout(() => setN(i + 1), i * 600));
    const d = setTimeout(onReady, 2500);
    return () => { t.forEach(clearTimeout); clearTimeout(d); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="spawn dotgrid">
      <DotMatrix state="running" size={5} px={40} />
      <div className="label2">Starting {name}</div>
      <div className="spawnlog mono" aria-live="polite">
        <div style={{ transform: `translateY(${20 - (n - 1) * 20}px)` }}>
          {SPAWN_LOG.map((l, i) => <div key={l} className={i === n - 1 ? "cur" : ""}>{l}</div>)}
        </div>
      </div>
    </div>
  );
}
