import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
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
  isOrchestratorPaneId,
} from './lib/orchestrator-panes';


async function syncBridgePanes(
  client: BridgeClient,
  apply: (panes: PaneInfo[]) => void,
): Promise<void> {
  const list = await client.listPanes();
  apply(list);
}

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
  registry,
  orchestratorTunnel,
  settingsHydrated,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  settingsHydrated: boolean;
  chatEvents: OrchestratorChatEvent[];
  settings: PublicSettings;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  orchestratorTunnel: PaneTunnelApi;
}) {
  const backend = settings.orchestrator_backend ?? 'api';
  const cliBackend = isCliOrchestratorBackend(backend) ? backend : null;

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
    await bridge.patchSettings({ orchestrator_backend: next });
  };

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex-1 min-h-0 flex flex-col">
        {cliBackend ? (
          <OrchestratorTerminal
            backend={cliBackend}
            pane={orchestratorPaneView}
            starting={!orchestratorPane && bridgeReady && settingsHydrated}
            error={null}
            subscribePaneData={orchestratorTunnel.subscribePaneData}
            onRetry={() => void patchBackend(cliBackend)}
            transport={orchestratorTunnel.transport}
            syncPTYResize={false}
            mobileInputDelayMs={settings.mobile_input_delay_ms}
            mobileInputVisible={settings.mobile_input_visible}
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
  panes,
  bridgeReady,
  registry,
  mobileInputDelayMs,
  mobileInputVisible,
  selected,
  onSelect,
}: {
  panes: PaneInfo[];
  bridgeReady: boolean;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
  selected: string | null;
  onSelect: (paneId: string | null) => void;
}) {
  const selectedPane = selected ? registry.panes.get(selected) : undefined;
  const selectedTransport = useBridgePaneTransport(registry.makeTransport, selected ?? 'pane-placeholder');

  return (
    <div className="flex flex-col h-full min-h-0">
      {selected && selectedPane ? (
        <BridgePaneTerminal
          pane={selectedPane.info}
          status={selectedPane.status}
          subscribePaneData={registry.subscribePaneData}
          transport={selectedTransport}
          title={selectedPane.info.agent_type}
          mobileInputDelayMs={mobileInputDelayMs}
          mobileInputVisible={mobileInputVisible}
        />
      ) : (
      <div className="flex-1 min-h-0 overflow-y-auto pt-16">
        {!bridgeReady && (
          <div className="text-xs text-pm-muted text-center py-6">Connecting to desktop…</div>
        )}
        {bridgeReady && panes.length === 0 && (
          <div className="text-xs text-pm-muted text-center py-6 px-4">
            No panes yet. Open the menu to spawn an agent on your PC.
          </div>
        )}
        {panes.map((pane) => (
          <div
            key={pane.id}
            className={`flex items-center gap-1 border-b border-pm-border/50 ${selected === pane.id ? 'bg-pm-raised' : ''}`}
          >
            <button
              type="button"
              onClick={() => onSelect(pane.id)}
              className="flex-1 min-w-0 flex items-center gap-2 px-3 py-2.5 text-left hover:bg-pm-raised/60 text-xs"
            >
              <span className={`w-2 h-2 rounded-full flex-shrink-0 ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`} />
              <span className="font-mono text-zinc-200 truncate flex-1">{pane.agent_type}</span>
              <span className="text-pm-muted text-[10px]">{pane.status}</span>
            </button>
          </div>
        ))}
      </div>
      )}
    </div>
  );
}

