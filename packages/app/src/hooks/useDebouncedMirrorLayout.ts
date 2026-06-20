import { useEffect, useState } from 'react';

const DEFAULT_MIRROR_LAYOUT_DEBOUNCE_MS = 300;

/** Debounce PTY cols/rows before bumping mirror sessionKey (avoids remount storms). */
export function useDebouncedMirrorLayout(
  cols: number,
  rows: number,
  delayMs = DEFAULT_MIRROR_LAYOUT_DEBOUNCE_MS,
): { cols: number; rows: number } {
  const [debounced, setDebounced] = useState({ cols, rows });

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced({ cols, rows });
    }, delayMs);
    return () => window.clearTimeout(timer);
  }, [cols, rows, delayMs]);

  return debounced;
}
