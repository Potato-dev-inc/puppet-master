"use client";
import { AnimatePresence, motion } from "motion/react";
import { useTheme } from "./useTheme";

export function ThemeToggle({ label }: { label?: boolean }) {
  const [t, toggle] = useTheme();
  const dark = t === "dark";
  return (
    <button className={label ? "btn" : "icon-btn"} onClick={toggle} aria-label={dark ? "Switch to light" : "Switch to dark"} data-tip="Theme">
      <span className="theme-ico">
        <AnimatePresence mode="wait" initial={false}>
          <motion.svg key={t} width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"
            initial={{ rotate: -90, opacity: 0 }} animate={{ rotate: 0, opacity: 1 }} exit={{ rotate: 90, opacity: 0 }} transition={{ duration: 0.18 }}>
            {dark ? <path d="M13.5 9.5A5.5 5.5 0 1 1 6.5 2.5a4.5 4.5 0 0 0 7 7z" /> : <><circle cx="8" cy="8" r="2.8" /><path d="M8 1.5v1.5M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1 1M11.6 11.6l1 1M12.6 3.4l-1 1M4.4 11.6l-1 1" /></>}
          </motion.svg>
        </AnimatePresence>
      </span>
      {label && <span>{dark ? "Dark" : "Light"}</span>}
    </button>
  );
}
