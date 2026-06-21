import { useEffect, useState } from 'react';
import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { LlmModel, LlmProvider, OrchestratorBackend, Settings } from '@puppet-master/shared';
import { ORCHESTRATOR_BACKEND_LABELS } from '@puppet-master/shared';
import {
  clampSidebarWidth,
  isSidebarWidthPreset,
  listModels,
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  resolveSettingsFilePath,
  SIDEBAR_WIDTH_PRESET_LABELS,
  SIDEBAR_WIDTH_PRESETS,
} from '../../lib/settings';
import { installGlobalNpmMcpConfigs, installNpmMcpConfigs, uninstallGlobalNpmMcpConfigs, uninstallNpmMcpConfigs, getMcpStatus, PUPPET_MASTER_MCP_COMMAND, type EnsureMcpResult, type McpStatusReport } from '../../lib/mcp-config';
import type { UpdateCheckResult } from '../../lib/app-update';
import { MobilePairingPanel } from '../MobilePairingPanel';
import { parseDevServerPort } from '../../lib/public-bridge-url';
import { tauri, type AppInstallInfo, type CoordinationStorageInfo } from '../../lib/tauri';
import { ask } from '@tauri-apps/plugin-dialog';
import {
  CodeBlock,
  FieldInput,
  FieldLabel,
  FieldSelect,
  InfoCard,
  MockListCard,
  SettingBlock,
  SettingToggle,
  SettingsSection,
  StorageBarMock,
  type SettingsTabId,
} from './settings-ui';

export const EMPTY_CUSTOM: LlmModel = {
  provider: 'openrouter',
  model_id: '',
  label: '',
};

const CLAUDE_DESKTOP_MCP = JSON.stringify({ mcpServers: { 'puppet-master': PUPPET_MASTER_MCP_COMMAND } }, null, 2);
const CODEX_MCP = "[mcp_servers.puppet-master]\ncommand = \"npx\"\nargs = [\"-y\",\"@puppet-master/mcp\"]";

export interface SettingsTabContext {
  settings: Settings;
  setSettings: (next: Settings) => void;
  projectPath: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  bridgeUrl: string | null;
  draftCustom: LlmModel;
  setDraftCustom: (next: LlmModel) => void;
  onSidebarWidthChange?: (width: number) => void;
  /** Live workspace sidebar width (e.g. after drag resize). */
  currentSidebarWidth?: number;
  updateCheck?: UpdateCheckResult | null;
  updateChecking?: boolean;
  onCheckForUpdates?: () => void;
  onOpenRelease?: () => void;
}

export function SettingsTabPanel({ tab, ctx }: { tab: SettingsTabId; ctx: SettingsTabContext }) {
  switch (tab) {
    case 'general': return <GeneralTab ctx={ctx} />;
    case 'appearance': return <AppearanceTab ctx={ctx} />;
    case 'session': return <SessionTab ctx={ctx} />;
    case 'orchestrator': return <OrchestratorTab ctx={ctx} />;
    case 'api': return <ApiTab ctx={ctx} />;
    case 'mcp': return <McpTab ctx={ctx} />;
    case 'mobile': return <MobileTab ctx={ctx} />;
    case 'security': return <SecurityTab />;
    case 'storage': return <StorageTab ctx={ctx} />;
    case 'notifications': return <NotificationsTab />;
    case 'backup': return <BackupTab />;
    case 'marketplace': return <MarketplaceTab />;
    case 'rules': return <RulesTab />;
    case 'costs': return <CostsTab />;
    case 'team': return <TeamTab />;
    case 'automation': return <AutomationTab />;
    case 'observability': return <ObservabilityTab />;
    case 'plugins': return <PluginsTab />;
    case 'developer': return <DeveloperTab ctx={ctx} />;
    case 'advanced': return <AdvancedTab ctx={ctx} />;
    default: { const _exhaustive: never = tab; return _exhaustive; }
  }
}

function GeneralTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, projectPath, onProjectPathChange, updateCheck, updateChecking, onCheckForUpdates, onOpenRelease } = ctx;
  const [settingsPath, setSettingsPath] = useState('settings.json');
  const [installInfo, setInstallInfo] = useState<AppInstallInfo | null>(null);
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  useEffect(() => { void tauri.getAppInstallInfo().then(setInstallInfo); }, []);
  const pickPath = async () => {
    if (!onProjectPathChange) return;
    const result = await openDialog({ directory: true, multiple: false, defaultPath: (await homeDir()) ?? projectPath ?? undefined });
    if (typeof result === 'string') {
      await onProjectPathChange(result);
      setSettings({ ...settings, project_path: result });
    }
  };
  return (
    <SettingsSection title="General" description="Launch behavior, defaults, and product preferences.">
      <InfoCard title="Settings file" description={settingsPath} implemented />
      <SettingBlock label="Default project folder" implemented description="Workspace root for new panes and orchestrator tools.">
        <div className="flex gap-2">
          <FieldInput value={projectPath ?? settings.project_path ?? ''} onChange={(e) => setSettings({ ...settings, project_path: e.target.value })} className="font-mono" placeholder="~/work/my-project" />
          <button type="button" onClick={() => void pickPath()} className="shrink-0 rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Browse</button>
        </div>
      </SettingBlock>
      <SettingBlock label="Theme" implemented description="Desktop chrome theme (stored in settings.json).">
        <FieldSelect
          value={settings.theme ?? 'dark'}
          onChange={(e) => setSettings({ ...settings, theme: e.target.value as Settings['theme'] })}
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </FieldSelect>
      </SettingBlock>
      <SettingToggle label="Auto-save workspace state" description="Persist panes, scrollback, layout, and summaries after important events." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Semantic repository index" description="Build a local index for smarter cross-pane agent context." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Auto session summaries" description="Generate a concise summary when a session is closed or restored." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Reduce motion" description="Disable large transitions and decorative animations." checked={false} onChange={() => undefined} implemented={false} />
      <SettingBlock label="App version & updates" implemented description="Checks GitHub releases when the app opens. You can also check manually here.">
        <div className="space-y-3 text-sm">
          <p className="text-pm-muted">
            Installed version: <span className="font-mono text-pm-text">v{installInfo?.version ?? '…'}</span>
            {installInfo && !installInfo.isPackaged ? ' (dev build)' : ''}
          </p>
          {updateCheck?.updateAvailable && updateCheck.latestVersion ? (
            <p className="text-pm-ok">
              Update available: v{updateCheck.latestVersion}. Download the installer from GitHub releases.
            </p>
          ) : updateCheck?.error ? (
            <p className="text-pm-muted">Could not check for updates: {updateCheck.error}</p>
          ) : updateCheck?.latestVersion ? (
            <p className="text-pm-muted">You are on the latest release (v{updateCheck.latestVersion}).</p>
          ) : (
            <p className="text-pm-muted">Update check runs automatically on launch.</p>
          )}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => onCheckForUpdates?.()}
              disabled={updateChecking}
              className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40 disabled:opacity-50"
            >
              {updateChecking ? 'Checking…' : 'Check for updates'}
            </button>
            {updateCheck?.updateAvailable && (
              <button
                type="button"
                onClick={() => onOpenRelease?.()}
                className="rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm font-medium text-pm-accent hover:bg-pm-accent/20"
              >
                Download & install
              </button>
            )}
          </div>
        </div>
      </SettingBlock>
    </SettingsSection>
  );
}

function AppearanceTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, onSidebarWidthChange, currentSidebarWidth } = ctx;
  const storedWidth = clampSidebarWidth(settings.sidebar_width ?? 360);
  const liveWidth = clampSidebarWidth(currentSidebarWidth ?? storedWidth);
  const [useCustomWidth, setUseCustomWidth] = useState(() => !isSidebarWidthPreset(storedWidth));

  useEffect(() => {
    setUseCustomWidth(!isSidebarWidthPreset(storedWidth));
  }, [storedWidth]);

  const applySidebarWidth = (next: number) => {
    const clamped = clampSidebarWidth(next);
    setSettings({ ...settings, sidebar_width: clamped });
    onSidebarWidthChange?.(clamped);
  };

  const selectValue = useCustomWidth ? 'custom' : String(storedWidth);

  return (
    <SettingsSection title="Appearance" description="Tune interface density and terminal layout.">
      <SettingBlock label="Sidebar width" implemented description="Orchestrator sidebar width in the workspace (stored in settings.json). Drag the divider in the workspace to resize live.">
        <div className="flex flex-col gap-2">
          <p className="text-xs text-pm-muted">
            Current width:{' '}
            <span className="font-mono text-pm-text">{liveWidth}px</span>
            {useCustomWidth && isSidebarWidthPreset(liveWidth) && (
              <span className="ml-1">(custom preset)</span>
            )}
          </p>
          <FieldSelect
            value={selectValue}
            onChange={(e) => {
              if (e.target.value === 'custom') {
                setUseCustomWidth(true);
                return;
              }
              setUseCustomWidth(false);
              applySidebarWidth(Number(e.target.value));
            }}
          >
            {SIDEBAR_WIDTH_PRESETS.map((preset) => (
              <option key={preset} value={preset}>
                {SIDEBAR_WIDTH_PRESET_LABELS[preset]} ({preset}px)
              </option>
            ))}
            <option value="custom">Custom…</option>
          </FieldSelect>
          {useCustomWidth && (
            <div className="flex items-center gap-2">
              <FieldInput
                type="number"
                min={MIN_SIDEBAR_WIDTH}
                max={MAX_SIDEBAR_WIDTH}
                step={1}
                value={storedWidth}
                onChange={(e) => {
                  const value = Number(e.target.value);
                  if (!Number.isFinite(value)) return;
                  applySidebarWidth(value);
                }}
                className="font-mono"
              />
              <span className="shrink-0 text-xs text-pm-muted">px</span>
            </div>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Default grid columns" implemented={false} description="Starting terminal grid when opening a workspace.">
        <FieldSelect value="2" disabled><option value="1">1 column</option><option value="2">2 columns</option><option value="3">3 columns</option></FieldSelect>
      </SettingBlock>
      <InfoCard title="Design language" description="Warm accents are reserved for primary actions and active states." implemented={false} />
    </SettingsSection>
  );
}

function SessionTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="Sessions" description="Workspace defaults, history, and orchestration behavior.">
      <SettingToggle label="Send initial prompt on start" description="Automatically submit the new-session prompt when a workspace opens." checked onChange={() => undefined} implemented={false} />
      <SettingBlock label="Default orchestrator" implemented description="Sidebar brain for tool routing and delegation.">
        <FieldSelect value={settings.orchestrator_backend ?? 'api'} onChange={(e) => setSettings({ ...settings, orchestrator_backend: e.target.value as OrchestratorBackend })}>
          {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((backend) => (
            <option key={backend} value={backend}>{ORCHESTRATOR_BACKEND_LABELS[backend]}</option>
          ))}
        </FieldSelect>
      </SettingBlock>
      <SettingBlock label="Keep session history" implemented={false} description="How long to retain closed session metadata.">
        <FieldSelect value="30" disabled><option value="7">7 days</option><option value="30">30 days</option><option value="90">90 days</option><option value="365">1 year</option></FieldSelect>
      </SettingBlock>
    </SettingsSection>
  );
}

function OrchestratorTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, draftCustom, setDraftCustom } = ctx;
  const models = listModels(settings).filter((m) => m.provider === settings.default_provider);
  const customForProvider = (settings.custom_models ?? []).filter((m) => m.provider === settings.default_provider);
  const addCustomModel = () => {
    const label = draftCustom.label.trim();
    const model_id = draftCustom.model_id.trim();
    if (!label || !model_id) return;
    const entry: LlmModel = { provider: draftCustom.provider, model_id, label };
    const next = [...(settings.custom_models ?? []).filter((m) => !(m.provider === entry.provider && m.model_id === entry.model_id)), entry];
    setSettings({ ...settings, custom_models: next, default_provider: entry.provider, default_model: entry.model_id });
    setDraftCustom({ ...EMPTY_CUSTOM, provider: draftCustom.provider });
  };
  const removeCustomModel = (target: LlmModel) => {
    const next = (settings.custom_models ?? []).filter((m) => !(m.provider === target.provider && m.model_id === target.model_id));
    setSettings({ ...settings, custom_models: next });
  };
  return (
    <SettingsSection title="Orchestrator" description="Configure the Puppet Master sidebar brain and delegation logic.">
      <SettingBlock label="Backend" implemented>
        <FieldSelect value={settings.orchestrator_backend ?? 'api'} onChange={(e) => setSettings({ ...settings, orchestrator_backend: e.target.value as OrchestratorBackend })}>
          {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((backend) => (
            <option key={backend} value={backend}>{ORCHESTRATOR_BACKEND_LABELS[backend]}</option>
          ))}
        </FieldSelect>
      </SettingBlock>
      <SettingToggle label="Cost-aware model router" description="Route tasks to cheaper or stronger models depending on complexity." checked onChange={() => undefined} implemented={false} />
      <SettingBlock label="Default provider & model" implemented description="Used by the API orchestrator backend.">
        <div className="space-y-3">
          <div>
            <FieldLabel>Provider</FieldLabel>
            <FieldSelect value={settings.default_provider} onChange={(e) => {
              const provider = e.target.value as LlmProvider;
              const first = listModels(settings).find((m) => m.provider === provider);
              setSettings({ ...settings, default_provider: provider, default_model: first?.model_id ?? settings.default_model });
              setDraftCustom({ ...draftCustom, provider });
            }}>
              <option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option>
            </FieldSelect>
          </div>
          <div>
            <FieldLabel>Model</FieldLabel>
            <FieldSelect value={settings.default_model} onChange={(e) => setSettings({ ...settings, default_model: e.target.value })}>
              {models.map((m) => (<option key={`${m.provider}::${m.model_id}`} value={m.model_id}>{m.label}</option>))}
            </FieldSelect>
          </div>
        </div>
      </SettingBlock>
      <SettingBlock label="Custom models" implemented description="Add vendor/model ids for the sidebar picker.">
        {customForProvider.length > 0 && (
          <ul className="mb-3 space-y-1">
            {customForProvider.map((m) => (
              <li key={`${m.provider}::${m.model_id}`} className="flex items-center gap-2 text-xs font-mono">
                <span className="min-w-0 flex-1 truncate">{m.label} — {m.model_id}</span>
                <button type="button" onClick={() => removeCustomModel(m)} className="rounded px-1.5 py-0.5 text-pm-err hover:bg-pm-err/10">Remove</button>
              </li>
            ))}
          </ul>
        )}
        <div className="grid grid-cols-2 gap-2">
          <div><FieldLabel>Provider</FieldLabel><FieldSelect value={draftCustom.provider} onChange={(e) => setDraftCustom({ ...draftCustom, provider: e.target.value as LlmProvider })}><option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option></FieldSelect></div>
          <div><FieldLabel>Display label</FieldLabel><FieldInput value={draftCustom.label} onChange={(e) => setDraftCustom({ ...draftCustom, label: e.target.value })} placeholder="My model" /></div>
        </div>
        <div className="mt-2 flex gap-2">
          <FieldInput value={draftCustom.model_id} onChange={(e) => setDraftCustom({ ...draftCustom, model_id: e.target.value })} className="font-mono" placeholder="model-id or vendor/model" />
          <button type="button" onClick={addCustomModel} disabled={!draftCustom.label.trim() || !draftCustom.model_id.trim()} className="shrink-0 rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm text-pm-accent hover:bg-pm-accent/20 disabled:opacity-50">Add</button>
        </div>
      </SettingBlock>
      <InfoCard title="Shared tool surface" description="Sidebar API loop and external MCP hosts use the same list, spawn, read, write, kill, snapshot, approve, and route bridge tools." />
    </SettingsSection>
  );
}

function ApiTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="API keys" description="Keys are stored in secure OS storage in the desktop build.">
      <SettingBlock label="Anthropic API key" implemented><FieldInput type="password" value={settings.anthropic_api_key ?? ''} onChange={(e) => setSettings({ ...settings, anthropic_api_key: e.target.value })} className="font-mono" placeholder="sk-ant-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenAI API key" implemented><FieldInput type="password" value={settings.openai_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openai_api_key: e.target.value })} className="font-mono" placeholder="sk-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenRouter API key" implemented><FieldInput type="password" value={settings.openrouter_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openrouter_api_key: e.target.value })} className="font-mono" placeholder="sk-or-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="GitHub token" implemented={false}><FieldInput type="password" className="font-mono" placeholder="ghp_…" disabled /></SettingBlock>
      <SettingBlock label="Linear API key" implemented={false}><FieldInput type="password" className="font-mono" placeholder="lin_api_…" disabled /></SettingBlock>
      <SettingToggle label="Validate keys on save" description="Probe provider APIs with a minimal request before storing." checked onChange={() => undefined} implemented={false} />
    </SettingsSection>
  );
}

function statusTone(ok: boolean): string {
  return ok ? 'text-emerald-400' : 'text-red-400';
}

function StatusRow({ label, ok, detail }: { label: string; ok: boolean; detail: string }) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-2 rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs">
      <span className="font-medium text-pm-text">{label}</span>
      <div className="min-w-0 text-right">
        <div className={statusTone(ok)}>{ok ? 'OK' : 'Needs attention'}</div>
        <p className="mt-1 text-pm-muted">{detail}</p>
      </div>
    </div>
  );
}

function McpTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl, projectPath, settings } = ctx;
  const installPath = projectPath ?? settings.project_path ?? '';
  const [installing, setInstalling] = useState(false);
  const [installScope, setInstallScope] = useState<'project' | 'global' | 'uninstall-project' | 'uninstall-global' | null>(null);
  const [installResults, setInstallResults] = useState<EnsureMcpResult[] | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const [status, setStatus] = useState<McpStatusReport | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const refreshStatus = async (autoRepair = false) => {
    if (!installPath) {
      setStatus(null);
      return;
    }
    setChecking(true);
    setStatusError(null);
    try {
      const report = await getMcpStatus(installPath, autoRepair);
      setStatus(report);
      if (report.repairResults.length > 0) {
        setInstallResults(report.repairResults);
      }
    } catch (error) {
      setStatus(null);
      setStatusError(error instanceof Error ? error.message : String(error));
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    void refreshStatus(false);
  }, [installPath]);

  const installNpmPackage = async () => {
    if (!installPath || installing) return;
    setInstalling(true);
    setInstallScope('project');
    setInstallError(null);
    try {
      const results = await installNpmMcpConfigs(installPath);
      setInstallResults(results);
      await refreshStatus(false);
    } catch (error) {
      setInstallResults(null);
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      setInstalling(false);
    }
  };

  const installGlobalNpmPackage = async () => {
    if (installing) return;
    setInstalling(true);
    setInstallScope('global');
    setInstallError(null);
    try {
      const results = await installGlobalNpmMcpConfigs();
      setInstallResults(results);
      await refreshStatus(false);
    } catch (error) {
      setInstallResults(null);
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      setInstalling(false);
    }
  };

  const uninstallProjectMcp = async () => {
    if (!installPath || installing) return;
    const confirmed = await ask(
      'Remove puppet-master from this project’s .mcp.json, .codex/config.toml, and opencode.json?',
      { title: 'Uninstall MCP from project?', kind: 'warning', okLabel: 'Uninstall', cancelLabel: 'Cancel' },
    );
    if (!confirmed) return;
    setInstalling(true);
    setInstallScope('uninstall-project');
    setInstallError(null);
    try {
      const results = await uninstallNpmMcpConfigs(installPath);
      setInstallResults(results);
      await refreshStatus(false);
    } catch (error) {
      setInstallResults(null);
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      setInstalling(false);
    }
  };

  const uninstallGlobalMcp = async () => {
    if (installing) return;
    const confirmed = await ask(
      'Remove puppet-master from global Claude Code, Codex, and OpenCode MCP configs?',
      { title: 'Uninstall global MCP?', kind: 'warning', okLabel: 'Uninstall', cancelLabel: 'Cancel' },
    );
    if (!confirmed) return;
    setInstalling(true);
    setInstallScope('uninstall-global');
    setInstallError(null);
    try {
      const results = await uninstallGlobalNpmMcpConfigs();
      setInstallResults(results);
      await refreshStatus(false);
    } catch (error) {
      setInstallResults(null);
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      setInstalling(false);
    }
  };

  return (
    <SettingsSection title="MCP & bridge" description="External host integration for Cursor, Claude Desktop, Codex, OpenCode, and automation scripts.">
      <SettingBlock label="Runtime status" implemented description="Checks the local bridge, npm package, and orchestrator MCP configs. Use Recheck & repair to fix project configs.">
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void refreshStatus(true)}
              disabled={!installPath || checking}
              className="inline-flex min-h-10 items-center justify-center rounded-lg border border-pm-border px-4 py-2 text-sm font-semibold transition hover:bg-pm-border/40 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {checking ? 'Checking…' : 'Recheck & repair'}
            </button>
            <span className={`text-sm font-semibold ${status ? statusTone(status.overallReady) : 'text-pm-muted'}`}>
              {status ? (status.overallReady ? 'Ready for orchestrators' : 'Not ready') : 'No project selected'}
            </span>
          </div>
          {status && (
            <div className="space-y-2">
              <StatusRow
                label="HTTP bridge"
                ok={status.bridgeReachable}
                detail={status.bridgeReachable
                  ? `${status.bridgeUrl ?? bridgeUrl ?? 'local'}${status.bridgeVersion ? ` · v${status.bridgeVersion}` : ''}`
                  : status.portFileExists
                    ? `Port file exists but bridge is not responding (${status.portFilePath})`
                    : 'Start Puppet Master and ensure the bridge port file is written'}
              />
              <StatusRow
                label="npm package"
                ok={status.npmAvailable && Boolean(status.npmPackageVersion)}
                detail={status.npmPackageVersion
                  ? `@puppet-master/mcp@${status.npmPackageVersion} via ${status.launchCommand}`
                  : status.npmAvailable
                    ? 'npm is available but @puppet-master/mcp could not be resolved'
                    : 'Install Node.js/npm and ensure network access to the npm registry'}
              />
              <StatusRow
                label="Node.js"
                ok={status.nodeAvailable}
                detail={status.nodeAvailable ? 'node is on PATH for MCP launchers' : 'node was not found on PATH'}
              />
              {status.backends.map((backend) => (
                <div key={backend.backend} className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="font-medium text-pm-text">{backend.label}</span>
                    <span className={statusTone(backend.installed && backend.usesNpm)}>
                      {backend.installed
                        ? backend.usesNpm
                          ? 'Installed (npm)'
                          : 'Installed (local script)'
                        : 'Not installed'}
                    </span>
                  </div>
                  <p className="mt-1 text-pm-muted">{backend.message}</p>
                  <p className="mt-1 break-all font-mono text-[10px] text-pm-muted">{backend.configPath}</p>
                </div>
              ))}
            </div>
          )}
          {statusError && (
            <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-300">
              {statusError}
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Bridge endpoint" implemented description="Auto-discovered local HTTP bridge (port range 17321–17399).">
        <FieldInput value={status?.bridgeUrl ?? bridgeUrl ?? 'Discovering…'} readOnly className="font-mono" />
      </SettingBlock>
      <SettingBlock label="Install npm MCP package" implemented description="Register Claude Code, Codex, and OpenCode to use npx -y @puppet-master/mcp instead of any bundled or local script entry.">
        <div className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:flex-wrap">
            <button
              type="button"
              onClick={() => void installNpmPackage()}
              disabled={!installPath || installing}
              className="inline-flex min-h-10 items-center justify-center rounded-lg bg-pm-accent px-4 py-2 text-sm font-semibold text-black transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {installing && installScope === 'project' ? 'Installing…' : 'Install in project'}
            </button>
            <button
              type="button"
              onClick={() => void installGlobalNpmPackage()}
              disabled={installing}
              className="inline-flex min-h-10 items-center justify-center rounded-lg border border-pm-border px-4 py-2 text-sm font-semibold transition hover:bg-pm-border/40 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {installing && installScope === 'global' ? 'Installing…' : 'Install globally'}
            </button>
            <span className="min-w-0 break-all font-mono text-xs text-pm-muted">
              Project: {installPath || 'Choose a project folder in General first'}
            </span>
          </div>
          {installResults && (
            <div className="space-y-2">
              {installResults.map((result) => (
                <div key={result.backend} className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="font-mono text-pm-text">{result.backend}</span>
                    <span className={result.installed ? 'text-emerald-400' : 'text-red-400'}>
                      {result.installed ? result.changed ? 'Updated' : 'Already set' : 'Incomplete'}
                    </span>
                  </div>
                  <p className="mt-1 text-pm-muted">{result.message}</p>
                </div>
              ))}
              <p className="text-xs leading-5 text-pm-muted">
                Restart any open MCP host sessions so they disconnect the old process and launch the npm package entry.
              </p>
            </div>
          )}
          {installError && (
            <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-300">
              {installError}
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Uninstall MCP" implemented description="Remove puppet-master from orchestrator config files. Does not uninstall the Puppet Master app or the @puppet-master/mcp npm package. Restart MCP hosts (Cursor, Claude Code, etc.) after uninstalling.">
        <div className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:flex-wrap">
            <button
              type="button"
              onClick={() => void uninstallProjectMcp()}
              disabled={!installPath || installing}
              className="inline-flex min-h-10 items-center justify-center rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-2 text-sm font-semibold text-red-400 transition hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {installing && installScope === 'uninstall-project' ? 'Removing…' : 'Uninstall from project'}
            </button>
            <button
              type="button"
              onClick={() => void uninstallGlobalMcp()}
              disabled={installing}
              className="inline-flex min-h-10 items-center justify-center rounded-lg border border-red-500/40 px-4 py-2 text-sm font-semibold text-red-400 transition hover:bg-red-500/10 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {installing && installScope === 'uninstall-global' ? 'Removing…' : 'Uninstall globally'}
            </button>
          </div>
          <p className="text-xs leading-5 text-pm-muted">
            Project: removes <span className="font-mono">.mcp.json</span>, <span className="font-mono">.codex/config.toml</span>, and <span className="font-mono">opencode.json</span> entries.
            Global: removes Claude Code local and user MCP, <span className="font-mono">~/.mcp.json</span>, <span className="font-mono">~/.codex/config.toml</span>, and OpenCode global config.
            Cursor / Claude Desktop configs are manual — edit your host&apos;s MCP settings file.
          </p>
        </div>
      </SettingBlock>
      <SettingToggle label="Remote runners" description="Allow trusted remote machines to host long-running terminal panes." checked={false} onChange={() => undefined} implemented={false} />
      <CodeBlock title="Claude Desktop config" code={CLAUDE_DESKTOP_MCP} />
      <CodeBlock title="Codex config" code={CODEX_MCP} />
    </SettingsSection>
  );
}

function MobileTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="Mobile PWA" description="Pair a phone to mirror panes, send input, and monitor agent progress.">
      <SettingBlock label="Mobile input buffer (ms)" implemented description="Delay before committing mobile keystrokes to the PTY.">
        <FieldInput type="number" min={0} max={1000} step={50} value={settings.mobile_input_delay_ms ?? 250} onChange={(e) => {
          const value = Number(e.target.value);
          setSettings({ ...settings, mobile_input_delay_ms: Number.isFinite(value) ? value <= 0 ? 0 : Math.min(1000, Math.max(50, Math.round(value))) : 250 });
        }} className="font-mono" />
      </SettingBlock>
      <SettingToggle label="Show mobile input box" description="When disabled, the tap target stays invisible on the phone." checked={settings.mobile_input_visible ?? true} onChange={(value) => setSettings({ ...settings, mobile_input_visible: value })} implemented />
      <SettingToggle label="Require pairing approval" description="Ask for confirmation on desktop before a phone can connect." checked onChange={() => undefined} implemented={false} />
      <MobilePairingPanel publicPwaUrl={settings.public_pwa_url ?? ''} devServerPort={parseDevServerPort(settings.dev_server_port)} onPublicPwaUrlChange={(value) => setSettings({ ...settings, public_pwa_url: value.trim() || undefined })} onDevServerPortChange={(port) => setSettings({ ...settings, dev_server_port: port })} />
    </SettingsSection>
  );
}

