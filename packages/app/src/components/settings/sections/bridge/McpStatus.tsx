// Runtime status: the local bridge, the MCP runtime, Node.js and each agent's config. It owns its state and
// re-checks when the project folder changes or when an install or uninstall announces a change.
import { useCallback, useEffect, useState } from 'react';
import { getMcpStatus, type EnsureMcpResult, type McpStatusReport } from '../../../../lib/mcp-config';
import type { SettingsEnv } from '../../model';
import { Btn, Note, SettingBlock } from '../../settings-ui';
import { MCP_CHANGED_EVENT, RESTART_NOTE, ResultLines, StatusLine, errorText, installPathOf, runtimeName } from './shared';

const OK = 'OK';
const NEEDS_ATTENTION = 'Needs attention';

function bridgeDetail(status: McpStatusReport, fallbackUrl: string | null): string {
  if (status.bridgeReachable) {
    return `${status.bridgeUrl ?? fallbackUrl ?? 'local'}${status.bridgeVersion ? ` · v${status.bridgeVersion}` : ''}`;
  }
  if (status.portFileExists) return `Port file exists but bridge is not responding (${status.portFilePath})`;
  return 'Start Puppet Master and ensure the bridge port file is written';
}

function runtimeDetail(status: McpStatusReport, rust: boolean): string {
  if (rust) return `Using ${status.launchCommand}`;
  if (status.npmPackageVersion) return `@puppet-master/mcp@${status.npmPackageVersion} via ${status.launchCommand}`;
  if (status.npmAvailable) return 'npm is available but @puppet-master/mcp could not be resolved';
  return 'Install Node.js/npm and ensure network access to the npm registry';
}

export function McpStatusBlock({ env }: { env: SettingsEnv }) {
  const installPath = installPathOf(env);
  const rust = Boolean(env.settings.developer_use_rust_mcp);
  const [status, setStatus] = useState<McpStatusReport | null>(null);
  const [repairs, setRepairs] = useState<EnsureMcpResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(Boolean(installPath));

  const check = useCallback(
    async (autoRepair: boolean) => {
      if (!installPath) {
        setStatus(null);
        return;
      }
      setChecking(true);
      setError(null);
      try {
        const report = await getMcpStatus(installPath, autoRepair);
        setStatus(report);
        if (report.repairResults.length > 0) setRepairs(report.repairResults);
      } catch (e) {
        setStatus(null);
        setError(errorText(e));
      } finally {
        setChecking(false);
      }
    },
    [installPath],
  );

  useEffect(() => {
    void check(false);
  }, [check]);

  useEffect(() => {
    const onChanged = () => void check(false);
    window.addEventListener(MCP_CHANGED_EVENT, onChanged);
    return () => window.removeEventListener(MCP_CHANGED_EVENT, onChanged);
  }, [check]);

  const headline = !installPath
    ? 'No project selected'
    : status
      ? status.overallReady
        ? 'Ready for orchestrators'
        : 'Not ready'
      : checking
        ? 'Checking…'
        : 'Could not check';

  return (
    <SettingBlock
      label="Runtime status"
      description={`Checks the local bridge, ${runtimeName(env)}, and orchestrator MCP configs. Use Recheck & repair to fix project configs.`}
    >
      <div className="flex flex-wrap items-center gap-3">
        <Btn disabled={!installPath || checking} onClick={() => void check(true)}>
          {checking ? 'Checking…' : 'Recheck & repair'}
        </Btn>
        <Note role="status">{headline}</Note>
      </div>
      {status && (
        <div>
          <StatusLine label="HTTP bridge" value={status.bridgeReachable ? OK : NEEDS_ATTENTION} detail={bridgeDetail(status, env.bridgeUrl)} />
          <StatusLine
            label={rust ? 'MCP runtime' : 'npm package'}
            value={rust || (status.npmAvailable && Boolean(status.npmPackageVersion)) ? OK : NEEDS_ATTENTION}
            detail={runtimeDetail(status, rust)}
          />
          <StatusLine
            label="Node.js"
            value={status.nodeAvailable ? OK : NEEDS_ATTENTION}
            detail={status.nodeAvailable ? 'node is on PATH for MCP launchers' : 'node was not found on PATH'}
          />
          {status.backends.map((b) => (
            <StatusLine
              key={b.backend}
              label={b.label}
              value={b.installed ? (b.usesNpm ? 'Installed (npm)' : 'Installed (bundled)') : 'Not installed'}
              detail={
                <>
                  {b.message}
                  <br />
                  <span className="break-all font-mono text-[10px]">{b.configPath}</span>
                </>
              }
            />
          ))}
        </div>
      )}
      {repairs && <ResultLines results={repairs} note={RESTART_NOTE} />}
      {error && <Note role="alert">{error}</Note>}
    </SettingBlock>
  );
}
