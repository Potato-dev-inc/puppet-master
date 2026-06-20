import { useEffect, useRef, useState } from 'react';
import { findBridgeUrl, makeBridgeClient, subscribeBridgeEvents, type BridgeClient } from '../lib/bridge';
import type { McpLogEntry, OrchestratorChatEvent, OrchestratorUserMessage, PaneInfo } from '@puppet-master/shared';
import type { MobileOrchestratorViewport } from './useTerminalAuthorityCss';

const MOBILE_VIEWPORT_STALE_MS = 15_000;

export interface BridgeApi {
  client: BridgeClient | null;
  panesFromBridge: PaneInfo[];
  externalLogs: McpLogEntry[];
  chatEvents: OrchestratorChatEvent[];
  isReady: boolean;
  error: string | null;
  mobileOrchestratorViewport: MobileOrchestratorViewport | null;
  /** Subscribe to user messages posted from the mobile PWA. Returns unlisten fn. */
  onRemoteOrchestratorMessage: (cb: (msg: OrchestratorUserMessage) => void) => () => void;
}

export function useBridge(bridgeUrl?: string): BridgeApi {
  const [client, setClient] = useState<BridgeClient | null>(null);
  const [panesFromBridge, setPanesFromBridge] = useState<PaneInfo[]>([]);
  const [externalLogs, setExternalLogs] = useState<McpLogEntry[]>([]);
  const [chatEvents, setChatEvents] = useState<OrchestratorChatEvent[]>([]);
  const [isReady, setIsReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mobileOrchestratorViewport, setMobileOrchestratorViewport] =
    useState<MobileOrchestratorViewport | null>(null);
  const clientRef = useRef<BridgeClient | null>(null);
  const remoteListeners = useRef(new Set<(msg: OrchestratorUserMessage) => void>());
  const mobileViewportUpdatedAt = useRef(0);

  const onRemoteOrchestratorMessage = (cb: (msg: OrchestratorUserMessage) => void): (() => void) => {
    remoteListeners.current.add(cb);
    return () => { remoteListeners.current.delete(cb); };
  };

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (mobileViewportUpdatedAt.current <= 0) return;
      if (Date.now() - mobileViewportUpdatedAt.current > MOBILE_VIEWPORT_STALE_MS) {
        mobileViewportUpdatedAt.current = 0;
        setMobileOrchestratorViewport(null);
      }
    }, 2000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unsub: (() => void) | null = null;
    let pollTimer: ReturnType<typeof setTimeout> | null = null;

    async function tryConnect() {
      if (cancelled) return;
      const url = bridgeUrl ?? await findBridgeUrl();
      if (cancelled) return;
      if (!url) {
        setError('Bridge not reachable. Is the Puppet Master GUI running?');
        pollTimer = setTimeout(tryConnect, 2000);
        return;
      }
      const c = makeBridgeClient(url);
      clientRef.current = c;
      setClient(c);
      setIsReady(true);
      setError(null);
      unsub = subscribeBridgeEvents(
        url,
        (ev) => {
          if (ev.type === 'panes') setPanesFromBridge(ev.panes);
          if (ev.type === 'log') {
            setExternalLogs((prev) => [ev.entry, ...prev].slice(0, 500));
          }
          if (ev.type === 'chat') {
            setChatEvents((prev) => [...prev, ev.event].slice(-200));
            if (ev.event.type === 'user') {
              const userMsg = ev.event as Extract<OrchestratorChatEvent, { type: 'user' }>;
              const msg: OrchestratorUserMessage = { text: userMsg.text, message_id: userMsg.message_id };
              for (const cb of remoteListeners.current) cb(msg);
            }
          }
          if (ev.type === 'orchestrator-viewport') {
            mobileViewportUpdatedAt.current = Date.now();
            if (ev.active && ev.width > 0) {
              setMobileOrchestratorViewport({
                width: ev.width,
                height: ev.height,
                active: true,
              });
            } else {
              mobileViewportUpdatedAt.current = 0;
              setMobileOrchestratorViewport(null);
            }
          }
        },
        () => {
          /* swallow — SSE retries internally */
        },
      );
    }

    tryConnect();
    return () => {
      cancelled = true;
      unsub?.();
      if (pollTimer) clearTimeout(pollTimer);
      mobileViewportUpdatedAt.current = 0;
      setMobileOrchestratorViewport(null);
    };
  }, [bridgeUrl]);

  return {
    client,
    panesFromBridge,
    externalLogs,
    chatEvents,
    isReady,
    error,
    mobileOrchestratorViewport,
    onRemoteOrchestratorMessage,
  };
}
