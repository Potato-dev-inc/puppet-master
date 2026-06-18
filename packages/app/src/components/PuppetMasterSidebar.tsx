import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
  type LlmModel,
  type McpLogEntry,
  type OrchestratorBackend,
  type OrchestratorChatEvent,
} from '@puppet-master/shared';
import type { BridgeClient } from '../lib/bridge';
import type { ChatMessage } from '../lib/llm';
import { runPuppetMasterLoop } from '../lib/puppet-master';
import { runPuppetMasterCliLoop } from '../lib/puppet-master-cli';
import { makeTauriExecutor } from '../lib/mcp-tools';
import { findModel, getApiKey, invalidateSettingsCache, listModels, loadSettings, saveSettings } from '../lib/settings';
import { tauri } from '../lib/tauri';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import {
  ensureOrchestratorPane,
  findOrchestratorPane,
  isCliOrchestratorBackend,
  type CliOrchestratorBackend,
} from '../lib/orchestrator-panes';
import { OrchestratorTerminal } from './OrchestratorTerminal';

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

const PLANNED_FEATURES = [
  'Live sidebar delegation to Codex, Claude Code, and OpenCode panes',
  'Model-aware task routing by detected model and smartness score',
  'Agent context reader with pane summaries and handoff notes',
  'Resizable draggable workspace layouts with saved presets',
];

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
  const abortRef = useRef<AbortController | null>(null);

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

  const startOrchestratorPane = useCallback(async (activeBackend: CliOrchestratorBackend, cwd: string) => {
    setOrchestratorStarting(true);
    setOrchestratorError(null);
    setMcpStatus(null);
    try {
      await ensureOrchestratorPane(activeBackend, cwd);
      setMcpStatus('Puppet Master MCP ready');
      await registry.refresh();
    } catch (err) {
      setOrchestratorError(err instanceof Error ? err.message : String(err));
    } finally {
      setOrchestratorStarting(false);
    }
  }, [registry]);

  useEffect(() => {
    if (!cliBackend || !projectPath) {
      setOrchestratorError(null);
      setOrchestratorStarting(false);
      return;
    }
    if (orchestratorPane?.info.cwd === projectPath && orchestratorPane.status !== 'error') {
      return;
    }
    void startOrchestratorPane(cliBackend, projectPath);
  }, [cliBackend, projectPath, orchestratorPane?.info.cwd, orchestratorPane?.status, startOrchestratorPane]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const refreshFromSettings = useCallback(async () => {
    const s = await loadSettings();
    const models = listModels(s);
    setAvailableModels(models);
    setBackend(s.orchestrator_backend ?? 'api');
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
        if (cwd) void startOrchestratorPane(payload.backend as CliOrchestratorBackend, cwd);
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
          makeTauriExecutor(),
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
              abortRef.current = null;
              pushChat({ type: 'done', message_id: msg.message_id });
            },
            onError: (err) => {
              setMessages((m) => [
                ...m,
                { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
              ]);
              setBusy(false);
              abortRef.current = null;
              pushChat({ type: 'error', message_id: msg.message_id, error: err.message });
            },
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
        makeTauriExecutor(),
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
            abortRef.current = null;
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
            ]);
            setBusy(false);
            abortRef.current = null;
          },
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
  }, []);

  const newChat = useCallback(() => {
    abortRef.current?.abort();
    setBusy(false);
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
          onClick={() => setTheme((value) => (value === 'dark' ? 'light' : 'dark'))}
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

      <div className="px-3 py-2 border-b border-pm-border bg-pm-bg/35">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-[10px] uppercase tracking-wide text-pm-muted font-semibold">
            Planned orchestration
          </h2>
          <span className="text-[10px] text-pm-muted">Sidebar roadmap</span>
        </div>
        <div className="mt-2 grid gap-1">
          {PLANNED_FEATURES.map((feature) => (
            <div
              key={feature}
              className="rounded border border-pm-border bg-pm-raised px-2 py-1.5 text-[11px] leading-snug text-pm-text"
            >
              {feature}
            </div>
          ))}
        </div>
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
              if (isCliOrchestratorBackend(next)) {
                const cwd = projectPath ?? (await tauri.getProjectPath());
                if (cwd) void startOrchestratorPane(next, cwd);
              }
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
              subscribePaneData={registry.subscribePaneData}
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

        <div className="border-t border-pm-border max-h-32 overflow-auto">
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
              {busy ? 'Working…' : 'Send'}
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}
