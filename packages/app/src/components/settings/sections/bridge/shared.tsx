// Pieces shared by the Bridge & MCP rows: the "MCP changed" event, one install/uninstall action at a
// time, and small status lines. Monochrome: status is words, never colour.
import { useState, type ReactNode } from 'react';
import type { EnsureMcpResult } from '../../../../lib/mcp-config';
import type { SettingsEnv } from '../../model';
import { Note } from '../../settings-ui';

export const MCP_CHANGED_EVENT = 'pm-mcp-changed';

export const RESTART_NOTE =
  'Restart any open MCP host sessions so they disconnect the old process and launch the selected MCP runtime.';

/** Tell the other Bridge & MCP rows that agent configs were written or removed. */
export function announceMcpChanged(): void {
  window.dispatchEvent(new Event(MCP_CHANGED_EVENT));
}

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The project folder the MCP rows act on: the open project, else the saved default. */
export function installPathOf(env: SettingsEnv): string {
  return env.projectPath ?? env.settings.project_path ?? '';
}

/** The runtime that the app writes into agent configs, in words. */
export function runtimeName(env: SettingsEnv): string {
  return env.settings.developer_use_rust_mcp ? 'bundled Rust MCP binary' : 'npm package';
}

/**
 * One MCP config action at a time (install or uninstall). Keeps its own results and error, and announces
 * the change when it ends, success or not, so the status row re-checks.
 */
export function useMcpAction() {
  const [busy, setBusy] = useState<string | null>(null);
  const [results, setResults] = useState<EnsureMcpResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (scope: string, action: () => Promise<EnsureMcpResult[]>) => {
    setBusy(scope);
    setError(null);
    try {
      setResults(await action());
    } catch (e) {
      setResults(null);
      setError(errorText(e));
    } finally {
      setBusy(null);
      announceMcpChanged();
    }
  };

  return { busy, results, error, run };
}

/** A label, a short value on the right, and the detail under it. */
export function StatusLine({ label, value, detail }: { label: ReactNode; value: ReactNode; detail?: ReactNode }) {
  return (
    <div className="mt-2 flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
      <span className="min-w-0 font-medium">{label}</span>
      <span className="st-value">{value}</span>
      {detail !== undefined && (
        <div className="w-full min-w-0 break-words">
          <Note>{detail}</Note>
        </div>
      )}
    </div>
  );
}

/** Per-agent results of an install, repair or uninstall. */
export function ResultLines({ results, note }: { results: EnsureMcpResult[]; note?: string }) {
  return (
    <div>
      {results.map((r) => (
        <StatusLine
          key={r.backend}
          label={backendName(r.backend)}
          value={r.installed ? (r.changed ? 'Updated' : 'Already set') : 'Incomplete'}
          detail={r.message}
        />
      ))}
      {note && <Note>{note}</Note>}
    </div>
  );
}

/**
 * An install result's backend id (`claude_cli_global`) in words: the agent, then where it was written
 * (this project, all projects or the home folder). An id nobody mapped yet is still read as words.
 */
export function backendName(id: string): string {
  const agents: Record<string, string> = { claude: 'Claude Code', codex: 'Codex', opencode: 'OpenCode' };
  const [first = '', ...rest] = id.split('_').filter(Boolean);
  const agent = agents[first] ?? first.charAt(0).toUpperCase() + first.slice(1);
  const words = rest.filter((word) => word !== 'cli');
  if (words.join('_') === 'global_home') return `${agent} · home folder`;
  if (words.includes('global')) return `${agent} · all projects`;
  if (rest.includes('cli')) return `${agent} · this project`;
  return words.length ? `${agent} ${words.join(' ')}` : agent;
}
