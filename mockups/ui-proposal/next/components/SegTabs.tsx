"use client";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { useId, type ReactNode } from "react";

export function SegTabs<T extends string>({ value, options, onChange, label }: { value: T; options: readonly T[]; onChange: (v: T) => void; label: string }) {
  const id = useId();
  return (
    <LayoutGroup id={id}>
      <div className="segtabs" role="tablist" aria-label={label}>
        {options.map((o) => (
          <button key={o} role="tab" aria-selected={o === value} className={o === value ? "on" : ""} onClick={() => onChange(o)}>
            {o === value && <motion.span layoutId="pill" className="pill" transition={{ type: "spring", stiffness: 400, damping: 32 }} />}
            <span className="z">{o}</span>
          </button>
        ))}
      </div>
    </LayoutGroup>
  );
}

export function TabContent({ k, children }: { k: string; children: ReactNode }) {
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div key={k} className="tabcontent"
        initial={{ opacity: 0, y: 8, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
        exit={{ opacity: 0, y: -4, filter: "blur(4px)" }} transition={{ duration: 0.18 }}>
        {children}
      </motion.div>
    </AnimatePresence>
  );
}
