import { useEffect, useRef, useState } from 'react';
import { findBridgeUrl, makeBridgeClient, subscribeBridgeEvents, type BridgeClient } from '../lib/bridge';
import type { McpLogEntry, PaneInfo } from '@puppet-master/shared';

export interface BridgeApi {
  client: BridgeClient | null;
  panesFromBridge: PaneInfo[];
  externalLogs: McpLogEntry[];
  isReady: boolean;
  error: string | null;
}

export function useBridge(): BridgeApi {
  const [client, setClient] = useState<BridgeClient | null>(null);
  const [panesFromBridge, setPanesFromBridge] = useState<PaneInfo[]>([]);
  const [externalLogs, setExternalLogs] = useState<McpLogEntry[]>([]);
  const [isReady, setIsReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const clientRef = useRef<BridgeClient | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unsub: (() => void) | null = null;
    let pollTimer: ReturnType<typeof setTimeout> | null = null;

    async function tryConnect() {
      if (cancelled) return;
      const url = await findBridgeUrl();
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
    };
  }, []);

  return { client, panesFromBridge, externalLogs, isReady, error };
}