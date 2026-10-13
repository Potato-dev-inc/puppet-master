"use client";
import { useCallback, useEffect, useState } from "react";

export type Theme = "light" | "dark";

export function useTheme(): [Theme, () => void] {
  const [t, setT] = useState<Theme>("dark");
  useEffect(() => {
    const d = document.documentElement.dataset.theme as Theme | undefined;
    setT(d ?? (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"));
  }, []);
  const toggle = useCallback(() => {
    const n: Theme = (document.documentElement.dataset.theme ?? t) === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = n;
    try { localStorage.setItem("pm-theme", n); } catch { /* ignore */ }
    setT(n);
  }, [t]);
  return [t, toggle];
}
