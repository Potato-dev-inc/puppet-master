import { useEffect, useRef, type RefObject } from 'react';
import { tauri } from '../lib/tauri';
import { TerminalSession } from '../terminal';
import type { PaneSnapshotListener } from '../terminal';

export interface TerminalTransport {
  resize: (cols: number, rows: number) => void | Promise<void>;
  writeInput: (text: string) => void | Promise<void>;
}

interface UseTerminalSessionOptions {
  paneId: string;
  /** Bump when the pane process is replaced (agent switch). */
  sessionKey: number;
  subscribePaneData: (paneId: string, cb: PaneSnapshotListener) => () => void;
  transport?: TerminalTransport;
  syncPTYResize?: boolean;
  ptyCols?: number;
  ptyRows?: number;
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
  ptyCols,
  ptyRows,
}: UseTerminalSessionOptions): RefObject<HTMLDivElement> {
  const containerRef = useRef<HTMLDivElement>(null);
  const sessionRef = useRef<TerminalSession | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const defaultTransport = {
      resize: (cols: number, rows: number) => {
        void tauri.resize(paneId, cols, rows);
      },
      writeInput: (text: string) => {
        void tauri.writeInput(paneId, text, false);
      },
    };
    const activeTransport = transport ?? defaultTransport;

    const session = new TerminalSession({
      paneId,
      syncPTYResize,
      ptyCols,
      ptyRows,
      onResize: (cols, rows) => {
        void activeTransport.resize(cols, rows);
      },
      onInput: (text) => {
        void activeTransport.writeInput(text);
      },
    });
    sessionRef.current = session;
    session.mount(container, subscribePaneData);

    return () => {
      session.dispose();
      sessionRef.current = null;
    };
  }, [paneId, sessionKey, subscribePaneData, transport, syncPTYResize]);

  useEffect(() => {
    if (!syncPTYResize && ptyCols && ptyRows) {
      sessionRef.current?.setPtyDimensions(ptyCols, ptyRows);
    }
  }, [syncPTYResize, ptyCols, ptyRows]);

  return containerRef as RefObject<HTMLDivElement>;
}
