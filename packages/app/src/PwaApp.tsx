import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
  type LlmModel,
  type OrchestratorBackend,
  type OrchestratorChatEvent,
  type PaneInfo,
} from '@puppet-master/shared';
import { listPresets, type AgentType } from '@puppet-master/shared';
import { BridgePaneTerminal, useBridgePaneTransport } from './components/BridgePaneTerminal';
import { OrchestratorTerminal } from './components/OrchestratorTerminal';
import { useBridgePaneRegistry } from './hooks/useBridgePaneRegistry';
import { usePaneTunnel, type PaneTunnelApi } from './hooks/usePaneTunnel';
import { makeBridgeClient, subscribeBridgeEvents, type BridgeClient } from './lib/bridge';
import { routeBridgeEventToPaneTunnel } from './lib/bridge-pane-tunnel';
import { DEFAULT_PUBLIC_SETTINGS, type PublicSettings } from './lib/bridge-settings';
import { ngrokRequestHeaders } from './lib/bridge-ngrok';
import { LS_BRIDGE_URL, resolveBridgeBaseUrl, shouldUseSameOriginBridgeProxy } from './lib/bridge-url';
import {
  applyOrchestratorChatEvent,
  chatEventToMcpLog,
  type ChatLine,
  type McpLogLine,
} from './lib/orchestrator-chat';
import {
  findOrchestratorPane,
  isCliOrchestratorBackend,
} from './lib/orchestrator-panes';

interface DevInfo {
  tunnelUrl?: string | null;
  ngrokUrl?: string | null;
  bridgeProxyUrl?: string;
  tunnelProvider?: string | null;
}

// ---- Connection setup screen ----