function FloatingMobileMenu({
  open,
  onOpenChange,
  bridgeReady,
  tab,
  onTabChange,
  onEditConnection,
  settings,
  onPatchSettings,
  panes,
  selectedPaneId,
  onSelectPane,
  onRefreshPanes,
  onSpawnPane,
  onKillSelectedPane,
  spawning,
  spawnError,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  bridgeReady: boolean;
  tab: 'chat' | 'panes';
  onTabChange: (tab: 'chat' | 'panes') => void;
  onEditConnection: () => void;
  settings: PublicSettings;
  onPatchSettings: (patch: Partial<PublicSettings>) => Promise<void>;
  panes: PaneInfo[];
  selectedPaneId: string | null;
  onSelectPane: (paneId: string | null) => void;
  onRefreshPanes: () => Promise<void>;
  onSpawnPane: (agentType: AgentType) => Promise<void>;
  onKillSelectedPane: () => Promise<void>;
  spawning: boolean;
  spawnError: string | null;
}) {
  const models = DEFAULT_LLM_MODELS;
  const presets = listPresets();
  const backend = settings.orchestrator_backend ?? 'api';
  const activeModel =
    models.find((m) => m.provider === settings.default_provider && m.model_id === settings.default_model) ??
    models[0];

  const patchMobileInputDelay = async (value: number) => {
    const delayMs = Number.isFinite(value)
      ? value <= 0
        ? 0
        : Math.min(1000, Math.max(50, Math.round(value)))
      : 250;
    await onPatchSettings({ mobile_input_delay_ms: delayMs });
  };

  return (
    <div className="fixed top-[calc(10px+env(safe-area-inset-top,0px))] right-3 z-50 flex flex-col items-end gap-2">
      {open && (
        <div className="w-[min(calc(100vw-24px),22rem)] max-h-[calc(100dvh-92px)] overflow-y-auto rounded-lg border border-pm-border bg-pm-panel/95 p-3 text-xs shadow-2xl backdrop-blur">
          <div className="flex items-center gap-2 pb-2 border-b border-pm-border">
            <span
              className={`w-2 h-2 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`}
              title={bridgeReady ? 'Connected' : 'Connecting'}
            />
            <span className="font-semibold text-zinc-100 flex-1">Puppet Master</span>
            <button
              type="button"
              onClick={onEditConnection}
              className="rounded border border-pm-border px-2 py-1 text-pm-muted hover:bg-pm-border/40 hover:text-zinc-100"
            >
              Edit
            </button>
          </div>

          <div className="grid grid-cols-2 gap-2 py-3">
            <button
              type="button"
              onClick={() => onTabChange('chat')}
              className={`rounded border px-2 py-2 ${
                tab === 'chat'
                  ? 'border-pm-accent bg-pm-accent/15 text-pm-accent'
                  : 'border-pm-border text-pm-muted hover:bg-pm-border/40'
              }`}
            >
              Orchestrator
            </button>
            <button
              type="button"
              onClick={() => onTabChange('panes')}
              className={`rounded border px-2 py-2 ${
                tab === 'panes'
                  ? 'border-pm-accent bg-pm-accent/15 text-pm-accent'
                  : 'border-pm-border text-pm-muted hover:bg-pm-border/40'
              }`}
            >
              Panes
            </button>
          </div>

          {tab === 'chat' && (
            <div className="space-y-2 border-t border-pm-border pt-3">
              <label className="flex items-center gap-2 text-pm-muted">
                <span className="w-16 shrink-0">Backend</span>
                <select
                  value={backend}
                  disabled={!bridgeReady}
                  onChange={(e) => void onPatchSettings({ orchestrator_backend: e.target.value as OrchestratorBackend })}
                  className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-2 py-1 text-xs text-pm-text"
                >
                  {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((b) => (
                    <option key={b} value={b}>
                      {ORCHESTRATOR_BACKEND_LABELS[b]}
                    </option>
                  ))}
                </select>
              </label>
              {backend === 'api' && (
                <label className="flex items-center gap-2 text-pm-muted">
                  <span className="w-16 shrink-0">Model</span>
                  <select
                    value={modelKey(activeModel)}
                    disabled={!bridgeReady}
                    onChange={(e) => {
                      const found = models.find((m) => modelKey(m) === e.target.value);
                      if (found) void onPatchSettings({
                        default_provider: found.provider,
                        default_model: found.model_id,
                      });
                    }}
                    className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-2 py-1 text-xs text-pm-text"
                  >
                    {models.map((m) => (
                      <option key={modelKey(m)} value={modelKey(m)}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </div>
          )}

          {tab === 'panes' && (
            <div className="space-y-2 border-t border-pm-border pt-3">
              <label className="flex items-center gap-2 text-pm-muted">
                <span className="w-16 shrink-0">Pane</span>
                <select
                  value={selectedPaneId ?? ''}
                  disabled={!bridgeReady || panes.length === 0}
                  onChange={(e) => onSelectPane(e.target.value || null)}
                  className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-2 py-1 text-xs text-pm-text"
                >
                  <option value="">Pane list</option>
                  {panes.map((pane) => (
                    <option key={pane.id} value={pane.id}>
                      {pane.agent_type} · {pane.status}
                    </option>
                  ))}
                </select>
              </label>
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  onClick={() => void onRefreshPanes()}
                  className="rounded border border-pm-border px-2 py-2 text-pm-muted hover:bg-pm-border/40"
                >
                  Refresh
                </button>
                <button
                  type="button"
                  disabled={!selectedPaneId}
                  onClick={() => void onKillSelectedPane()}
                  className="rounded border border-pm-border px-2 py-2 text-pm-muted hover:bg-pm-border/40 disabled:opacity-40"
                >
                  Close
                </button>
              </div>
              {spawnError && (
                <div className="rounded border border-pm-err/40 bg-pm-err/5 p-2 text-[10px] text-pm-err">
                  {spawnError}
                </div>
              )}
              <div className="pt-1">
                <div className="pb-1 text-[10px] uppercase tracking-wide text-pm-muted">
                  New pane
                </div>
                <div className="grid grid-cols-2 gap-2">
                  {presets.map((preset) => (
                    <button
                      key={preset.type}
                      type="button"
                      disabled={!bridgeReady || spawning}
                      onClick={() => void onSpawnPane(preset.type)}
                      className="rounded border border-pm-border px-2 py-2 text-left text-zinc-100 hover:bg-pm-border/40 disabled:opacity-40"
                    >
                      <span className="block truncate">{preset.label}</span>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}

          <div className="space-y-2 border-t border-pm-border mt-3 pt-3">
            <label className="flex items-center justify-between gap-2 text-pm-muted">
              <span>Input box</span>
              <input
                type="checkbox"
                checked={settings.mobile_input_visible ?? true}
                disabled={!bridgeReady}
                onChange={(e) => void onPatchSettings({ mobile_input_visible: e.target.checked })}
                className="h-4 w-4 accent-pm-accent"
              />
            </label>
            <label className="flex items-center gap-2 text-pm-muted">
              <span className="w-16 shrink-0">Buffer</span>
              <input
                type="number"
                min={0}
                max={1000}
                step={50}
                value={settings.mobile_input_delay_ms ?? 250}
                disabled={!bridgeReady}
                onChange={(e) => void patchMobileInputDelay(Number(e.target.value))}
                className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-2 py-1 text-xs font-mono text-pm-text"
              />
              <span className="shrink-0">ms</span>
            </label>
          </div>
        </div>
      )}
      <button
        type="button"
        aria-label={open ? 'Close controls' : 'Open controls'}
        aria-expanded={open}
        onClick={() => onOpenChange(!open)}
        className="flex h-12 w-12 items-center justify-center rounded-full border border-pm-border bg-pm-accent text-pm-bg text-2xl leading-none shadow-2xl"
      >
        {open ? '×' : '⋯'}
      </button>
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
  const [settingsHydrated, setSettingsHydrated] = useState(false);
  const [tab, setTab] = useState<'chat' | 'panes'>('chat');
  const [editingUrl, setEditingUrl] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [selectedPaneId, setSelectedPaneId] = useState<string | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [spawnError, setSpawnError] = useState<string | null>(null);
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


  const orchestratorReady = useMemo(() => {
    if (!cliOrchestratorBackend) return true;
    return !!findOrchestratorPane(panes, cliOrchestratorBackend);
  }, [cliOrchestratorBackend, panes]);

  const mobileOrchestratorTunnel = usePaneTunnel(bridge, orchestratorPaneId, 'mobile');
  const orchestratorTunnelRef = useRef(mobileOrchestratorTunnel);
  orchestratorTunnelRef.current = mobileOrchestratorTunnel;

  useEffect(() => {
    if (!bridgeUrl) return;
    const c = makeBridgeClient(bridgeUrl);
    setBridge(c);
    setBridgeReady(false);
    setSettingsHydrated(false);

    const applyPanes = (list: PaneInfo[]) => {
      setPanes(list);
      registryRef.current.setPanesFromList(list);
    };

    fetch(`${bridgeUrl}/health`, {
      signal: AbortSignal.timeout(3000),
      headers: ngrokRequestHeaders(bridgeUrl),
    })
      .then((r) => { if (r.ok) setBridgeReady(true); })
      .catch(() => {});

    void c
      .getSettings()
      .then((next) => {
        setSettings(next);
        setSettingsHydrated(true);
        return syncBridgePanes(c, applyPanes);
      })
      .catch(() => {
        setSettingsHydrated(true);
      });

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
          if (isOrchestratorPaneId(ev.pane_id)) {
            routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
          } else {
            registryRef.current.ingestTerminalData(ev.pane_id, ev.data);
          }
        }
        if (ev.type === 'pane-status') {
          registryRef.current.updatePaneStatus(ev.pane_id, ev.status);
        }
        if (ev.type === 'pane-resize') {
          if (isOrchestratorPaneId(ev.pane_id)) {
            routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
          } else {
            registryRef.current.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows);
          }
        }
        if (ev.type === 'settings') {
          setSettings(ev.settings);
        }
      },
    );

    return unsub;
  }, [bridgeUrl]);

  useEffect(() => {
    if (tab !== 'panes') return;
    if (selectedPaneId && panes.some((pane) => pane.id === selectedPaneId)) return;
    setSelectedPaneId(panes[0]?.id ?? null);
  }, [panes, selectedPaneId, tab]);

  const patchSettings = useCallback(async (patch: Partial<PublicSettings>) => {
    if (!bridge) return;
    const updated = await bridge.patchSettings(patch);
    setSettings(updated);
  }, [bridge]);

  const refreshPanes = useCallback(async () => {
    if (!bridge) return;
    const list = await bridge.listPanes();
    setPanes(list);
    registryRef.current.setPanesFromList(list);
  }, [bridge]);

  const spawnPane = useCallback(async (agentType: AgentType) => {
    if (!bridge) return;
    setSpawning(true);
    setSpawnError(null);
    try {
      const { pane_id } = await bridge.spawnPane({ agent_type: agentType, cols: 120, rows: 30 });
      await refreshPanes();
      setSelectedPaneId(pane_id);
      setTab('panes');
      setMenuOpen(false);
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    } finally {
      setSpawning(false);
    }
  }, [bridge, refreshPanes]);

  const killSelectedPane = useCallback(async () => {
    if (!bridge || !selectedPaneId) return;
    setSpawnError(null);
    try {
      await bridge.killPane(selectedPaneId);
      setSelectedPaneId(null);
      await refreshPanes();
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    }
  }, [bridge, refreshPanes, selectedPaneId]);


  useEffect(() => {
    if (!bridge || !bridgeReady || !settingsHydrated || !cliOrchestratorBackend || orchestratorReady) {
      return;
    }

    let cancelled = false;
    const applyPanes = (list: PaneInfo[]) => {
      if (cancelled) return;
      setPanes(list);
      registryRef.current.setPanesFromList(list);
    };

    const tick = () => {
      void syncBridgePanes(bridge, applyPanes).catch(() => {});
    };

    tick();
    const id = window.setInterval(tick, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [bridge, bridgeReady, settingsHydrated, cliOrchestratorBackend, orchestratorReady]);

  if (!bridgeUrl || editingUrl) {
    return (
      <div className="pwa-shell bg-pm-bg text-zinc-100">
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
    <div className="pwa-shell bg-pm-bg text-zinc-100">
      <main className="pwa-main" aria-label="Puppet Master mobile workspace">
        {bridge && tab === 'chat' && (
          <section className="pwa-stage" aria-label="Orchestrator">
            <OrchestratorTab
              bridge={bridge}
              chatEvents={chatEvents}
              bridgeReady={bridgeReady}
              settings={settings}
              registry={registry}
              orchestratorTunnel={mobileOrchestratorTunnel}
              settingsHydrated={settingsHydrated}
            />
          </section>
        )}
        {bridge && tab === 'panes' && (
          <section className="pwa-stage" aria-label="Panes">
            <PanesTab
              panes={panes}
              bridgeReady={bridgeReady}
              registry={registry}
              mobileInputDelayMs={settings.mobile_input_delay_ms}
              mobileInputVisible={settings.mobile_input_visible}
              selected={selectedPaneId}
              onSelect={setSelectedPaneId}
            />
          </section>
        )}
        {!bridge && (
          <div className="flex items-center justify-center h-full text-xs text-pm-muted">
            Connecting…
          </div>
        )}
      </main>
      <FloatingMobileMenu
        open={menuOpen}
        onOpenChange={setMenuOpen}
        bridgeReady={bridgeReady}
        tab={tab}
        onTabChange={(next) => {
          setTab(next);
          setMenuOpen(false);
        }}
        onEditConnection={() => {
          setEditingUrl(true);
          setMenuOpen(false);
        }}
        settings={settings}
        onPatchSettings={patchSettings}
        panes={panes}
        selectedPaneId={selectedPaneId}
        onSelectPane={(paneId) => {
          setSelectedPaneId(paneId);
          setMenuOpen(false);
        }}
        onRefreshPanes={refreshPanes}
        onSpawnPane={spawnPane}
        onKillSelectedPane={killSelectedPane}
        spawning={spawning}
        spawnError={spawnError}
      />
    </div>
  );
}
