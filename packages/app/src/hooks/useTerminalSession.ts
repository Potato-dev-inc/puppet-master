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
}

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
}: UseTerminalSessionOptions): {
  containerRef: RefObject<HTMLDivElement>;
  nudgeReflow: () => void;
} {
  const containerRef = useRef<HTMLDivElement>(null);
  const sessionRef = useRef<TerminalSession | null>(null);
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
  }, [paneId, sessionKey, subscribePaneData, transport, syncPTYResize, renderMode, mobileInputDelayMs, mobileInputVisible, disableMobileInput]);

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

  return { containerRef: containerRef as RefObject<HTMLDivElement>, nudgeReflow };
}
