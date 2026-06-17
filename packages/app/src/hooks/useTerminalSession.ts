import { useEffect, useRef, type RefObject } from 'react';
import { tauri } from '../lib/tauri';
import { TerminalSession } from '../terminal';
import type { PaneSnapshotListener } from '../terminal';

interface UseTerminalSessionOptions {
  paneId: string;
  /** Bump when the pane process is replaced (agent switch). */
  sessionKey: number;
  subscribePaneData: (paneId: string, cb: PaneSnapshotListener) => () => void;
}

/**
 * React lifecycle wrapper around {@link TerminalSession}.
 */
export function useTerminalSession({
  paneId,
  sessionKey,
  subscribePaneData,
}: UseTerminalSessionOptions): RefObject<HTMLDivElement> {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const session = new TerminalSession({
      paneId,
      onResize: (cols, rows) => {
        void tauri.resize(paneId, cols, rows);
      },
      onInput: (text) => {
        void tauri.writeInput(paneId, text, false);
      },
    });

    session.mount(container, subscribePaneData);

    return () => {
      session.dispose();
    };
  }, [paneId, sessionKey, subscribePaneData]);

  return containerRef as RefObject<HTMLDivElement>;
}
