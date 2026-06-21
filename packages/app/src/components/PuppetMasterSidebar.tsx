import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent } from 'react';
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
  type LlmModel,
  type McpLogEntry,
  type OrchestratorBackend,
  type OrchestratorChatEvent,
  isOrchestratorPaneId,
} from '@puppet-master/shared';
import type { BridgeClient } from '../lib/bridge';
import type { ChatMessage } from '../lib/llm';
import { runPuppetMasterLoop } from '../lib/puppet-master';
import { runPuppetMasterCliLoop } from '../lib/puppet-master-cli';
import { makeBridgeExecutor, makeTauriExecutor } from '../lib/mcp-tools';
import { findModel, getApiKey, invalidateSettingsCache, listModels, loadSettings, saveSettings } from '../lib/settings';
import { tauri } from '../lib/tauri';
import { approvePermissionIfPresent } from '../lib/tui-autopilot';
import { classifyPaneAttention, compactPaneText, hashPaneText, type PaneAttention } from '../lib/pane-attention';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { usePaneTunnel } from '../hooks/usePaneTunnel';
import {
  ensureOrchestratorPane,
  findOrchestratorPane,
  isCliOrchestratorBackend,
  type CliOrchestratorBackend,
} from '../lib/orchestrator-panes';
import { isValidProjectPath, projectPathsEqual } from '../lib/project-path';
import { OrchestratorTerminal } from './OrchestratorTerminal';
import { CoordinationPanel } from './CoordinationPanel';

interface UiLogEntry {
  id: string;
  ts: number;
  source: 'builtin' | 'external';
  tool: string;
  args: unknown;
  result?: string;
  error?: string;
}

interface UiMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  text: string;
}

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

interface Props {
  width: number;
  bridge: BridgeClient | null;
  bridgeReady: boolean;
  externalLogs: McpLogEntry[];
  registry: PaneRegistryApi;
  projectPath: string | null;
  onShowSettings: () => void;
  settingsRevision?: number;
}

