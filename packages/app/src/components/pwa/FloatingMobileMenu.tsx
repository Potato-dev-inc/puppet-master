// Floating "..." menu of the mobile PWA: tab switch, orchestrator backend/model, pane management, input settings.
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
  type AgentType,
  type OrchestratorBackend,
  type PaneInfo,
} from '@puppet-master/shared';
import { launchMenuEntries, useAgentRegistry } from '../../lib/agentRegistry';
import type { PublicSettings } from '../../lib/bridge-settings';

export function FloatingMobileMenu({
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
  const presets = launchMenuEntries(useAgentRegistry());
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
                      disabled={!bridgeReady || spawning || preset.disabled}
                      title={preset.disabledReason}
                      onClick={() => void onSpawnPane(preset.type)}
                      className="rounded border border-pm-border px-2 py-2 text-left text-zinc-100 hover:bg-pm-border/40 disabled:opacity-40"
                    >
                      <span className="block truncate">{preset.label}</span>
                      {preset.experimental && <span className="block text-[10px] text-pm-warn">experimental</span>}
                      {preset.disabled && <span className="block text-[10px] text-pm-muted">Headless only</span>}
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