function SetupScreen({ onConnect }: { onConnect: (url: string) => void }) {
  const [url, setUrl] = useState(() => resolveBridgeBaseUrl(localStorage.getItem(LS_BRIDGE_URL), window.location));
  const [testing, setTesting] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [devInfo, setDevInfo] = useState<DevInfo | null>(null);

  useEffect(() => {
    fetch('/__puppet_master_dev__.json', { cache: 'no-store' })
      .then((res) => (res.ok ? res.json() as Promise<DevInfo> : null))
      .then((info) => {
        const publicUrl = info?.tunnelUrl ?? info?.ngrokUrl;
        if (publicUrl) setDevInfo({ ...info, tunnelUrl: publicUrl });
      })
      .catch(() => {});
  }, []);

  const handleConnect = async () => {
    setTesting(true);
    setErr(null);
    const base = url.replace(/\/$/, '');
    try {
      const res = await fetch(`${base}/health`, {
        signal: AbortSignal.timeout(4000),
        headers: ngrokRequestHeaders(base),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      localStorage.setItem(LS_BRIDGE_URL, base);
      onConnect(base);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  const useTunnelBridge = () => {
    if (!devInfo?.bridgeProxyUrl) return;
    setUrl(devInfo.bridgeProxyUrl);
  };

  return (
    <div className="flex flex-col items-center justify-center h-full p-6 gap-4 text-sm">
      <img src="/app-icon.svg" alt="Puppet Master" className="w-16 h-16 rounded-xl" />
      <h1 className="text-lg font-semibold">Puppet Master</h1>
      <p className="text-pm-muted text-center text-xs max-w-xs">
        {shouldUseSameOriginBridgeProxy(window.location)
          ? 'Remote access — bridge uses this page\'s /bridge proxy. Keep Puppet Master running on your PC.'
          : 'Run npm run dev on your PC — a public tunnel URL prints automatically (like Expo). Open it on your phone.'}
      </p>
      {devInfo?.tunnelUrl && !shouldUseSameOriginBridgeProxy(window.location) && (
        <div className="w-full max-w-xs rounded border border-pm-border bg-pm-raised/40 p-3 text-xs space-y-2">
          <div className="text-pm-muted uppercase tracking-wide text-[10px]">Phone / other network</div>
          <a
            href={devInfo.tunnelUrl}
            className="block text-pm-accent break-all hover:underline"
            target="_blank"
            rel="noreferrer"
          >
            {devInfo.tunnelUrl}
          </a>
          {devInfo.tunnelProvider && (
            <div className="text-[10px] text-pm-muted">via {devInfo.tunnelProvider}</div>
          )}
          {devInfo.bridgeProxyUrl && (
            <button
              type="button"
              onClick={useTunnelBridge}
              className="text-[10px] text-pm-muted hover:text-zinc-200 underline"
            >
              Use tunnel bridge URL for testing on this machine
            </button>
          )}
        </div>
      )}
      <input
        type="url"
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        placeholder={shouldUseSameOriginBridgeProxy(window.location) ? 'https://your-tunnel.example.com/bridge' : 'http://127.0.0.1:17321'}
        className="w-full max-w-xs bg-pm-bg border border-pm-border rounded px-3 py-2 text-xs text-zinc-100 focus:outline-none focus:border-pm-accent"
      />
      {err && <p className="text-pm-err text-xs">{err}</p>}
      <button
        onClick={() => void handleConnect()}
        disabled={testing}
        className="px-4 py-2 rounded bg-pm-accent/20 border border-pm-accent text-pm-accent text-xs disabled:opacity-50"
      >
        {testing ? 'Connecting…' : 'Connect'}
      </button>
    </div>
  );
}

// ---- Orchestrator (mirrors desktop sidebar) ----

function OrchestratorTab({
  bridge,
  bridgeReady,
  chatEvents,
  settings,
  onSettings,
  registry,
  orchestratorTunnel,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  chatEvents: OrchestratorChatEvent[];
  settings: PublicSettings;
  onSettings: (next: PublicSettings) => void;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  orchestratorTunnel: PaneTunnelApi;
}) {
  const backend = settings.orchestrator_backend ?? 'api';
  const cliBackend = isCliOrchestratorBackend(backend) ? backend : null;
  const models = DEFAULT_LLM_MODELS;
  const activeModel =
    models.find((m) => m.provider === settings.default_provider && m.model_id === settings.default_model) ??
    models[0];

  const orchestratorPane = useMemo(() => {
    if (!cliBackend) return undefined;
    const info = findOrchestratorPane(registry.paneList.map((p) => p.info), cliBackend);
    if (!info) return undefined;
    return registry.panes.get(info.id);
  }, [cliBackend, registry.paneList, registry.panes]);

  const orchestratorPaneView = useMemo(() => {
    if (!orchestratorPane) return undefined;
    const info = orchestratorTunnel.mergePaneInfo(orchestratorPane.info) ?? orchestratorPane.info;
    return { ...orchestratorPane, info };
  }, [orchestratorPane, orchestratorTunnel.mergePaneInfo]);

  const patchBackend = async (next: OrchestratorBackend) => {
    const updated = await bridge.patchSettings({ orchestrator_backend: next });
    onSettings(updated);
  };

  const patchModel = async (model: LlmModel) => {
    const updated = await bridge.patchSettings({
      default_provider: model.provider,
      default_model: model.model_id,
    });
    onSettings(updated);
  };

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex flex-col gap-1 px-3 py-2 border-b border-pm-border text-xs shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-pm-muted shrink-0">Backend:</span>
          <select
            value={backend}
            disabled={!bridgeReady}
            onChange={(e) => void patchBackend(e.target.value as OrchestratorBackend)}
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
              value={modelKey(activeModel)}
              disabled={!bridgeReady}
              onChange={(e) => {
                const found = models.find((m) => modelKey(m) === e.target.value);
                if (found) void patchModel(found);
              }}
              className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs"
            >
              {models.map((m) => (
                <option key={modelKey(m)} value={modelKey(m)}>
                  {m.label}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      <div className="flex-1 min-h-0 flex flex-col">
        {cliBackend ? (
          <OrchestratorTerminal
            backend={cliBackend}
            pane={orchestratorPaneView}
            starting={!orchestratorPane && bridgeReady}
            error={null}
            subscribePaneData={orchestratorTunnel.subscribePaneData}
            onRetry={() => void patchBackend(cliBackend)}
            transport={orchestratorTunnel.transport}
            syncPTYResize={false}
            mobileInputDelayMs={settings.mobile_input_delay_ms}
          />
        ) : (
          <ChatTab bridge={bridge} chatEvents={chatEvents} bridgeReady={bridgeReady} />
        )}
      </div>
    </div>
  );
}

// ---- Chat (API backend) ----

function ChatTab({
  bridge,
  chatEvents,
  bridgeReady,
}: {
  bridge: BridgeClient;
  chatEvents: OrchestratorChatEvent[];
  bridgeReady: boolean;
}) {
  const [lines, setLines] = useState<ChatLine[]>([]);
  const [logs, setLogs] = useState<McpLogLine[]>([]);
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const seenCount = useRef(0);

  useEffect(() => {
    const newEvents = chatEvents.slice(seenCount.current);
    if (newEvents.length === 0) return;
    seenCount.current = chatEvents.length;
    setLines((prev) => {
      let next = prev;
      for (const ev of newEvents) next = applyOrchestratorChatEvent(ev, next);
      return next;
    });
    setLogs((prev) => {
      const fresh = newEvents
        .map(chatEventToMcpLog)
        .filter((entry): entry is McpLogLine => entry !== null);
      return [...fresh, ...prev].slice(0, 50);
    });
  }, [chatEvents]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [lines]);

  const sendMessage = useCallback(async () => {
    const text = draft.trim();
    if (!text || sending || !bridgeReady) return;
    const messageId = crypto.randomUUID();
    setDraft('');
    setSending(true);
    setLines((prev) => applyOrchestratorChatEvent(
      { type: 'user', message_id: messageId, text },
      prev,
    ));
    try {
      await bridge.postOrchestratorMessage(text, messageId);
    } catch (e) {
      setLines((prev) => [
        ...prev,
        {
          id: `${messageId}-send-err`,
          role: 'error',
          text: e instanceof Error ? e.message : String(e),
        },
      ]);
    } finally {
      setSending(false);
    }
  }, [bridge, bridgeReady, draft, sending]);

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex-1 min-h-0 overflow-y-auto p-3 space-y-2">
        {!bridgeReady && (
          <div className="text-xs text-pm-muted text-center py-4">Connecting to desktop…</div>
        )}
        {lines.length === 0 && bridgeReady && (
          <div className="text-xs text-pm-muted">
            Ask the Puppet Master to coordinate your panes — same as the desktop sidebar.
          </div>
        )}
        {lines.map((line) => (
          <div
            key={line.id}
            className={`text-xs whitespace-pre-wrap ${
              line.role === 'user'
                ? 'text-zinc-100'
                : line.role === 'assistant'
                ? 'text-zinc-200'
                : line.role === 'tool'
                ? 'text-pm-muted font-mono'
                : line.role === 'error' || line.role === 'system'
                ? 'text-pm-warn'
                : 'text-pm-muted'
            }`}
          >
            <span className="text-pm-muted mr-1">[{line.role}]</span>
            {line.role === 'assistant' && line.streaming ? `${line.text}▋` : line.text}
          </div>
        ))}
        <div ref={bottomRef} />
      </div>

      <div className="border-t border-pm-border max-h-28 overflow-auto shrink-0">
        <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-pm-muted border-b border-pm-border bg-pm-bg/40">
          MCP log
        </div>
        {logs.length === 0 ? (
          <div className="px-3 py-1 text-[10px] text-pm-muted">No tool calls yet.</div>
        ) : (
          logs.slice(0, 20).map((log) => (
            <div key={log.id} className="px-3 py-0.5 text-[10px] font-mono">
              <span className={log.error ? 'text-pm-err' : 'text-pm-accent'}>{log.tool}</span>
              {log.text && <span className="text-pm-muted"> → {log.text}</span>}
            </div>
          ))
        )}
      </div>

      <div className="border-t border-pm-border p-2 shrink-0">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void sendMessage();
            }
          }}
          rows={2}
          placeholder="Ask the Puppet Master… (Ctrl+Enter)"
          className="w-full text-xs bg-pm-bg border border-pm-border rounded p-2 resize-none text-zinc-100 focus:outline-none focus:border-pm-accent"
          disabled={sending || !bridgeReady}
        />
        <div className="flex justify-end mt-1">
          <button
            onClick={() => void sendMessage()}
            disabled={sending || !bridgeReady || !draft.trim()}
            className="px-3 py-1 text-xs rounded border border-pm-accent bg-pm-accent/20 text-pm-accent disabled:opacity-40"
          >
            {sending ? 'Working…' : 'Send'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ---- Panes Tab ----

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-accent',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

function PanesTab({
  bridge,
  panes,
  bridgeReady,
  registry,
  mobileInputDelayMs,
}: {
  bridge: BridgeClient;
  panes: PaneInfo[];
  bridgeReady: boolean;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  mobileInputDelayMs?: number;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [showNewPane, setShowNewPane] = useState(false);
  const [spawning, setSpawning] = useState(false);
  const [spawnError, setSpawnError] = useState<string | null>(null);
  const presets = listPresets();

  const refresh = useCallback(async () => {
    try {
      const list = await bridge.listPanes();
      registry.setPanesFromList(list);
    } catch {
      /* ignore */
    }
  }, [bridge, registry]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selectedPane = selected ? registry.panes.get(selected) : undefined;
  const selectedTransport = useBridgePaneTransport(registry.makeTransport, selected ?? 'pane-placeholder');

  const spawnPane = useCallback(async (agentType: AgentType) => {
    setShowNewPane(false);
    setSpawning(true);
    setSpawnError(null);
    try {
      const { pane_id } = await bridge.spawnPane({ agent_type: agentType, cols: 120, rows: 30 });
      await refresh();
      setSelected(pane_id);
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    } finally {
      setSpawning(false);
    }
  }, [bridge, refresh]);

  const killPane = useCallback(async (paneId: string) => {
    try {
      await bridge.killPane(paneId);
      if (selected === paneId) {
        setSelected(null);
      }
      await refresh();
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    }
  }, [bridge, refresh, selected]);

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex items-center justify-between gap-2 px-3 py-2 border-b border-pm-border">
        <span className="text-xs text-pm-muted">{panes.length} pane{panes.length !== 1 ? 's' : ''}</span>
        <div className="flex items-center gap-2">
          <div className="relative">
            <button
              type="button"
              onClick={() => setShowNewPane((open) => !open)}
              disabled={!bridgeReady || spawning}
              className="text-xs text-pm-accent px-2 py-0.5 rounded border border-pm-accent/50 bg-pm-accent/10 disabled:opacity-40"
            >
              {spawning ? 'Spawning…' : '+ New'}
            </button>
            {showNewPane && (
              <div className="absolute top-full right-0 mt-1 z-20 w-52 max-h-64 overflow-y-auto rounded-md border border-pm-border bg-pm-panel shadow-lg">
                {presets.map((preset) => (
                  <button
                    key={preset.type}
                    type="button"
                    onClick={() => void spawnPane(preset.type)}
                    className="w-full text-left px-3 py-2 text-xs hover:bg-pm-border/40 border-b border-pm-border/30 last:border-b-0"
                  >
                    <div className="font-medium text-zinc-100">{preset.label}</div>
                    <div className="text-[10px] text-pm-muted truncate">{preset.description}</div>
                  </button>
                ))}
              </div>
            )}
          </div>
          <button
            type="button"
            onClick={() => void refresh()}
            className="text-xs text-pm-muted px-2 py-0.5 rounded border border-pm-border hover:bg-pm-border/40"
          >
            Refresh
          </button>
        </div>
      </div>
      {spawnError && (
        <div className="px-3 py-1.5 text-[10px] text-pm-err border-b border-pm-border bg-pm-err/5">
          {spawnError}
        </div>
      )}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {!bridgeReady && (
          <div className="text-xs text-pm-muted text-center py-6">Connecting to desktop…</div>
        )}
        {bridgeReady && panes.length === 0 && (
          <div className="text-xs text-pm-muted text-center py-6 px-4">
            No panes yet. Tap <span className="text-pm-accent">+ New</span> to spawn an agent on your PC.
          </div>
        )}
        {panes.map((pane) => (
          <div
            key={pane.id}
            className={`flex items-center gap-1 border-b border-pm-border/50 ${selected === pane.id ? 'bg-pm-raised' : ''}`}
          >
            <button
              type="button"
              onClick={() => setSelected(pane.id)}
              className="flex-1 min-w-0 flex items-center gap-2 px-3 py-2.5 text-left hover:bg-pm-raised/60 text-xs"
            >
              <span className={`w-2 h-2 rounded-full flex-shrink-0 ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`} />
              <span className="font-mono text-zinc-200 truncate flex-1">{pane.agent_type}</span>
              <span className="text-pm-muted text-[10px]">{pane.status}</span>
            </button>
            <button
              type="button"
              onClick={() => void killPane(pane.id)}
              className="shrink-0 px-2 py-2 text-[10px] text-pm-muted hover:text-pm-err"
              title="Close pane"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
      {selected && selectedPane && (
        <div className="border-t border-pm-border flex flex-col min-h-[40vh] max-h-[55vh]">
          <BridgePaneTerminal
            pane={selectedPane.info}
            status={selectedPane.status}
            subscribePaneData={registry.subscribePaneData}
            transport={selectedTransport}
            title={selectedPane.info.agent_type}
            mobileInputDelayMs={mobileInputDelayMs}
          />
        </div>
      )}
    </div>
  );
}

// ---- Root PWA shell ----

export default function PwaApp() {
  const [bridgeUrl, setBridgeUrl] = useState<string | null>(() =>
    resolveBridgeBaseUrl(localStorage.getItem(LS_BRIDGE_URL), window.location),
  );
  const [bridge, setBridge] = useState<BridgeClient | null>(null);
  const [bridgeReady, setBridgeReady] = useState(false);
  const [chatEvents, setChatEvents] = useState<OrchestratorChatEvent[]>([]);
  const [panes, setPanes] = useState<PaneInfo[]>([]);
  const [settings, setSettings] = useState<PublicSettings>(DEFAULT_PUBLIC_SETTINGS);
  const [tab, setTab] = useState<'chat' | 'panes'>('chat');
  const [editingUrl, setEditingUrl] = useState(false);
  const registry = useBridgePaneRegistry(bridge);
  const registryRef = useRef(registry);
  registryRef.current = registry;

  const orchestratorBackend = settings.orchestrator_backend ?? 'api';
  const cliOrchestratorBackend = isCliOrchestratorBackend(orchestratorBackend)
    ? orchestratorBackend
    : null;
  const orchestratorPaneId = useMemo(() => {
    if (!cliOrchestratorBackend) return null;
    const info = findOrchestratorPane(panes, cliOrchestratorBackend);
    return info?.id ?? null;
  }, [cliOrchestratorBackend, panes]);

  const mobileOrchestratorTunnel = usePaneTunnel(bridge, orchestratorPaneId, 'mobile');
  const orchestratorTunnelRef = useRef(mobileOrchestratorTunnel);
  orchestratorTunnelRef.current = mobileOrchestratorTunnel;

  useEffect(() => {
    if (!bridgeUrl) return;
    const c = makeBridgeClient(bridgeUrl);
    setBridge(c);
    setBridgeReady(false);

    fetch(`${bridgeUrl}/health`, {
      signal: AbortSignal.timeout(3000),
      headers: ngrokRequestHeaders(bridgeUrl),
    })
      .then((r) => { if (r.ok) setBridgeReady(true); })
      .catch(() => {});

    void c.getSettings().then(setSettings).catch(() => {});

    const unsub = subscribeBridgeEvents(
      bridgeUrl,
      (ev) => {
        if (ev.type === 'chat') {
          setChatEvents((prev) => [...prev, ev.event].slice(-500));
        }
        if (ev.type === 'panes') {
          setPanes(ev.panes);
          registryRef.current.setPanesFromList(ev.panes);
        }
        if (ev.type === 'terminal') {
          registryRef.current.ingestTerminalData(ev.pane_id, ev.data);
          routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
        }
        if (ev.type === 'pane-status') {
          registryRef.current.updatePaneStatus(ev.pane_id, ev.status);
        }
        if (ev.type === 'pane-resize') {
          registryRef.current.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows);
          routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
        }
        if (ev.type === 'settings') {
          setSettings(ev.settings);
        }
      },
    );

    return unsub;
  }, [bridgeUrl]);

  if (!bridgeUrl || editingUrl) {
    return (
      <div className="h-full bg-pm-bg text-zinc-100">
        <SetupScreen
          onConnect={(url) => {
            setBridgeUrl(url);
            setEditingUrl(false);
          }}
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full bg-pm-bg text-zinc-100">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-pm-border bg-pm-panel">
        <img src="/app-icon.svg" alt="" className="w-5 h-5 rounded" />
        <span className="text-sm font-semibold flex-1">Puppet Master</span>
        <span
          className={`w-1.5 h-1.5 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`}
          title={bridgeReady ? 'Connected' : 'Connecting…'}
        />
        <button
          onClick={() => setEditingUrl(true)}
          className="text-[10px] text-pm-muted px-1.5 py-0.5 rounded hover:bg-pm-border/40"
        >
          Edit
        </button>
      </div>

      <div className="flex border-b border-pm-border bg-pm-panel">
        {(['chat', 'panes'] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`flex-1 py-2 text-xs capitalize ${
              tab === t
                ? 'border-b-2 border-pm-accent text-pm-accent'
                : 'text-pm-muted hover:text-zinc-100'
            }`}
          >
            {t === 'chat' ? 'orchestrator' : t}
          </button>
        ))}
      </div>

      <div className="flex-1 min-h-0">
        {bridge && tab === 'chat' && (
          <OrchestratorTab
            bridge={bridge}
            chatEvents={chatEvents}
            bridgeReady={bridgeReady}
            settings={settings}
            onSettings={setSettings}
            registry={registry}
            orchestratorTunnel={mobileOrchestratorTunnel}
          />
        )}
        {bridge && tab === 'panes' && (
          <PanesTab
            bridge={bridge}
            panes={panes}
            bridgeReady={bridgeReady}
            registry={registry}
            mobileInputDelayMs={settings.mobile_input_delay_ms}
          />
        )}
        {!bridge && (
          <div className="flex items-center justify-center h-full text-xs text-pm-muted">
            Connecting…
          </div>
        )}
      </div>
    </div>
  );
}
