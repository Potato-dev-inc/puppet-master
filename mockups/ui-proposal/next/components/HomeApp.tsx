"use client";
import Link from "next/link";
import { useState } from "react";
import { ExampleBadge } from "./ExampleBadge";
import { StateChip } from "./StateChip";
import { TrayMenu } from "./TrayMenu";
import { PANES, Pane } from "@/lib/mock";
import s from "./home.module.css";

const RANK = { needs: 0, error: 1, thinking: 2, tool: 3, running: 4, idle: 5 } as const;

export function HomeApp() {
  const [panes, setPanes] = useState<Pane[]>(PANES);
  const [log, setLog] = useState("");
  const sorted = [...panes].sort((a, b) => RANK[a.state] - RANK[b.state]);
  const need = panes.filter((p) => p.state === "needs").length;

  const answer = (p: Pane, a: string) => {
    setPanes((ps) => ps.map((x) => (x.id === p.id ? { ...x, state: "running", prompt: undefined, since: "now" } : x)));
    setLog(`Sent "${a}" to ${p.agent} · ${p.project}`);
  };

  return (
    <div className={s.wrap}>
      <main className={s.main}>
        <Link href="/" className="back">Index</Link>
        <div className={s.top}>
          <h1>{need > 0 ? `${need} need${need === 1 ? "s" : ""} you` : "Nothing needs you"}</h1>
          <ExampleBadge />
        </div>
        <p className={s.sub}>{panes.length} panes, ranked by who is waiting longest on you.</p>
        {log && <div className={`glass ${s.log}`} role="status">{log}</div>}
        <ul className={s.list}>
          {sorted.map((p) => (
            <li key={p.id} className={`glass ${s.row}`} data-state={p.state}>
              <div className={s.head}>
                <b>{p.agent}</b><span className={s.proj}>{p.project}</span>
                <StateChip state={p.state} since={p.since} />
                <span className={s.sp} />
                <button className="cap sm">Open pane</button>
              </div>
              <pre className={`mono ${s.tail}`}>{p.lines.join("\n")}</pre>
              {p.prompt && (
                <div className={s.ans}>
                  <span>{p.prompt.question}</span>
                  {p.prompt.answers.map((a, i) => (
                    <button key={a} className={`cap sm ${i === 0 ? "flag" : ""}`} onClick={() => answer(p, a)}>{a}</button>
                  ))}
                </div>
              )}
            </li>
          ))}
        </ul>
      </main>
      <aside className={s.tray}>
        <p className={s.cap}>Tray menu (right-click)</p>
        <TrayMenu panes={panes} />
      </aside>
    </div>
  );
}
