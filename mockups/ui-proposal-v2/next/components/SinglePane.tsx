"use client";
import { useState } from "react";
import { AGENTS, Agent } from "@/lib/mock";
import { PaneView } from "./PaneView";

export function SinglePane() {
  const [a, setA] = useState<Agent | null>(AGENTS[0]);
  if (!a) return <div className="empty-win label3">Pane closed.</div>;
  return (
    <div className="solo">
      <PaneView agent={a} act={{
        answer: (x) => setA({ ...a, state: "running", question: undefined, lines: [...a.lines, { k: "user", s: `› ${x}` }] }),
        usePhone: () => setA({ ...a, phone: false }),
        interrupt: () => setA({ ...a, state: "idle", question: undefined }),
        close: () => setA(null),
        send: (t) => setA({ ...a, state: "running", lines: [...a.lines, { k: "user", s: `› ${t}` }] }),
      }} />
    </div>
  );
}
