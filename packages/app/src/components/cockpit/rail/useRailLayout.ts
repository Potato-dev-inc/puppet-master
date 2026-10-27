// Whether the session rail is shown: the person's choice (remembered), the window being wide enough, and the
// `]` key. Storage can be blocked (private window), so every read and write is guarded.
import { useCallback, useEffect, useState } from 'react';

export const RAIL_KEY = 'pm.sessionRail.open';
/** Below this window width the rail is hidden whatever the person chose. */
export const RAIL_MIN_WIDTH = 1100;

function readOpen(): boolean {
  try {
    return window.localStorage.getItem(RAIL_KEY) !== '0';
  } catch {
    return true;
  }
}

function writeOpen(open: boolean): void {
  try {
    window.localStorage.setItem(RAIL_KEY, open ? '1' : '0');
  } catch {
    // The choice just is not remembered.
  }
}

/** A key press in a place where typing a character is the point. */
export function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== 'string') return false;
  return el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName);
}

export interface RailLayout {
  /** The person wants the rail. */
  open: boolean;
  /** The window is wide enough to hold it. */
  fits: boolean;
  /** Show it: open and fits. */
  visible: boolean;
  toggle: () => void;
}

export function useRailLayout(): RailLayout {
  const [open, setOpen] = useState(readOpen);
  const [width, setWidth] = useState(() => (typeof window === 'undefined' ? RAIL_MIN_WIDTH : window.innerWidth));
  const fits = width >= RAIL_MIN_WIDTH;

  const toggle = useCallback(() => {
    setOpen((was) => {
      writeOpen(!was);
      return !was;
    });
  }, []);

  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  useEffect(() => {
    if (!fits) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== ']' || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented || e.isComposing) return;
      if (isTypingTarget(e.target)) return;
      toggle();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [fits, toggle]);

  return { open, fits, visible: open && fits, toggle };
}
