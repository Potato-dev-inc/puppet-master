"use client";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";

const WORDS = ["Thinking", "Reading files", "Planning"];

export function ThinkingLabel() {
  const [i, setI] = useState(0);
  useEffect(() => { const t = setInterval(() => setI((x) => (x + 1) % WORDS.length), 2200); return () => clearInterval(t); }, []);
  return (
    <span className="think-wrap">
      <AnimatePresence mode="wait" initial={false}>
        <motion.span key={i} className="shimmer" initial={{ opacity: 0, y: 6, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
          exit={{ opacity: 0, y: -6, filter: "blur(4px)" }} transition={{ duration: 0.22 }}>{WORDS[i]}</motion.span>
      </AnimatePresence>
    </span>
  );
}
