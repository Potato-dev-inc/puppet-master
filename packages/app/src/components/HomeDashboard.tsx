import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useCallback, useEffect, useState } from 'react';
import {
  ORCHESTRATOR_BACKEND_LABELS,
  type OrchestratorBackend,
  type Settings,
} from '@puppet-master/shared';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { loadSettings, resolveSettingsFilePath } from '../lib/settings';
import logoUrl from '../assets/branding/logo.svg';

interface Props {
  projectPath: string | null;
  bridgeReady: boolean;
  bridgeError: string | null;
  registry: PaneRegistryApi;
  settingsRevision: number;
  onOpenWorkspace: () => void;
  onOpenSettings: () => void;
  onProjectPathChange: (path: string) => Promise<void>;
}

function shortPath(path: string): string {
  const home = path.replace(/^\/Users\/[^/]+/, '~');
  if (home.length <= 56) return home;
  return `…${home.slice(-52)}`;
}

function paneStatusLabel(status: string): string {
  switch (status) {
    case 'running':
      return 'running';
    case 'waiting_input':
      return 'waiting';
    case 'idle':
      return 'idle';
    case 'error':
      return 'error';
    default:
      return status;
  }
}

export function HomeDashboard({
  projectPath,
  bridgeReady,
  bridgeError,
  registry,
  settingsRevision,
  onOpenWorkspace,
  onOpenSettings,
  onProjectPathChange,
}: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [settingsPath, setSettingsPath] = useState('settings.json');
  const paneCount = registry.paneList.length;
  const runningCount = registry.paneList.filter((p) => p.status === 'running').length;
  const waitingCount = registry.paneList.filter((p) => p.status === 'waiting_input').length;

  useEffect(() => {
    void loadSettings().then(setSettings);
    void resolveSettingsFilePath().then(setSettingsPath);
  }, [settingsRevision]);

  const pickProject = useCallback(async () => {
    const result = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: projectPath ?? (await homeDir()) ?? undefined,
    });
    if (typeof result === 'string') {
      await onProjectPathChange(result);
    }
  }, [onProjectPathChange, projectPath]);

  const orchestrator = settings?.orchestrator_backend ?? 'api';
  const modelLabel = settings
    ? `${settings.default_provider} / ${settings.default_model}`
    : 'Loading…';

  return (
    <main className="pm-home-screen">
      <div className="pm-home-grid-bg" aria-hidden />
      <div className="pm-home-vignette" aria-hidden />

      <div className="pm-home-shell">
        <aside className="pm-home-rail">
          <div className="pm-home-rail-brand">
            <img src={logoUrl} alt="" className="pm-home-rail-logo" draggable={false} />
            <span>Puppet Master</span>
          </div>
          <nav className="pm-home-rail-nav">
            <button type="button" className="pm-home-rail-link pm-home-rail-link--active">
              Overview
            </button>
            <button type="button" className="pm-home-rail-link" onClick={onOpenWorkspace}>
              Workspace
            </button>
            <button type="button" className="pm-home-rail-link" onClick={onOpenSettings}>
              Settings
            </button>
          </nav>
          <div className="pm-home-rail-foot">
            <span className="pm-home-rail-foot-label">Config</span>
            <code className="pm-home-rail-foot-path" title={settingsPath}>
              {shortPath(settingsPath)}
            </code>
          </div>
        </aside>

        <div className="pm-home-main">
          <header className="pm-home-topbar">
            <div>
              <p className="pm-home-kicker">Desktop orchestration</p>
              <h1 className="pm-home-title">Command your agent control room</h1>
            </div>
            <div className="pm-home-topbar-actions">
              <button type="button" onClick={() => void pickProject()} className="pm-home-btn pm-home-btn--ghost">
                {projectPath ? 'Change project' : 'Pick project'}
              </button>
              <button type="button" onClick={onOpenSettings} className="pm-home-btn pm-home-btn--ghost">
                Settings
              </button>
              <button type="button" onClick={onOpenWorkspace} className="pm-home-btn pm-home-btn--primary">
                Open workspace
              </button>
            </div>
          </header>

          <section className="pm-home-hero-band">
            <div className="pm-home-hero-visual" aria-hidden>
              <div className="pm-home-hero-grid">
                <div className="pm-home-mini-pane pm-home-mini-pane--a">
                  <span>planner</span>
                  <div className="pm-home-mini-lines"><i /><i /><i /></div>
                </div>
                <div className="pm-home-mini-pane pm-home-mini-pane--b">
                  <span>bridge</span>
                  <div className="pm-home-mini-lines"><i /><i /></div>
                </div>
                <div className="pm-home-mini-pane pm-home-mini-pane--c">
                  <span>tests</span>
                  <div className="pm-home-mini-lines"><i /><i /><i /></div>
                </div>
                <div className="pm-home-mini-scan" />
              </div>
            </div>
            <div className="pm-home-hero-copy">
              <p className="pm-home-hero-lead">
                Bridge-ready orchestration for Claude, Codex, OpenCode, Bash, and external MCP hosts.
                Launch the workspace when you are ready to work with live panes.
              </p>
              {projectPath ? (
                <div className="pm-home-project">
                  <span className="pm-home-project-label">Active project</span>
                  <code>{shortPath(projectPath)}</code>
                </div>
              ) : (
                <p className="pm-home-project pm-home-project--empty">
                  Pick a project folder before spawning agents.
                </p>
              )}
            </div>
          </section>

          <section className="pm-home-metrics">
            <MetricCard
              label="Bridge"
              value={bridgeReady ? 'Online' : bridgeError ? 'Offline' : 'Connecting'}
              detail={bridgeReady ? 'HTTP + SSE' : bridgeError ?? 'Waiting for local bridge'}
              ok={bridgeReady}
            />
            <MetricCard
              label="Panes"
              value={String(paneCount)}
              detail={
                paneCount === 0
                  ? 'No agents running'
                  : `${runningCount} running · ${waitingCount} waiting`
              }
              ok={paneCount > 0}
            />
            <MetricCard
              label="Orchestrator"
              value={ORCHESTRATOR_BACKEND_LABELS[orchestrator as OrchestratorBackend]}
              detail="Default sidebar backend"
              ok
            />
            <MetricCard label="Model" value={settings?.default_model ?? '…'} detail={modelLabel} ok={Boolean(settings)} />
          </section>

          <div className="pm-home-grid">
            <section className="pm-home-panel pm-home-panel--wide">
              <div className="pm-home-panel-head">
                <h2>Quick actions</h2>
                <p>Enter the workspace or adjust your defaults.</p>
              </div>
              <div className="pm-home-action-grid">
                <ActionCard
                  title="Open workspace"
                  description="Terminal grid, orchestrator sidebar, pane controls."
                  onClick={onOpenWorkspace}
                  primary
                />
                <ActionCard
                  title="Pick project"
                  description="Set cwd for new panes and orchestrator tools."
                  onClick={() => void pickProject()}
                />
                <ActionCard
                  title="Settings"
                  description="Orchestrator, API keys, mobile pairing, appearance."
                  onClick={onOpenSettings}
                />
              </div>
            </section>

            <section className="pm-home-panel">
              <div className="pm-home-panel-head">
                <h2>Active panes</h2>
                <p>{paneCount === 0 ? 'Nothing running yet.' : 'Click a pane to open the workspace.'}</p>
              </div>
              {paneCount === 0 ? (
                <div className="pm-home-empty">
                  <p>No terminal panes yet.</p>
                  <button type="button" onClick={onOpenWorkspace} className="pm-home-btn pm-home-btn--ghost">
                    Open workspace
                  </button>
                </div>
              ) : (
                <ul className="pm-home-pane-list">
                  {registry.paneList.map((pane) => (
                    <li key={pane.info.id}>
                      <button type="button" className="pm-home-pane-row" onClick={onOpenWorkspace}>
                        <span className={`pm-home-pane-dot pm-home-pane-dot--${pane.status}`} aria-hidden />
                        <span className="pm-home-pane-main">
                          <span className="pm-home-pane-title">{pane.info.agent_type}</span>
                          <span className="pm-home-pane-meta">{shortPath(pane.info.cwd)}</span>
                        </span>
                        <span className="pm-home-pane-badge">{paneStatusLabel(pane.status)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="pm-home-panel">
              <div className="pm-home-panel-head">
                <h2>Saved defaults</h2>
                <p>Loaded from settings.json on startup.</p>
              </div>
              <dl className="pm-home-config-list">
                <div>
                  <dt>Orchestrator</dt>
                  <dd>{ORCHESTRATOR_BACKEND_LABELS[orchestrator as OrchestratorBackend]}</dd>
                </div>
                <div>
                  <dt>Provider / model</dt>
                  <dd>{modelLabel}</dd>
                </div>
                <div>
                  <dt>Sidebar width</dt>
                  <dd>{settings?.sidebar_width ?? 360}px</dd>
                </div>
                <div>
                  <dt>Theme</dt>
                  <dd className="pm-home-config-capitalize">{settings?.theme ?? 'dark'}</dd>
                </div>
                <div>
                  <dt>Mobile input delay</dt>
                  <dd>{settings?.mobile_input_delay_ms ?? 250} ms</dd>
                </div>
              </dl>
            </section>
          </div>
        </div>
      </div>
    </main>
  );
}

function MetricCard({
  label,
  value,
  detail,
  ok,
}: {
  label: string;
  value: string;
  detail: string;
  ok: boolean;
}) {
  return (
    <div className="pm-home-metric">
      <span className="pm-home-metric-label">{label}</span>
      <span className="pm-home-metric-value">{value}</span>
      <span className={ok ? 'pm-home-metric-detail pm-home-metric-detail--ok' : 'pm-home-metric-detail'}>
        {detail}
      </span>
    </div>
  );
}

function ActionCard({
  title,
  description,
  onClick,
  primary,
}: {
  title: string;
  description: string;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={['pm-home-action', primary ? 'pm-home-action--primary' : ''].filter(Boolean).join(' ')}
    >
      <span className="pm-home-action-title">{title}</span>
      <span className="pm-home-action-desc">{description}</span>
    </button>
  );
}
