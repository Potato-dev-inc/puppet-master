// Watches worker panes for states that need the orchestrator (permission prompts, blocked, finished report).
// Routine permission prompts are auto-approved; everything else is queued as a "wake" for the orchestrator,
// delivered once it is idle.
import { useCallback, useEffect, useRef, type MutableRefObject } from 'react';
import { isOrchestratorPaneId } from '@puppet-master/shared';
import type { PaneRegistryApi } from '../../hooks/usePaneRegistry';
import { makeTauriExecutor } from '../../lib/mcp-executor';
import { classifyPaneAttention, compactPaneText, hashPaneText, type PaneAttention } from '../../lib/pane-attention';
import { tauri } from '../../lib/tauri';
import { approvePermissionIfPresent } from '../../lib/tui-autopilot';

interface AttentionState {
  kind: PaneAttention['kind'];
  hash: string;
  firstSeenAt: number;
  lastNotifiedAt: number;
  lastActionAt: number;
}

const ATTENTION_WAKE_AFTER_MS = 30_000;
const ATTENTION_NOTIFY_DEBOUNCE_MS = 60_000;
const AUTO_APPROVE_DEBOUNCE_MS = 2_500;

interface AttentionInputs {
  panes: PaneRegistryApi['panes'];
  /** True while the orchestrator is mid-run; wakes wait until it clears. */
  busyRef: MutableRefObject<boolean>;
  logAutoApprove: (paneId: string) => void;
  runWake: (text: string) => Promise<void>;
}

export function useAttentionTracking({ panes, busyRef, logAutoApprove, runWake }: AttentionInputs) {
  const paneLookupRef = useRef(panes);
  const attentionRef = useRef<Map<string, AttentionState>>(new Map());
  const queuedWakeRef = useRef<string | null>(null);
  const wakeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wakeRunnerRef = useRef<((text: string) => Promise<void>) | null>(null);

  useEffect(() => { paneLookupRef.current = panes; }, [panes]);
  useEffect(() => { wakeRunnerRef.current = runWake; }, [runWake]);

  const tryRunQueuedWake = useCallback(() => {
    if (wakeTimerRef.current) {
      clearTimeout(wakeTimerRef.current);
      wakeTimerRef.current = null;
    }
    const queued = queuedWakeRef.current;
    if (!queued) return;
    if (busyRef.current) {
      wakeTimerRef.current = setTimeout(tryRunQueuedWake, 1000);
      return;
    }
    queuedWakeRef.current = null;
    void wakeRunnerRef.current?.(queued);
  }, [busyRef]);

  const enqueueWake = useCallback((line: string) => {
    const prefix = '[Worker pane action required]';
    queuedWakeRef.current = queuedWakeRef.current
      ? `${queuedWakeRef.current}\n${line}`
      : `${prefix}\n${line}\n\nRead the affected pane buffers, then use press_key or write_terminal_input to unblock the workers. Complete or update tasks if the worker is done or failed.`;
    tryRunQueuedWake();
  }, [tryRunQueuedWake]);

  const inspectPaneAttention = useCallback(async (paneId: string, snapshot?: string) => {
    if (isOrchestratorPaneId(paneId)) return;
    const paneData = paneLookupRef.current.get(paneId);
    if (!paneData) return;

    const currentSnapshot = snapshot ?? (await tauri.readSnapshot(paneId));
    const buffer = currentSnapshot || (await tauri.readBuffer(paneId, 80));
    const compact = compactPaneText(buffer);
    const attention = classifyPaneAttention(compact, paneData.info);
    const now = Date.now();
    if (attention.kind === 'none') {
      attentionRef.current.delete(paneId);
      return;
    }

    const hash = hashPaneText(`${attention.kind}:${compact.slice(-3000)}`);
    const existing = attentionRef.current.get(paneId);
    const state: AttentionState =
      existing && existing.kind === attention.kind && existing.hash === hash
        ? existing
        : {
            kind: attention.kind,
            hash,
            firstSeenAt: now,
            lastNotifiedAt: 0,
            lastActionAt: 0,
          };
    attentionRef.current.set(paneId, state);

    if (attention.kind === 'routine_permission') {
      if (now - state.lastActionAt < AUTO_APPROVE_DEBOUNCE_MS) return;
      state.lastActionAt = now;
      const verdict = await approvePermissionIfPresent(makeTauriExecutor(), paneId);
      if (verdict === 'approved') {
        logAutoApprove(paneId);
        setTimeout(() => {
          void inspectPaneAttention(paneId);
        }, 700);
      }
      return;
    }

    const paneStatus = paneData.status;
    const stableMs = now - state.firstSeenAt;
    const shouldWake =
      paneStatus !== 'running' ||
      attention.kind === 'report_ready' ||
      stableMs >= ATTENTION_WAKE_AFTER_MS;
    if (!shouldWake) {
      setTimeout(() => {
        void inspectPaneAttention(paneId);
      }, ATTENTION_WAKE_AFTER_MS - stableMs);
      return;
    }
    if (now - state.lastNotifiedAt < ATTENTION_NOTIFY_DEBOUNCE_MS) return;

    state.lastNotifiedAt = now;
    const excerpt = compact.slice(-420).replace(/\s+/g, ' ').trim();
    enqueueWake(
      `pane ${paneId} (${paneData.info.agent_type}, status=${paneStatus}) ${attention.reason}. Recent screen: ${excerpt}`,
    );
  }, [enqueueWake, logAutoApprove]);

  useEffect(() => {
    let disposed = false;
    let unlistenSnapshot: (() => void) | null = null;
    let unlistenStatus: (() => void) | null = null;
    let unlistenPanesChanged: (() => void) | null = null;

    const inspectKnownPanes = () => {
      for (const pane of paneLookupRef.current.values()) {
        if (!isOrchestratorPaneId(pane.info.id)) {
          void inspectPaneAttention(pane.info.id);
        }
      }
    };

    void (async () => {
      unlistenSnapshot = await tauri.onTerminalSnapshot((event) => {
        void inspectPaneAttention(event.pane_id, event.snapshot);
      });
      unlistenStatus = await tauri.onPtyStatus((event) => {
        void inspectPaneAttention(event.pane_id);
      });
      unlistenPanesChanged = await tauri.onPanesChanged(() => {
        setTimeout(inspectKnownPanes, 250);
      });
      setTimeout(inspectKnownPanes, 250);
      if (disposed) {
        unlistenSnapshot?.();
        unlistenStatus?.();
        unlistenPanesChanged?.();
      }
    })();

    return () => {
      disposed = true;
      unlistenSnapshot?.();
      unlistenStatus?.();
      unlistenPanesChanged?.();
    };
  }, [inspectPaneAttention]);
}
