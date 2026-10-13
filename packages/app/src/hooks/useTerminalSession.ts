import { useCallback, useEffect, useRef, type RefObject } from 'react';
import { tauri } from '../lib/tauri';
import { TerminalSession } from '../terminal';
import type { TerminalRenderMode } from '../terminal';
import type { PaneDataSubscriber } from '../terminal/types';
import { isKeyboardLikelyOpen } from '../lib/layout-viewport';

export interface TerminalTransport {
  resize: (cols: number, rows: number) => void | Promise<void>;
  writeInput: (text: string, appendNewline?: boolean) => void | Promise<void>;
}

interface UseTerminalSessionOptions {
  paneId: string;
  /** Bump when the pane process is replaced (agent switch). */
  sessionKey: number;
  subscribePaneData: PaneDataSubscriber;
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
  /** Bump `n` to redraw a mirror from the PC's current screen (phone nudge after an action). */
  refreshSignal?: { n: number; skipIfScrolledUp: boolean };
  /** Owner window: the PTY size a phone holds right now (null when no phone holds the pane). */
  heldPtySize?: { cols: number; rows: number } | null;
  /**
   * Called with the grid that fits the terminal's on-screen area, on mount and when that area
   * changes size (debounced, duplicates dropped). Used by the phone to size the pane to itself.
   */
  onFitGrid?: (grid: { cols: number; rows: number }) => void;
  /**
   * With `onFitGrid`: false keeps the reported rows while the on-screen keyboard is open (the PTY
   * keeps its height; the terminal is cropped instead of redrawn). Columns still follow.
   * True reports the measured grid as is. Only the phone sizes the pane with this.
   */
  keyboardResizesAgent?: boolean;
  /** The agent reprints its conversation on every resize: wipe scrollback at each resize (PaneInfo flag). */
  reprintsOnResize?: boolean;
  /** Bump to rebuild the session in place: the same steps as a fresh mount (the desktop nudge). */
  remountEpoch?: number;
  /** With `remountEpoch` > 0: the PTY's grid right now, so the remount's first fit sends nothing if it matches. */
  assumePtyGrid?: { cols: number; rows: number };
}

/** Before the first successful measurement (xterm still settling after mount). */
const FIT_GRID_DEBOUNCE_MS = 250;
/** After the first measurement: the grid is already local, so report changes quickly. */
const FIT_GRID_RESIZE_DEBOUNCE_MS = 50;
const FIT_GRID_FIRST_TRIES = 20;
/**
 * Rows lost (since the last closed-keyboard measurement) that count as an open keyboard. Covers
 * Android `interactive-widget=resizes-content`, where neither innerHeight nor the visual viewport
 * shows the keyboard (about 100 px at phone cell sizes).
 */
const KEYBOARD_ROWS_MIN = 5;

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
  refreshSignal,
  heldPtySize,
  onFitGrid,
  keyboardResizesAgent = true,
  reprintsOnResize = false,
  remountEpoch = 0,
  assumePtyGrid,
}: UseTerminalSessionOptions): {
  containerRef: RefObject<HTMLDivElement>;
  nudgeReflow: () => void;
  afterReclaim: () => void;
  isScrolledUp: () => boolean;
  getSelection: () => string;
} {
  const containerRef = useRef<HTMLDivElement>(null);
  const sessionRef = useRef<TerminalSession | null>(null);
  const sizesPane = !!onFitGrid;
  const assumePtyGridRef = useRef(assumePtyGrid);
  assumePtyGridRef.current = assumePtyGrid;
  const nudgeReflow = useCallback(() => {
    sessionRef.current?.nudgeReflow();
  }, []);

  const isScrolledUp = useCallback(() => sessionRef.current?.isScrolledUp() ?? false, []);

  const afterReclaim = useCallback(() => {
    sessionRef.current?.afterReclaim();
  }, []);

  const getSelection = useCallback(() => sessionRef.current?.getSelection() ?? '', []);

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
      reprintsOnResize,
      assumePtyGrid: remountEpoch > 0 ? assumePtyGridRef.current : undefined,
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
  }, [paneId, sessionKey, subscribePaneData, transport, syncPTYResize, renderMode, mobileInputDelayMs, mobileInputVisible, disableMobileInput, sizesPane, remountEpoch]);

  // The flag arrives with the pane list, possibly after mount: no remount, only the setting.
  useEffect(() => {
    sessionRef.current?.setReprintsOnResize(reprintsOnResize);
  }, [reprintsOnResize, paneId, sessionKey]);

  // A window opened while a phone already holds the pane gets no resize event: follow the held size.
  const heldCols = heldPtySize?.cols;
  const heldRows = heldPtySize?.rows;
  useEffect(() => {
    if (heldCols === undefined || heldRows === undefined) return;
    sessionRef.current?.followHeldPty(heldCols, heldRows);
  }, [heldCols, heldRows, paneId, sessionKey]);

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

  const refreshN = refreshSignal?.n ?? 0;
  const refreshSkipRef = useRef(false);
  refreshSkipRef.current = refreshSignal?.skipIfScrolledUp ?? false;
  useEffect(() => {
    if (!refreshN) return;
    sessionRef.current?.refreshFromSource({ skipIfScrolledUp: refreshSkipRef.current });
  }, [refreshN]);

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
    // Rows of the last measurement taken with the keyboard closed, for the current columns.
    let closedCols = 0;
    let closedRows: number | null = null;
    const report = (): boolean => {
      const measured = sessionRef.current?.measureFitGrid();
      if (!measured) return false;
      measuredOnce = true;
      tries = 0;
      let grid = measured;
      if (!keyboardResizesAgent) {
        if (closedCols !== measured.cols) {
          closedCols = measured.cols;
          closedRows = null;
        }
        const keyboardOpen =
          isKeyboardLikelyOpen() ||
          (closedRows !== null && measured.rows <= closedRows - KEYBOARD_ROWS_MIN);
        if (keyboardOpen) {
          grid = { cols: measured.cols, rows: closedRows ?? measured.rows };
        } else {
          closedRows = measured.rows;
        }
      }
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
  }, [wantsFitGrid, keyboardResizesAgent, paneId, sessionKey]);

  return { containerRef: containerRef as RefObject<HTMLDivElement>, nudgeReflow, afterReclaim, isScrolledUp, getSelection };
}
