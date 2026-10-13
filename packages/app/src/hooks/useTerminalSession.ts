import { useCallback, useEffect, useRef, type RefObject } from 'react';
import { tauri } from '../lib/tauri';
import { TerminalSession } from '../terminal';
import type { PaneSnapshotListener, TerminalRenderMode } from '../terminal';

export interface TerminalTransport {
  resize: (cols: number, rows: number) => void | Promise<void>;
  writeInput: (text: string, appendNewline?: boolean) => void | Promise<void>;
}

interface UseTerminalSessionOptions {
  paneId: string;
  /** Bump when the pane process is replaced (agent switch). */
  sessionKey: number;
  subscribePaneData: (paneId: string, cb: PaneSnapshotListener) => () => void;
  transport?: TerminalTransport;
  syncPTYResize?: boolean;
  renderMode?: TerminalRenderMode;
  ptyCols?: number;
  ptyRows?: number;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
  disableMobileInput?: boolean;
  /** Bump to force a tiny resize/repaint (e.g. when re-entering the grid). */
  reflowKey?: number | string;
  /**
   * Called with the grid that fits the terminal's on-screen area, on mount and when that area
   * changes size (debounced, duplicates dropped). Used by the phone to size the pane to itself.
   */
  onFitGrid?: (grid: { cols: number; rows: number }) => void;
}

/** Before the first successful measurement (xterm still settling after mount). */
const FIT_GRID_DEBOUNCE_MS = 250;
/** After the first measurement: the grid is already local, so report changes quickly. */
const FIT_GRID_RESIZE_DEBOUNCE_MS = 50;
const FIT_GRID_FIRST_TRIES = 20;

/**
 * React lifecycle wrapper around {@link TerminalSession}.
 */
export function useTerminalSession({
  paneId,
  sessionKey,
  subscribePaneData,
  transport,
  syncPTYResize = true,
  renderMode,
  ptyCols,
  ptyRows,
  mobileInputDelayMs,
  mobileInputVisible,
  disableMobileInput,
  reflowKey,
  onFitGrid,
}: UseTerminalSessionOptions): {
  containerRef: RefObject<HTMLDivElement>;
  nudgeReflow: () => void;
} {
  const containerRef = useRef<HTMLDivElement>(null);
  const sessionRef = useRef<TerminalSession | null>(null);
  const sizesPane = !!onFitGrid;
  const nudgeReflow = useCallback(() => {
    sessionRef.current?.nudgeReflow();
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const defaultTransport: TerminalTransport = {
      resize: (cols: number, rows: number) => {
        void tauri.resize(paneId, cols, rows);
      },
      writeInput: (text: string, appendNewline = false) => {
        void tauri.writeInput(paneId, text, appendNewline);
      },
    };
    const activeTransport = transport ?? defaultTransport;

    const session = new TerminalSession({
      paneId,
      syncPTYResize,
      renderMode,
      ptyCols,
      ptyRows,
      mobileInputDelayMs,
      mobileInputVisible,
      disableMobileInput,
      sizesPane,
      onResize: (cols, rows) => {
        void activeTransport.resize(cols, rows);
      },
      onInput: (text, appendNewline = false) => {
        void activeTransport.writeInput(text, appendNewline);
      },
    });
    sessionRef.current = session;
    session.mount(container, subscribePaneData);

    return () => {
      session.dispose();
      sessionRef.current = null;
    };
  }, [paneId, sessionKey, subscribePaneData, transport, syncPTYResize, renderMode, mobileInputDelayMs, mobileInputVisible, disableMobileInput, sizesPane]);

  useEffect(() => {
    if (syncPTYResize) return;
    if (ptyCols === undefined || ptyRows === undefined) return;
    sessionRef.current?.setPtyDimensions(ptyCols, ptyRows);
  }, [syncPTYResize, ptyCols, ptyRows]);

  useEffect(() => {
    if (reflowKey === undefined) return;
    // Defer until xterm is open — a pane returning from a detached window remounts
    // the session in the same tick as this effect, so a single rAF is often too early.
    let cancelled = false;
    let frame = 0;
    let attempts = 0;
    const tryNudge = () => {
      if (cancelled) return;
      const didNudge = sessionRef.current?.nudgeReflow() ?? false;
      if (!didNudge && ++attempts < 10) {
        frame = requestAnimationFrame(tryNudge);
      }
    };
    frame = requestAnimationFrame(tryNudge);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [reflowKey]);

  const onFitGridRef = useRef(onFitGrid);
  onFitGridRef.current = onFitGrid;
  const wantsFitGrid = !!onFitGrid;
  useEffect(() => {
    const container = containerRef.current;
    if (!wantsFitGrid || !container) return;
    let last = '';
    let timer = 0;
    let tries = 0;
    let measuredOnce = false;
    const report = (): boolean => {
      const grid = sessionRef.current?.measureFitGrid();
      if (!grid) return false;
      measuredOnce = true;
      tries = 0;
      const key = `${grid.cols}x${grid.rows}`;
      if (key !== last) {
        last = key;
        onFitGridRef.current?.(grid);
      }
      return true;
    };
    const schedule = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        // xterm measures its cells a frame or two after mount: retry until it has.
        if (!report() && ++tries < FIT_GRID_FIRST_TRIES) schedule();
      }, measuredOnce ? FIT_GRID_RESIZE_DEBOUNCE_MS : FIT_GRID_DEBOUNCE_MS);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(container);
    schedule();
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [wantsFitGrid, paneId, sessionKey]);

  return { containerRef: containerRef as RefObject<HTMLDivElement>, nudgeReflow };
}
