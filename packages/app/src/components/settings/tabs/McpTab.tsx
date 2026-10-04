// Settings > MCP & bridge: runtime status, install/uninstall of MCP configs for orchestrator hosts.
import { useEffect, useState } from 'react';
import { ask } from '@tauri-apps/plugin-dialog';
import {
  getMcpStatus,
  installGlobalNpmMcpConfigs,
  installNpmMcpConfigs,
  PUPPET_MASTER_MCP_COMMAND,
  uninstallGlobalNpmMcpConfigs,
  uninstallNpmMcpConfigs,
  type EnsureMcpResult,
  type McpStatusReport,
} from '../../../lib/mcp-config';
import { CodeBlock, FieldInput, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

const CLAUDE_DESKTOP_MCP = JSON.stringify({ mcpServers: { 'puppet-master': PUPPET_MASTER_MCP_COMMAND } }, null, 2);
const CODEX_MCP = "[mcp_servers.puppet-master]\ncommand = \"npx\"\nargs = [\"-y\",\"@puppet-master/mcp\"]";

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

export function McpTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl, projectPath, settings } = ctx;
  const installPath = projectPath ?? settings.project_path ?? '';
  const selectedRuntime = settings.developer_use_rust_mcp ? 'bundled Rust MCP binary' : 'npm package';
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
      <SettingBlock label="Runtime status" implemented description={`Checks the local bridge, ${selectedRuntime}, and orchestrator MCP configs. Use Recheck & repair to fix project configs.`}>
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
                label={settings.developer_use_rust_mcp ? 'MCP runtime' : 'npm package'}
                ok={settings.developer_use_rust_mcp || (status.npmAvailable && Boolean(status.npmPackageVersion))}
                detail={settings.developer_use_rust_mcp
                  ? `Using ${status.launchCommand}`
                  : status.npmPackageVersion
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
                    <span className={statusTone(backend.installed && (settings.developer_use_rust_mcp || backend.usesNpm))}>
                      {backend.installed
                        ? backend.usesNpm
                          ? 'Installed (npm)'
                          : 'Installed (bundled)'
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
      <SettingBlock label="Install MCP runtime" implemented description={`Register Claude Code, Codex, and OpenCode to use the selected runtime: ${selectedRuntime}.`}>
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
                Restart any open MCP host sessions so they disconnect the old process and launch the selected MCP runtime.
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
