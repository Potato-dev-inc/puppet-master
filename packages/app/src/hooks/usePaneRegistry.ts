import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PaneStreamManager, createOwnerReplayReader } from '../terminal';
import type { PaneDataSubscriber } from '../terminal/types';
import {
  tauri,
  type PaneInfo,
  type PaneStatusEvent,
  type TerminalDataEvent,
  type TerminalSnapshotEvent,
} from '../lib/tauri';

export interface PaneData {
  info: PaneInfo;
  status: PaneInfo['status'];
}

export interface PaneRegistryApi {
  panes: Map<string, PaneData>;
  paneList: PaneData[];
  /** True after the first pane list fetch completes. */
  initialReady: boolean;
  spawnPane: (args: Parameters<typeof tauri.spawnPane>[0]) => Promise<string>;
  killPane: (paneId: string) => Promise<void>;
  replacePaneAgent: (paneId: string, agentType: string, cwd?: string) => Promise<string>;
  killAll: () => Promise<void>;
  writeInput: (paneId: string, text: string, appendNewline?: boolean) => Promise<void>;
  resize: (paneId: string, cols: number, rows: number) => Promise<void>;
  refresh: () => Promise<void>;
  subscribePaneData: PaneDataSubscriber;
}

/** Mount replay for this window: the screen at the PTY's current size, not size-mixed raw history. */
const readOwnerReplay = createOwnerReplayReader({
  readScreen: tauri.readScreenReplay,
  readRaw: tauri.readRawBuffer,
  pendingResize: tauri.pendingResize,
});

const EMPTY: Map<string, PaneData> = new Map();

