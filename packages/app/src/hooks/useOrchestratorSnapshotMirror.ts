import { useEffect, useRef, type RefObject } from 'react';
import type { BridgeClient } from '../lib/bridge';
import type { TerminalTransport } from './useTerminalSession';
import {
  CanvasTerminal,
  SnapshotBatcher,
  TERMINAL_FONT_FAMILY,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  terminalThemeFromCss,
} from '../terminal';
import {
  isMobileInputDevice,
  MobileInputGuard,
  type MobileInputDelivery,
} from '../terminal/mobile-input-guard';

interface UseOrchestratorSnapshotMirrorOptions {
  paneId: string;
  sessionKey: number;
  bridge: BridgeClient;
  transport?: TerminalTransport;
  subscribeSnapshots: (paneId: string, cb: (snapshot: string) => void) => () => void;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
}

/**
 * Mobile orchestrator mirror: renders vt100 snapshots at the local viewport size
 * instead of replaying raw PTY bytes (which reflow when the desktop resizes).
 */
export function useOrchestratorSnapshotMirror({
  paneId,
  sessionKey,
  bridge,
  transport,
  subscribeSnapshots,
  mobileInputDelayMs,
  mobileInputVisible,
}: UseOrchestratorSnapshotMirrorOptions): RefObject<HTMLDivElement> {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new CanvasTerminal(container, {
      cols: 80,
      rows: 24,
      fontFamily: TERMINAL_FONT_FAMILY,
      fontSize: TERMINAL_FONT_SIZE,
      scrollback: TERMINAL_SCROLLBACK,
      theme: terminalThemeFromCss(),
    });
    const batcher = new SnapshotBatcher(term);
    term.fit();

    const resizeObserver = new ResizeObserver(() => {
      term.fit();
    });
    resizeObserver.observe(container);

    const unsubSnapshot = subscribeSnapshots(paneId, (snapshot) => {
      batcher.push(snapshot);
    });

    void bridge.readSnapshot(paneId).then((snapshot) => {
      if (snapshot) batcher.push(snapshot);
    }).catch(() => {});

    const emitInput = (text: string, delivery: MobileInputDelivery) => {
      if (!text || !transport) return;
      term.applyLocalInput(text);
      void transport.writeInput(text, delivery === 'immediate');
    };

    const unsubData = term.onData((data) => {
      emitInput(data, 'immediate');
    });

    let mobileGuard: MobileInputGuard | null = null;
    if (isMobileInputDevice()) {
      mobileGuard = new MobileInputGuard({
        container,
        emitInput,
        scrollToCursor: () => {},
        bufferDelayMs: mobileInputDelayMs,
        inputVisible: mobileInputVisible,
      });
    }

    return () => {
      resizeObserver.disconnect();
      unsubSnapshot();
      unsubData();
      mobileGuard?.dispose();
      batcher.dispose();
      term.dispose();
    };
  }, [paneId, sessionKey, bridge, transport, subscribeSnapshots, mobileInputDelayMs, mobileInputVisible]);

  return containerRef as RefObject<HTMLDivElement>;
}