export function PuppetMasterSidebar({
  width,
  bridge,
  bridgeReady,
  externalLogs,
  registry,
  projectPath,
  onShowSettings,
  settingsRevision = 0,
}: Props) {
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [logs, setLogs] = useState<UiLogEntry[]>([]);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [model, setModel] = useState<LlmModel>(DEFAULT_LLM_MODELS[0]);
  const [availableModels, setAvailableModels] = useState<LlmModel[]>(DEFAULT_LLM_MODELS);
  const [backend, setBackend] = useState<OrchestratorBackend>('api');
  const [theme, setTheme] = useState<'dark' | 'light'>('dark');
  const [orchestratorStarting, setOrchestratorStarting] = useState(false);
  const [orchestratorError, setOrchestratorError] = useState<string | null>(null);
  const [mcpStatus, setMcpStatus] = useState<string | null>(null);
  const [mcpLogHeight, setMcpLogHeight] = useState(128);
  const [standbyPanes, setStandbyPanes] = useState<Array<{ id: string; status: string }>>([]);
  const abortRef = useRef<AbortController | null>(null);
  const busyRef = useRef(busy);
  const draftRef = useRef(draft);
  const messagesRef = useRef(messages);
  const backendRef = useRef(backend);
  const modelRef = useRef(model);
  const bridgeRef = useRef(bridge);
  const paneLookupRef = useRef(registry.panes);
  const lastDraftChangeRef = useRef(Date.now());
  const attentionRef = useRef<Map<string, AttentionState>>(new Map());
  const queuedWakeRef = useRef<string | null>(null);
  const wakeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wakeRunnerRef = useRef<((text: string) => Promise<void>) | null>(null);

  useEffect(() => { busyRef.current = busy; }, [busy]);
  useEffect(() => { draftRef.current = draft; lastDraftChangeRef.current = Date.now(); }, [draft]);
  useEffect(() => { messagesRef.current = messages; }, [messages]);
  useEffect(() => { backendRef.current = backend; }, [backend]);
  useEffect(() => { modelRef.current = model; }, [model]);
  useEffect(() => { bridgeRef.current = bridge; }, [bridge]);
  useEffect(() => { paneLookupRef.current = registry.panes; }, [registry.panes]);

  const cliBackend = isCliOrchestratorBackend(backend) ? backend : null;
  const orchestratorPane = useMemo(() => {
    if (!cliBackend) return undefined;
    const info = findOrchestratorPane(
      registry.paneList.map((pane) => pane.info),
      cliBackend,
    );
    if (!info) return undefined;
    return registry.panes.get(info.id);
  }, [cliBackend, registry.paneList, registry.panes]);

  const orchestratorTunnel = usePaneTunnel(
    null,
    orchestratorPane?.info.id,
    'desktop',
    registry,
  );

  const startOrchestratorPane = useCallback(async (activeBackend: CliOrchestratorBackend, cwd: string) => {
    setOrchestratorStarting(true);
    setOrchestratorError(null);
    setMcpStatus(null);
    try {
      await ensureOrchestratorPane(activeBackend, cwd);
      setMcpStatus('Puppet Master MCP configured');
      await registry.refresh();
    } catch (err) {
      setOrchestratorError(err instanceof Error ? err.message : String(err));
    } finally {
      setOrchestratorStarting(false);
    }
  }, [registry]);

  const logAutoApprove = useCallback((paneId: string) => {
    setLogs((prev) => {
      const entry: UiLogEntry = {
        id: crypto.randomUUID(),
        ts: Date.now(),
        source: 'builtin',
        tool: 'auto-approve',
        args: { pane_id: paneId },
        result: 'permission prompt approved',
      };
      return [entry, ...prev].slice(0, 500);
    });
  }, []);

  const runInternalWake = useCallback(async (text: string) => {
    const settings = await loadSettings();
    const activeBackend = settings.orchestrator_backend ?? backendRef.current;
    const wakeMessage: UiMessage = {
      id: crypto.randomUUID(),
      role: 'system',
      text,
    };
    setMessages((m) => [...m, wakeMessage]);

    if (activeBackend === 'api') {
      const activeModel = modelRef.current;
      const apiKey = getApiKey(settings, activeModel.provider);
      if (!apiKey) {
        setMessages((m) => [
          ...m,
          { id: crypto.randomUUID(), role: 'system', text: `Worker needs attention, but no ${activeModel.provider} API key is set.` },
        ]);
        return;
      }

      const streamId = 'streaming-' + crypto.randomUUID();
      setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
      busyRef.current = true;
      setBusy(true);
      const controller = new AbortController();
      abortRef.current = controller;
      const history: ChatMessage[] = messagesRef.current
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m): ChatMessage =>
          m.role === 'user'
            ? { role: 'user', content: m.text }
            : { role: 'assistant', content: [{ type: 'text', text: m.text }] },
        );

      await runPuppetMasterLoop(
        activeModel,
        apiKey,
        bridgeRef.current ? makeBridgeExecutor(bridgeRef.current) : makeTauriExecutor(),
        history,
        text,
        {
          onAssistantText: (t) => {
            setMessages((m) => {
              const last = m[m.length - 1];
              if (last && last.role === 'assistant' && last.id === streamId) {
                return [...m.slice(0, -1), { ...last, text: last.text + t }];
              }
              return [...m, { id: streamId, role: 'assistant', text: t }];
            });
          },
          onToolCall: (tool, args, result, error) => {
            setLogs((prev) => {
              const entry: UiLogEntry = {
                id: crypto.randomUUID(),
                ts: Date.now(),
                source: 'builtin',
                tool,
                args,
                result,
                error,
              };
              return [entry, ...prev].slice(0, 500);
            });
          },
          onComplete: () => {
            busyRef.current = false;
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
            ]);
            busyRef.current = false;
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onStandby: (running) => setStandbyPanes(running),
          onAutoApprove: logAutoApprove,
        },
        controller.signal,
      );
      return;
    }

    if (isCliOrchestratorBackend(activeBackend)) {
      busyRef.current = true;
      setBusy(true);
      const streamId = 'streaming-' + crypto.randomUUID();
      setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
      const controller = new AbortController();
      abortRef.current = controller;

      await runPuppetMasterCliLoop(activeBackend, bridgeRef.current, text, {
        onAssistantText: (t) => {
          setMessages((m) => {
            const last = m[m.length - 1];
            if (last && last.role === 'assistant' && last.id === streamId) {
              return [...m.slice(0, -1), { ...last, text: last.text + t }];
            }
            return [...m, { id: streamId, role: 'assistant', text: t }];
          });
        },
        onToolCall: (tool, args, result, error) => {
          setLogs((prev) => {
            const entry: UiLogEntry = {
              id: crypto.randomUUID(),
              ts: Date.now(),
              source: 'builtin',
              tool,
              args,
              result,
              error,
            };
            return [entry, ...prev].slice(0, 500);
          });
        },
        onComplete: () => {
          busyRef.current = false;
          setBusy(false);
          abortRef.current = null;
        },
        onError: (err) => {
          setMessages((m) => [
            ...m,
            { id: crypto.randomUUID(), role: 'system', text: err.message },
          ]);
          busyRef.current = false;
          setBusy(false);
          abortRef.current = null;
        },
      }, controller.signal);
    }
  }, [logAutoApprove]);

  useEffect(() => {
    wakeRunnerRef.current = runInternalWake;
  }, [runInternalWake]);

  const tryRunQueuedWake = useCallback(() => {
    if (wakeTimerRef.current) {
      clearTimeout(wakeTimerRef.current);
      wakeTimerRef.current = null;
    }
    const queued = queuedWakeRef.current;
    if (!queued) return;
    const userIsTyping = draftRef.current.trim().length > 0;
    const draftIsFresh = Date.now() - lastDraftChangeRef.current < ATTENTION_WAKE_AFTER_MS;
    if (busyRef.current || (userIsTyping && draftIsFresh)) {
      wakeTimerRef.current = setTimeout(tryRunQueuedWake, 1000);
      return;
    }
    queuedWakeRef.current = null;
    void wakeRunnerRef.current?.(queued);
  }, []);

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

  const orchestratorPaneId = orchestratorPane?.info.id;
  const orchestratorPaneCwd = orchestratorPane?.info.cwd;
  const orchestratorPaneStatus = orchestratorPane?.status;

  useEffect(() => {
    if (!cliBackend || !isValidProjectPath(projectPath)) {
      setOrchestratorError(null);
      setOrchestratorStarting(false);
      return;
    }
    if (orchestratorStarting) return;
    if (!orchestratorPaneId) {
      void startOrchestratorPane(cliBackend, projectPath);
      return;
    }
    if (!orchestratorPaneCwd || !projectPathsEqual(orchestratorPaneCwd, projectPath)) {
      void startOrchestratorPane(cliBackend, projectPath);
      return;
    }
    if (orchestratorPaneStatus === 'error') {
      return;
    }
  }, [
    cliBackend,
    projectPath,
    orchestratorPaneId,
    orchestratorPaneCwd,
    orchestratorPaneStatus,
    orchestratorStarting,
    startOrchestratorPane,
  ]);

  const refreshFromSettings = useCallback(async () => {
    const s = await loadSettings();
    const models = listModels(s);
    setAvailableModels(models);
    setBackend(s.orchestrator_backend ?? 'api');
    setTheme(s.theme ?? 'dark');
    const found = findModel(s, s.default_provider, s.default_model) ?? models[0];
    if (found) setModel(found);
  }, []);

  useEffect(() => {
    void refreshFromSettings();
  }, [refreshFromSettings, settingsRevision]);

  useEffect(() => {
    let cancelled = false;
    let unlistenSettings: (() => void) | null = null;
    let unlistenEnsure: (() => void) | null = null;

    void (async () => {
      unlistenSettings = await tauri.onSettingsChanged((payload) => {
        invalidateSettingsCache();
        if (payload.orchestrator_backend) {
          setBackend(payload.orchestrator_backend as OrchestratorBackend);
        }
        void refreshFromSettings();
      });
      unlistenEnsure = await tauri.onOrchestratorEnsure(async (payload) => {
        if (!isCliOrchestratorBackend(payload.backend as OrchestratorBackend)) return;
        const cwd = projectPath ?? (await tauri.getProjectPath());
        if (isValidProjectPath(cwd)) void startOrchestratorPane(payload.backend as CliOrchestratorBackend, cwd);
      });
      if (cancelled) {
        unlistenSettings?.();
        unlistenEnsure?.();
      }
    })();

    return () => {
      cancelled = true;
      unlistenSettings?.();
      unlistenEnsure?.();
    };
  }, [projectPath, refreshFromSettings, startOrchestratorPane]);

  // Listen for messages POSTed from the mobile PWA via the Rust bridge
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    const pushChat = (event: OrchestratorChatEvent) => {
      void tauri.pushChatEvent(JSON.stringify(event));
    };

    const processRemoteMessage = async (msg: { text: string; message_id: string }) => {
      if (cancelled) return;
      const settings = await loadSettings();
      const activeBackend = settings.orchestrator_backend ?? 'api';

      const userUiMsg: UiMessage = { id: msg.message_id, role: 'user', text: msg.text };
      setMessages((m) =>
        m.some((entry) => entry.id === msg.message_id) ? m : [...m, userUiMsg],
      );

      if (activeBackend === 'api') {
        const apiKey = getApiKey(settings, model.provider);
        if (!apiKey) {
          const errText = `No ${model.provider} API key set. Open Settings on desktop.`;
          setMessages((prev) => [
            ...prev,
            { id: crypto.randomUUID(), role: 'system', text: errText },
          ]);
          pushChat({ type: 'error', message_id: msg.message_id, error: errText });
          return;
        }

        const streamId = `streaming-${msg.message_id}`;
        setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
        setBusy(true);

        const controller = new AbortController();
        abortRef.current = controller;

        await runPuppetMasterLoop(
          model,
          apiKey,
          bridge ? makeBridgeExecutor(bridge) : makeTauriExecutor(),
          [],
          msg.text,
          {
            onAssistantText: (t) => {
              setMessages((m) => {
                const last = m[m.length - 1];
                if (last && last.role === 'assistant' && last.id === streamId) {
                  return [...m.slice(0, -1), { ...last, text: last.text + t }];
                }
                return [...m, { id: streamId, role: 'assistant', text: t }];
              });
              pushChat({ type: 'text', message_id: msg.message_id, text: t });
            },
            onToolCall: (tool, args, result, error) => {
              const entry: UiLogEntry = {
                id: crypto.randomUUID(),
                ts: Date.now(),
                source: 'builtin',
                tool,
                args,
                result,
                error,
              };
              setLogs((prev) => [entry, ...prev].slice(0, 500));
              pushChat({ type: 'tool', message_id: msg.message_id, tool, result, error });
            },
            onComplete: () => {
              setBusy(false);
              setStandbyPanes([]);
              abortRef.current = null;
              pushChat({ type: 'done', message_id: msg.message_id });
            },
            onError: (err) => {
              setMessages((m) => [
                ...m,
                { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
              ]);
              setBusy(false);
              setStandbyPanes([]);
              abortRef.current = null;
              pushChat({ type: 'error', message_id: msg.message_id, error: err.message });
            },
            onStandby: (running) => setStandbyPanes(running),
            onAutoApprove: logAutoApprove,
          },
          controller.signal,
        );
        return;
      }

      if (isCliOrchestratorBackend(activeBackend)) {
        setBusy(true);
        const streamId = `streaming-${msg.message_id}`;
        setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
        const controller = new AbortController();
        abortRef.current = controller;

        await runPuppetMasterCliLoop(activeBackend, bridge, msg.text, {
          onAssistantText: (t) => {
            setMessages((m) => {
              const last = m[m.length - 1];
              if (last && last.role === 'assistant' && last.id === streamId) {
                return [...m.slice(0, -1), { ...last, text: last.text + t }];
              }
              return [...m, { id: streamId, role: 'assistant', text: t }];
            });
            pushChat({ type: 'text', message_id: msg.message_id, text: t });
          },
          onToolCall: (tool, args, result, error) => {
            const entry: UiLogEntry = {
              id: crypto.randomUUID(),
              ts: Date.now(),
              source: 'builtin',
              tool,
              args,
              result,
              error,
            };
            setLogs((prev) => [entry, ...prev].slice(0, 500));
            pushChat({ type: 'tool', message_id: msg.message_id, tool, result, error });
          },
          onComplete: () => {
            setBusy(false);
            abortRef.current = null;
            pushChat({ type: 'done', message_id: msg.message_id });
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: err.message },
            ]);
            setBusy(false);
            abortRef.current = null;
            pushChat({ type: 'error', message_id: msg.message_id, error: err.message });
          },
        }, controller.signal);
      }
    };

    tauri.onOrchestratorMessage((msg) => {
      void processRemoteMessage(msg);
    }).then((fn) => { unlisten = fn; }).catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [bridge, model]);

  useEffect(() => {
    if (externalLogs.length === 0) return;
    setLogs((prev) => {
      const known = new Set(prev.map((l) => l.id));
      const fresh: UiLogEntry[] = externalLogs
        .filter((l) => !known.has(l.id))
        .map((l) => ({
          id: l.id,
          ts: l.timestamp,
          source: l.source,
          tool: l.tool,
          args: l.args,
          result: l.result_preview,
          error: l.error,
        }));
      return [...fresh, ...prev].slice(0, 500);
    });
  }, [externalLogs]);

  const send = useCallback(async () => {
    const text = draft.trim();
    if (!text) return;
    setDraft('');

    const settings = await loadSettings();
    const activeBackend = settings.orchestrator_backend ?? 'api';

    if (activeBackend === 'api') {
      const apiKey = getApiKey(settings, model.provider);
      if (!apiKey) {
        setMessages((m) => [
          ...m,
          { id: crypto.randomUUID(), role: 'system', text: `No ${model.provider} API key set. Open Settings.` },
        ]);
        onShowSettings();
        return;
      }

      const userMsg: UiMessage = { id: crypto.randomUUID(), role: 'user', text };
      const streamId = 'streaming-' + crypto.randomUUID();
      setMessages((m) => [...m, userMsg, { id: streamId, role: 'assistant', text: '' }]);
      setBusy(true);

      const controller = new AbortController();
      abortRef.current = controller;
      const history: ChatMessage[] = messages
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m): ChatMessage =>
          m.role === 'user'
            ? { role: 'user', content: m.text }
            : { role: 'assistant', content: [{ type: 'text', text: m.text }] },
        );

      await runPuppetMasterLoop(
        model,
        apiKey,
        bridge ? makeBridgeExecutor(bridge) : makeTauriExecutor(),
        history,
        text,
        {
          onAssistantText: (t) => {
            setMessages((m) => {
              const last = m[m.length - 1];
              if (last && last.role === 'assistant' && last.id.startsWith('streaming-')) {
                return [...m.slice(0, -1), { ...last, text: last.text + t }];
              }
              return [...m, { id: 'streaming-' + crypto.randomUUID(), role: 'assistant', text: t }];
            });
          },
          onToolCall: (tool, args, result, error) => {
            setLogs((prev) => {
              const entry: UiLogEntry = {
                id: crypto.randomUUID(),
                ts: Date.now(),
                source: 'builtin',
                tool,
                args,
                result,
                error,
              };
              return [entry, ...prev].slice(0, 500);
            });
          },
          onComplete: () => {
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
            ]);
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onStandby: (running) => setStandbyPanes(running),
          onAutoApprove: logAutoApprove,
        },
        controller.signal,
      );
      return;
    }

    const userMsg: UiMessage = { id: crypto.randomUUID(), role: 'user', text };
    setMessages((m) => [...m, userMsg]);
    setBusy(true);
    const controller = new AbortController();
    abortRef.current = controller;

    await runPuppetMasterCliLoop(
      activeBackend,
      bridge,
      text,
      {
        onAssistantText: (t) => {
          setMessages((m) => [...m, { id: crypto.randomUUID(), role: 'assistant', text: t }]);
        },
        onToolCall: () => {},
        onComplete: () => {
          setBusy(false);
          abortRef.current = null;
        },
        onError: (err) => {
          setMessages((m) => [
            ...m,
            { id: crypto.randomUUID(), role: 'system', text: err.message },
          ]);
          setBusy(false);
          abortRef.current = null;
        },
      },
      controller.signal,
    );
  }, [bridge, bridgeReady, draft, messages, model, onShowSettings]);

  const interrupt = useCallback(() => {
    abortRef.current?.abort();
    setBusy(false);
    setStandbyPanes([]);
  }, []);

  const newChat = useCallback(() => {
    abortRef.current?.abort();
    setBusy(false);
    setStandbyPanes([]);
    setMessages([]);
    setLogs([]);
    setDraft('');
  }, []);

  const presetModels = availableModels.filter(
    (m) => DEFAULT_LLM_MODELS.some((p) => p.provider === m.provider && p.model_id === m.model_id),
  );
  const customModels = availableModels.filter(
    (m) => !DEFAULT_LLM_MODELS.some((p) => p.provider === m.provider && p.model_id === m.model_id),
  );

  const resizeMcpLog = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = mcpLogHeight;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);

    const move = (moveEvent: globalThis.PointerEvent) => {
      const delta = startY - moveEvent.clientY;
      setMcpLogHeight(Math.min(280, Math.max(72, startHeight + delta)));
    };
    const up = (upEvent: globalThis.PointerEvent) => {
      target.releasePointerCapture(upEvent.pointerId);
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
      target.removeEventListener('pointercancel', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
    target.addEventListener('pointercancel', up);
  };

  return (
    <aside
      className="flex-shrink-0 border-l border-pm-border bg-pm-panel flex flex-col"
      style={{ width }}
    >
      <div className="px-3 py-2 border-b border-pm-border text-sm font-semibold flex items-center gap-2">
        <span>Puppet Master</span>
        <span
          className={`w-1.5 h-1.5 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`}
          title={bridgeReady ? 'Bridge connected' : 'Bridge not reachable'}
        />
        <div className="flex-1" />
        <button
          onClick={newChat}
          className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
          title="Start a new chat (clears history)"
          disabled={busy}
        >
          New chat
        </button>
        <button
          onClick={() => {
            void (async () => {
              const next = theme === 'dark' ? 'light' : 'dark';
              setTheme(next);
              const current = await loadSettings();
              await saveSettings({ ...current, theme: next });
            })();
          }}
          className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
          title="Toggle monochrome light/dark theme"
        >
          {theme === 'dark' ? 'Light' : 'Dark'}
        </button>
        <button
          onClick={onShowSettings}
          className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
          title="Settings"
        >
          ⚙
        </button>
      </div>

      <div className="flex flex-col gap-1 px-3 py-1 border-b border-pm-border text-xs">
        <div className="flex items-center gap-2">
          <span className="text-pm-muted shrink-0">Backend:</span>
          <select
            value={backend}
            onChange={async (e) => {
              const next = e.target.value as OrchestratorBackend;
              setBackend(next);
              const s = await loadSettings();
              await saveSettings({ ...s, orchestrator_backend: next });
            }}
            className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs"
          >
            {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((b) => (
              <option key={b} value={b}>
                {ORCHESTRATOR_BACKEND_LABELS[b]}
              </option>
            ))}
          </select>
        </div>
        {backend === 'api' && (
          <div className="flex items-center gap-2">
            <span className="text-pm-muted shrink-0">Model:</span>
            <select
              value={modelKey(model)}
              onChange={async (e) => {
                const found = availableModels.find((m) => modelKey(m) === e.target.value);
                if (!found) return;
                setModel(found);
                const s = await loadSettings();
                await saveSettings({
                  ...s,
                  default_provider: found.provider,
                  default_model: found.model_id,
                });
              }}
              className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs"
            >
              <optgroup label="Built-in">
                {presetModels.map((m) => (
                  <option key={modelKey(m)} value={modelKey(m)}>
                    {m.label}
                  </option>
                ))}
              </optgroup>
              {customModels.length > 0 && (
                <optgroup label="Custom">
                  {customModels.map((m) => (
                    <option key={modelKey(m)} value={modelKey(m)}>
                      {m.label}
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
          </div>
        )}
        {cliBackend && mcpStatus && (
          <div className="text-[10px] text-pm-muted truncate" title={mcpStatus}>
            {mcpStatus}
          </div>
        )}
      </div>

      <div className="flex-1 min-h-0 flex flex-col">
        {cliBackend ? (
          projectPath ? (
            <OrchestratorTerminal
              backend={cliBackend}
              pane={orchestratorPane}
              starting={orchestratorStarting}
              error={orchestratorError}
              subscribePaneData={orchestratorTunnel.subscribePaneData}
              transport={orchestratorTunnel.transport}
              syncPTYResize
              onRetry={() => {
                if (projectPath) void startOrchestratorPane(cliBackend, projectPath);
              }}
            />
          ) : (
            <div className="flex-1 min-h-0 flex items-center justify-center p-4 text-xs text-center text-pm-muted">
              Pick a project folder in the header to start the orchestrator agent.
            </div>
          )
        ) : (
          <div className="flex-1 min-h-0 overflow-auto p-3 space-y-2">
            {messages.length === 0 && (
              <div className="text-xs text-pm-muted">
                Ask the Puppet Master to coordinate your panes. Try &quot;spawn a claude pane and ask it to summarize the repo&quot;.
                Add custom models in Settings.
              </div>
            )}
            {messages.map((m) => (
              <div
                key={m.id}
                className={`text-xs whitespace-pre-wrap ${
                  m.role === 'user'
                    ? 'text-zinc-100'
                    : m.role === 'assistant'
                    ? 'text-zinc-200'
                    : 'text-pm-warn'
                }`}
              >
                <span className="text-pm-muted mr-1">[{m.role}]</span>
                {m.text}
              </div>
            ))}
          </div>
        )}

        <CoordinationPanel bridge={bridge} bridgeReady={bridgeReady} />

        <div
          role="separator"
          aria-orientation="horizontal"
          title="Resize MCP log"
          className="h-1 shrink-0 cursor-row-resize bg-pm-border/60 hover:bg-pm-accent transition-colors touch-none"
          onPointerDown={resizeMcpLog}
        />

        <div
          className="border-t border-pm-border overflow-auto shrink-0"
          style={{ height: mcpLogHeight }}
        >
          <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-pm-muted border-b border-pm-border bg-pm-bg/40">
            MCP log
          </div>
          {logs.length === 0 ? (
            <div className="px-3 py-1 text-[10px] text-pm-muted">No tool calls yet.</div>
          ) : (
            logs.slice(0, 30).map((l) => (
              <div key={l.id} className="px-3 py-0.5 text-[10px] font-mono">
                <span className="text-pm-muted">[{l.source}]</span>{' '}
                <span className={l.error ? 'text-pm-err' : 'text-pm-accent'}>{l.tool}</span>
                {l.error && <span className="text-pm-err"> — {l.error}</span>}
                {!l.error && l.result && (
                  <span className="text-pm-muted"> → {l.result.slice(0, 80)}</span>
                )}
              </div>
            ))
          )}
        </div>
      </div>

      {!cliBackend && (
        <div className="border-t border-pm-border p-2">
          {busy && standbyPanes.length > 0 && (
            <div
              className="text-[10px] text-pm-muted truncate mb-1"
              title={standbyPanes.map((p) => p.id).join(', ')}
            >
              Standing by for: {standbyPanes.map((p) => p.id).join(', ')}
            </div>
          )}
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                e.preventDefault();
                void send();
              }
            }}
            rows={2}
            placeholder="Ask the Puppet Master… (Ctrl+Enter to send)"
            className="w-full text-xs bg-pm-bg border border-pm-border rounded p-2 resize-none"
            disabled={busy}
          />
          <div className="flex justify-end gap-2 mt-1">
            {busy && (
              <button
                onClick={interrupt}
                className="px-2 py-1 text-xs rounded border border-pm-err/50 bg-pm-err/10 text-pm-err hover:bg-pm-err/20"
              >
                Interrupt
              </button>
            )}
            <button
              onClick={() => void send()}
              disabled={busy}
              className="px-2 py-1 text-xs rounded border border-pm-accent bg-pm-accent/20 text-pm-accent hover:bg-pm-accent/30 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {busy
                ? standbyPanes.length > 0
                  ? `Waiting on ${standbyPanes.length}…`
                  : 'Working…'
                : 'Send'}
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}