export function usePaneRegistry(): PaneRegistryApi {
  const [panes, setPanes] = useState<Map<string, PaneData>>(EMPTY);
  const [initialReady, setInitialReady] = useState(false);
  const panesRef = useRef(panes);
  panesRef.current = panes;

  const streamsRef = useRef(new PaneStreamManager());
  const replacingRef = useRef<Set<string>>(new Set());
  const exitRefreshTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  // Refreshes overlap (a pane event fires one while another is in flight). Only the newest one may
  // write: a slower, older list would bring back a pane that just closed (a ghost sidebar row) or
  // hide one that just opened.
  const refreshSeq = useRef(0);

  const refresh = useCallback(async () => {
    const seq = ++refreshSeq.current;
    try {
      const list = await tauri.listPanes();
      if (seq !== refreshSeq.current) return;
      const next = new Map<string, PaneData>();
      for (const info of list) {
        next.set(info.id, { info, status: info.status });
      }
      setPanes(next);
    } catch (err) {
      console.error('[usePaneRegistry] refresh failed', err);
    } finally {
      if (seq === refreshSeq.current) setInitialReady(true);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    let unsubSnapshot: (() => void) | undefined;
    let unsubData: (() => void) | undefined;
    let unsubStatus: (() => void) | undefined;
    let unsubExit: (() => void) | undefined;
    let unsubPanesChanged: (() => void) | undefined;
    let unsubResized: (() => void) | undefined;

    (async () => {
      unsubPanesChanged = await tauri.onPanesChanged(() => {
        void refresh();
      });
      const ingestData = (e: TerminalDataEvent) => {
        streamsRef.current.ingest(e.pane_id, e.data, e.offset);
      };
      unsubData = await tauri.onTerminalData(ingestData);
      unsubResized = await tauri.onPtyResized((e) => {
        streamsRef.current.ingestResize(e.pane_id, e.cols, e.rows, e.offset);
      });
      unsubSnapshot = await tauri.onTerminalSnapshot((e: TerminalSnapshotEvent) => {
        // Snapshot events are retained for MCP/debug consumers; xterm rendering
        // consumes raw terminal-data events instead.
        void e;
      });
      unsubStatus = await tauri.onPtyStatus((e: PaneStatusEvent) => {
        setPanes((prev) => {
          const existing = prev.get(e.pane_id);
          if (!existing || existing.status === e.status) return prev;
          const next = new Map(prev);
          next.set(e.pane_id, { ...existing, status: e.status });
          return next;
        });
      });
      unsubExit = await tauri.onPtyExit((e) => {
        if (replacingRef.current.has(e.pane_id)) {
          return;
        }
        setPanes((prev) => {
          const next = new Map(prev);
          const existing = next.get(e.pane_id);
          if (existing) {
            next.set(e.pane_id, { ...existing, status: 'error' });
          }
          return next;
        });
        const pending = exitRefreshTimers.current.get(e.pane_id);
        if (pending) clearTimeout(pending);
        const timer = setTimeout(() => {
          exitRefreshTimers.current.delete(e.pane_id);
          if (!replacingRef.current.has(e.pane_id)) {
            void refresh();
          }
        }, 200);
        exitRefreshTimers.current.set(e.pane_id, timer);
      });

      if (disposed) {
        unsubData?.();
        unsubResized?.();
        unsubSnapshot?.();
        unsubStatus?.();
        unsubExit?.();
        unsubPanesChanged?.();
      }
    })();

    return () => {
      disposed = true;
      unsubData?.();
      unsubResized?.();
      unsubSnapshot?.();
      unsubStatus?.();
      unsubExit?.();
      unsubPanesChanged?.();
    };
  }, [refresh]);

  const spawnPane = useCallback(async (args: Parameters<typeof tauri.spawnPane>[0]) => {
    const id = await tauri.spawnPane(args);
    await refresh();
    return id;
  }, [refresh]);

  const killPane = useCallback(async (paneId: string) => {
    await tauri.killPane(paneId);
    streamsRef.current.delete(paneId);
    await refresh();
  }, [refresh]);

  const replacePaneAgent = useCallback(
    async (paneId: string, agentType: string, cwd?: string) => {
      const existing = panesRef.current.get(paneId);
      if (!existing) {
        throw new Error(`unknown pane: ${paneId}`);
      }
      const { cols, rows } = existing.info;
      const spawnCwd = cwd ?? existing.info.cwd;

      streamsRef.current.reset(paneId);

      const pendingExit = exitRefreshTimers.current.get(paneId);
      if (pendingExit) {
        clearTimeout(pendingExit);
        exitRefreshTimers.current.delete(paneId);
      }

      replacingRef.current.add(paneId);
      try {
        await tauri.killPane(paneId);
        const id = await tauri.spawnPane({
          agent_type: agentType,
          pane_id: paneId,
          cols,
          rows,
          cwd: spawnCwd,
        });
        await refresh();
        return id;
      } catch (err) {
        await refresh();
        throw err;
      } finally {
        setTimeout(() => {
          replacingRef.current.delete(paneId);
        }, 800);
      }
    },
    [refresh],
  );

  const killAll = useCallback(async () => {
    await tauri.killAllPanes();
    await refresh();
  }, [refresh]);

  const writeInput = useCallback(async (paneId: string, text: string, appendNewline = true) => {
    await tauri.writeInput(paneId, text, appendNewline);
  }, []);

  const resize = useCallback(async (paneId: string, cols: number, rows: number) => {
    await tauri.resize(paneId, cols, rows);
  }, []);

  const subscribePaneData = useCallback<PaneDataSubscriber>((paneId, cb, onResize) => {
    return streamsRef.current.subscribe(paneId, cb, readOwnerReplay, onResize);
  }, []);


  const paneList = useMemo(
    () => Array.from(panes.values()).sort((a, b) => a.info.created_at - b.info.created_at),
    [panes],
  );

  return {
    panes,
    paneList,
    initialReady,
    spawnPane,
    killPane,
    replacePaneAgent,
    killAll,
    writeInput,
    resize,
    refresh,
    subscribePaneData,
  };
}