function SecurityTab() {
  return (
    <SettingsSection title="Security" description="Protect API keys, local bridge access, and paired devices.">
      <SettingToggle label="Encrypted local storage" description="Encrypt settings, session history, and sensitive metadata." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Require approval for external MCP clients" description="Prompt before Cursor, Claude Desktop, or Codex can control panes." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Mask secrets in terminal scrollback" description="Redact detected API keys and tokens from saved history." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Audit log" description="Record every pane write, MCP tool call, model route, and approval decision." checked implemented={false} onChange={() => undefined} />
      <InfoCard title="Security status" description="OS keychain is available. Bridge accepts local loopback clients unless mobile pairing is enabled." implemented={false} />
    </SettingsSection>
  );
}

function StorageTab({ ctx }: { ctx: SettingsTabContext }) {
  const [storageInfo, setStorageInfo] = useState<CoordinationStorageInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = async () => {
    try {
      setError(null);
      setStorageInfo(await tauri.getCoordinationStorageInfo());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };
  useEffect(() => {
    void refresh();
  }, [ctx.projectPath]);
  const scopeLabel = storageInfo?.scope === 'project' ? 'Project-local' : 'Global fallback';
  return (
    <SettingsSection title="Coordination storage" description="Task board, resource locks, audit entries, and pane timeline events are scoped to the selected project folder.">
      <SettingBlock label="Workspace session" implemented description="Coordination events are replayed from this JSONL file to rebuild tasks and locks.">
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-3">
            <MetricPill label="Scope" value={scopeLabel} />
            <MetricPill label="Tasks" value={String(storageInfo?.task_count ?? 0)} />
            <MetricPill label="Locks" value={String(storageInfo?.lock_count ?? 0)} />
          </div>
          <div className="rounded-lg border border-pm-border bg-pm-bg p-3">
            <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">Project</div>
            <div className="mt-1 break-all font-mono text-xs text-pm-text">
              {storageInfo?.project_path ?? ctx.projectPath ?? 'No project folder selected'}
            </div>
          </div>
          <div className="rounded-lg border border-pm-border bg-pm-bg p-3">
            <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">Event log</div>
            <div className="mt-1 break-all font-mono text-xs text-pm-text">
              {storageInfo?.event_log_path ?? 'Loading...'}
            </div>
            <div className="mt-2 text-xs text-pm-muted">
              {storageInfo?.event_count ?? 0} events · {storageInfo?.exists ? 'file exists' : 'created on first write'}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button type="button" onClick={() => void refresh()} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">
              Refresh
            </button>
            <span className="break-all text-xs text-pm-muted">
              Directory: {storageInfo?.storage_dir ?? 'Loading...'}
            </span>
          </div>
          {error && (
            <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-300">
              {error}
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingToggle label="Project-local tasks and locks" description="Keep coordination state inside each project's .puppet-master folder." checked implemented onChange={() => undefined} />
      <SettingToggle label="Require locks for file edits" description="Prompt orchestrators to claim file or directory ownership before delegating edits." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Archive completed sessions" description="Move completed task and lock history into dated archive files." checked={false} implemented={false} onChange={() => undefined} />
    </SettingsSection>
  );
}

function MetricPill({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2">
      <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">{label}</div>
      <div className="mt-1 truncate text-sm font-semibold text-pm-text">{value}</div>
    </div>
  );
}

function NotificationsTab() {
  return (
    <SettingsSection title="Notifications" description="Choose when Puppet Master should interrupt you.">
      <SettingToggle label="Desktop notifications" description="Show system notifications for completed tasks and blocked panes." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when agent waits for input" description="Useful for Claude confirmation prompts and failed commands." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when tests fail" description="Send a notification when a Bash pane detects test failures." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify on cost threshold" description="Warn when a session approaches the configured daily cost limit." checked implemented={false} onChange={() => undefined} />
    </SettingsSection>
  );
}

function BackupTab() {
  return (
    <SettingsSection title="Backup & sync" description="Keep encrypted workspace metadata available across machines.">
      <SettingToggle label="Cloud backup" description="Upload encrypted session metadata and settings to your account." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Include terminal scrollback" description="Back up pane output. Secrets are redacted before upload." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Team sync preview" description="Sync approved workspace summaries to a shared team feed." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" disabled className="rounded-lg border border-pm-border px-3 py-2 text-sm opacity-50">Export local backup</button>
    </SettingsSection>
  );
}

function MarketplaceTab() {
  return (
    <SettingsSection title="Agent marketplace" description="Install curated agents, prompt packs, and workflow presets.">
      <MockListCard title="Available packs" description="Marketplace installs will connect to a curated registry." items={[
        { title: 'Security reviewer', meta: 'Policy-aware code review agent · 18K installs', status: 'Installed' },
        { title: 'PR summarizer', meta: 'Generates release notes and review summaries · 42K installs', status: 'Installed' },
        { title: 'DB migration planner', meta: 'Plans reversible schema migrations · 9K installs' },
        { title: 'Frontend polish agent', meta: 'UI cleanup, accessibility, and responsive QA · 31K installs' },
      ]} />
    </SettingsSection>
  );
}

function RulesTab() {
  return (
    <SettingsSection title="Rules & guardrails" description="Create approval rules for commands, files, tools, and remote execution.">
      <MockListCard title="Guardrail policies" description="Command and tool approval rules are planned." items={[
        { title: 'Block destructive shell commands', meta: 'Require approval for rm -rf, git reset --hard, force push.', status: 'Enabled' },
        { title: 'Protect secrets and env files', meta: 'Prevent agents from printing .env or API tokens.', status: 'Enabled' },
        { title: 'Require approval for dependency changes', meta: 'Prompt before package managers modify lockfiles.', status: 'Enabled' },
        { title: 'Sandbox remote runners', meta: 'Limit remote jobs to approved repos.', status: 'Draft' },
      ]} />
    </SettingsSection>
  );
}

function CostsTab() {
  return (
    <SettingsSection title="Cost controls" description="Estimate, route, and limit model usage across sessions.">
      <SettingBlock label="Daily cost limit" implemented={false}><FieldInput value="$25" disabled /></SettingBlock>
      <SettingToggle label="Cost-aware model routing" description="Use cheaper models for simple tasks and stronger models for complex reasoning." checked implemented={false} onChange={() => undefined} />
      <div className="space-y-2 opacity-70">
        <StorageBarMock label="Anthropic" value="$1.62" percent="w-2/5" />
        <StorageBarMock label="OpenAI" value="$0.54" percent="w-1/5" />
        <StorageBarMock label="OpenRouter" value="$0.25" percent="w-1/6" />
      </div>
    </SettingsSection>
  );
}

function TeamTab() {
  return (
    <SettingsSection title="Team workspace" description="Share approved summaries, templates, and agent recipes with teammates.">
      <SettingToggle label="Enable team sync" description="Publish approved session summaries to a shared workspace feed." checked={false} implemented={false} onChange={() => undefined} />
      <MockListCard title="Members" description="Team presence and roles are not connected yet." items={[
        { title: 'Ada Chen', meta: 'Owner', status: 'online' },
        { title: 'Marco Lee', meta: 'Reviewer', status: 'idle' },
        { title: 'Nina Patel', meta: 'Developer', status: 'offline' },
      ]} />
    </SettingsSection>
  );
}

function AutomationTab() {
  return (
    <SettingsSection title="Automation" description="Run recipes when terminal, git, test, or MCP events happen.">
      <MockListCard title="Recipes" description="Event-driven automation will hook into bridge SSE in a future release." items={[
        { title: 'When tests fail', meta: 'Send failing output to Claude, ask Codex for a patch, rerun Bash tests.', status: 'Enabled' },
        { title: 'When Claude waits for input', meta: 'Notify desktop and mobile, then summarize the pending decision.', status: 'Enabled' },
        { title: 'When PR changes', meta: 'Refresh semantic index and generate review plan.' },
        { title: 'Nightly workspace digest', meta: 'Summarize all active panes and export a Markdown report.' },
      ]} />
    </SettingsSection>
  );
}

function ObservabilityTab() {
  return (
    <SettingsSection title="Observability" description="Monitor model routes, pane events, bridge latency, and MCP calls.">
      <MockListCard title="Metrics" description="Live observability dashboards are planned." items={[
        { title: 'Bridge p95 latency', meta: 'Last 24 hours', status: '24 ms' },
        { title: 'MCP tool calls today', meta: 'Last 24 hours', status: '186' },
        { title: 'Pane writes blocked', meta: 'Guardrails', status: '4' },
        { title: 'Average agent idle time', meta: 'Last 24 hours', status: '38 sec' },
      ]} />
      <InfoCard title="Protocol inspector" description="Enable developer mode to see raw SSE events, request IDs, and MCP payload timing." implemented={false} />
    </SettingsSection>
  );
}

function PluginsTab() {
  return (
    <SettingsSection title="Plugins" description="Extend Puppet Master with sandboxed local plugins.">
      <SettingToggle label="Plugin sandbox" description="Run plugins in a restricted worker with explicit file and network permissions." checked implemented={false} onChange={() => undefined} />
      <MockListCard title="Installed plugins" description="Plugin runtime is not available yet." items={[
        { title: 'GitHub Pull Request Connector', meta: 'Sandboxed local plugin', status: 'Enabled' },
        { title: 'Linear Issue Sync', meta: 'Sandboxed local plugin', status: 'Enabled' },
        { title: 'Slack Digest Publisher', meta: 'Sandboxed local plugin', status: 'Disabled' },
        { title: 'Docker Runner Provider', meta: 'Sandboxed local plugin', status: 'Experimental' },
      ]} />
    </SettingsSection>
  );
}

function DeveloperTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl } = ctx;
  const [settingsPath, setSettingsPath] = useState<string>('…');
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  return (
    <SettingsSection title="Developer" description="Diagnostics and integration details for plugin authors.">
      <InfoCard title="Bridge endpoint" description={bridgeUrl ?? 'Bridge not discovered yet. Start the desktop app.'} />
      <InfoCard title="Settings file" description={`All desktop preferences persist to ${settingsPath} under the "settings" key.`} />
      <SettingToggle label="Log MCP tool calls" description="Verbose bridge and orchestration debugging." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Show protocol inspector" description="Display raw SSE events and bridge payloads in workspace." checked={false} implemented={false} onChange={() => undefined} />
      <CodeBlock title="Bridge health response" code={"{\n  \"ok\": true,\n  \"version\": \"0.8.0\",\n  \"panes\": 3,\n  \"transport\": [\"http\", \"sse\"],\n  \"latency_ms\": 18,\n  \"features\": [\"guardrails\", \"routing\", \"audit_log\"]\n}"} />
    </SettingsSection>
  );
}

function AdvancedTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings } = ctx;
  const [installInfo, setInstallInfo] = useState<AppInstallInfo | null>(null);
  const [uninstallError, setUninstallError] = useState<string | null>(null);
  const [uninstalling, setUninstalling] = useState(false);
  useEffect(() => { void tauri.getAppInstallInfo().then(setInstallInfo); }, []);
  const exportSettings = () => {
    const redacted = {
      ...settings,
      anthropic_api_key: settings.anthropic_api_key ? '***' : undefined,
      openai_api_key: settings.openai_api_key ? '***' : undefined,
      openrouter_api_key: settings.openrouter_api_key ? '***' : undefined,
    };
    const blob = new Blob([JSON.stringify(redacted, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'puppet-master-settings.json';
    anchor.click();
    URL.revokeObjectURL(url);
  };
  const handleUninstall = async () => {
    setUninstallError(null);
    const confirmed = await ask(
      'Puppet Master will close and be removed from this computer. Your app data folder may remain until you delete it manually. Continue?',
      { title: 'Uninstall Puppet Master?', kind: 'warning', okLabel: 'Uninstall', cancelLabel: 'Cancel' },
    );
    if (!confirmed) return;
    setUninstalling(true);
    try {
      await tauri.launchUninstall();
    } catch (error) {
      setUninstallError(error instanceof Error ? error.message : String(error));
      setUninstalling(false);
    }
  };
  return (
    <SettingsSection title="Advanced" description="Power-user options. Change carefully.">
      <SettingBlock label="Uninstall Puppet Master" implemented description={installInfo?.uninstallInstructions ?? 'Loading uninstall instructions…'}>
        <div className="space-y-3">
          {installInfo?.dataDir && (
            <p className="text-xs leading-6 text-pm-muted">
              App data: <span className="font-mono text-pm-text">{installInfo.dataDir}</span>
            </p>
          )}
          {uninstallError && <p className="text-sm text-red-400">{uninstallError}</p>}
          <button
            type="button"
            onClick={() => void handleUninstall()}
            disabled={uninstalling || !installInfo?.uninstallAvailable}
            className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm font-semibold text-red-400 transition hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {uninstalling ? 'Uninstalling…' : 'Uninstall Puppet Master'}
          </button>
          {installInfo && !installInfo.uninstallAvailable && (
            <p className="text-xs text-pm-muted">
              In-app uninstall is only available from an installed NSIS/dmg build. For dev builds, stop the app and delete the project folder.
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Scrollback limit" implemented={false}><FieldInput value="10000" disabled className="font-mono" /></SettingBlock>
      <SettingBlock label="Pane spawn timeout" implemented={false}><FieldInput value="30000" disabled className="font-mono" /></SettingBlock>
      <SettingToggle label="Telemetry" description="Send anonymous product diagnostics." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" onClick={exportSettings} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Export settings JSON</button>
    </SettingsSection>
  );
}
