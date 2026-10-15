"use client";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { MODELS } from "@/lib/mock";
import { Icon } from "./Icon";

export type Mode = "steer" | "followup";

export function Composer({ agentName, onSend }: { agentName: string; onSend: (text: string, mode: Mode) => void }) {
  const [text, setText] = useState("");
  const [mode, setMode] = useState<Mode>("steer");
  const [model, setModel] = useState(MODELS[0]);
  const [dd, setDd] = useState(false);
  const ta = useRef<HTMLTextAreaElement>(null);
  const ddRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ta.current; if (!el) return;
    el.style.height = "20px";
    el.style.height = Math.min(el.scrollHeight, 120) + "px";
  }, [text]);
  useEffect(() => {
    if (!dd) return;
    const h = (e: MouseEvent) => { if (!ddRef.current?.contains(e.target as Node)) setDd(false); };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [dd]);

  const submit = () => { if (text.trim()) { onSend(text.trim(), mode); setText(""); } };
  const has = text.trim().length > 0;

  return (
    <div className="composer2">
      <textarea ref={ta} rows={1} value={text} aria-label="Reply" onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); } }}
        placeholder={mode === "steer" ? "Steer the current turn…" : "Start the next turn…"} />
      <div className="cbar">
        <div ref={ddRef} className="rel">
          <button className="chipbtn" onClick={() => setDd(!dd)} aria-expanded={dd}>{model}<Icon n="chev" size={12} /></button>
          <AnimatePresence>
            {dd && (
              <motion.div className="menu float up" initial={{ opacity: 0, y: 6, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: 4 }} transition={{ duration: 0.16 }}>
                {MODELS.map((m) => <button key={m} onClick={() => { setModel(m); setDd(false); }}><span>{m}</span>{m === model && <Icon n="check" size={14} />}</button>)}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
        <button className="icon-btn" aria-label="Attach file" data-tip="Attach"><Icon n="clip" /></button>
        <motion.button layout className={`steer ${mode === "followup" ? "on" : ""}`} aria-pressed={mode === "followup"}
          onClick={() => setMode(mode === "steer" ? "followup" : "steer")} transition={{ type: "spring", stiffness: 500, damping: 36 }} data-tip="send_message / followup_task">
          <motion.span className="sico" animate={{ rotate: mode === "followup" ? 90 : 0 }}><Icon n="steer" size={14} /></motion.span>
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.span key={mode} initial={{ opacity: 0, x: 6 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -6 }} transition={{ duration: 0.16 }}>
              {mode === "steer" ? "Steer" : "Follow-up"}
            </motion.span>
          </AnimatePresence>
        </motion.button>
        <span className="sp" />
        <span className="label3 hint">{agentName}</span>
        <button className={`send ${has ? "ready" : ""}`} aria-label="Send" disabled={!has} onClick={submit}><Icon n="up" size={14} /></button>
      </div>
    </div>
  );
}
