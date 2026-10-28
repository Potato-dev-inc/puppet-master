// Settings > Agents > Run agents in Linux (WSL): the WSL state, one check per distro, and where each agent runs.
// Saves through the wsl_* commands (lib/wsl.ts). Only "Check" starts a distro; opening this block never does.
// Hidden on a build without WSL (status `not_windows`). Agent names come from the backend; nothing here names one.
import { useEffect, useState, type ReactNode } from 'react';
import { DotSpinner } from '../../cockpit/DotSpinner';
import {
  wslCheck,
  wslPlacesGet,
  wslPlaceSet,
  wslStatus,
  type AgentPlaceRow,
  type Distro,
  type DistroView,
  type WslStatus,
} from '../../../lib/wsl';
import { Popup } from '../controls';
import { Btn, Note, SettingBlock } from '../settings-ui';

/** The "Runs on" value for Windows. Distro names are never empty, so this cannot clash with one. */
export const WINDOWS_VALUE = '';

const FOOTER =
  'In Linux an agent has its own sign-in. Puppet Master never copies your Windows logins or keys into Linux. ' +
  'Linux is not a sandbox: Linux programs can still read your Windows drives. Helpers you talk to on your phone always run on Windows.';

/** The text a failed call shows: the server's plain sentence, without its `CODE:` prefix. */
export function linuxErrorText(err: unknown): string {
  const text = (err instanceof Error ? err.message : String(err)).replace(/^[A-Z][A-Z0-9_]*:\s*/, '').trim();
  return text || "Couldn't do that. Try again.";
}

/** The distros an agent can be set to: WSL 2 only (agents need WSL 2), plus the one it is set to already. */
export function linuxOptions(distros: Distro[], row: AgentPlaceRow): string[] {
  const names = distros.filter((d) => d.version === 2).map((d) => d.name);
  if (row.distro && !names.includes(row.distro)) names.push(row.distro);
  return names;
}

/** The line under an agent row: whether it was found in the distro it runs in (null when not checked). */
export function foundText(row: AgentPlaceRow, views: Record<string, DistroView>): string | null {
  if (!row.distro) return null;
  const fromCheck = views[row.distro]?.agents.find((a) => a.agentId === row.agentId)?.found;
  const found = fromCheck ?? row.found;
  if (found === true) return `Found in ${row.distro}`;
  if (found === false) return `Not found in ${row.distro}. Install it inside Linux, then check again.`;
  return 'Not checked yet';
}

/** The line under a distro after a check: when, git, and whether Windows programs run from Linux. */
export function checkedText(view: DistroView): string {
  const time = new Date(view.checkedAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  return `Checked ${time} · git ${view.git ?? 'not found'} · Windows programs: ${view.interop ? 'on' : 'off'}`;
}

/** The select is locked only while an agent on Windows cannot go to Linux. Back to Windows is always allowed. */
export function isLockedToWindows(row: AgentPlaceRow): boolean {
  return Boolean(row.whyNot) && !row.distro;
}

function copyText(text: string): void {
  navigator.clipboard?.writeText(text).catch(() => undefined);
}

function MissingWsl({ command, onCopy, onAgain }: { command: ReactNode; onCopy?: () => void; onAgain: () => void }) {
  return (
    <div className="space-y-2 text-sm">
      {command}
      <div className="flex flex-wrap gap-2">
        {onCopy && (
          <Btn small onClick={onCopy}>
            Copy
          </Btn>
        )}
        <Btn small onClick={onAgain}>
          Check again
        </Btn>
      </div>
    </div>
  );
}

export function LinuxRows({ onSaved }: { onSaved?: () => void }) {
  // undefined = still loading, null = not available here (a browser preview), otherwise the WSL state.
  const [status, setStatus] = useState<WslStatus | null | undefined>(undefined);
  const [rows, setRows] = useState<AgentPlaceRow[]>([]);
  const [views, setViews] = useState<Record<string, DistroView>>({});
  const [checking, setChecking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    wslStatus(false)
      .then(async (next) => {
        const places = next.state === 'ready' ? await wslPlacesGet().catch(() => []) : [];
        if (alive) {
          setStatus(next);
          setRows(places);
        }
      })
      .catch(() => alive && setStatus(null));
    return () => {
      alive = false;
    };
  }, []);

  const checkAgain = async () => {
    setError(null);
    try {
      const next = await wslStatus(true);
      setStatus(next);
      setRows(next.state === 'ready' ? await wslPlacesGet() : []);
    } catch (e) {
      setError(linuxErrorText(e));
    }
  };

  const check = async (distro: string) => {
    setError(null);
    setChecking(distro);
    try {
      const view = await wslCheck(distro);
      setViews((all) => ({ ...all, [distro]: view }));
    } catch (e) {
      setError(linuxErrorText(e));
    } finally {
      setChecking(null);
    }
  };

  const choose = async (row: AgentPlaceRow, value: string) => {
    setError(null);
    try {
      const next = await wslPlaceSet(row.agentId, value === WINDOWS_VALUE ? null : value);
      setRows((all) => all.map((r) => (r.agentId === row.agentId ? next : r)));
      onSaved?.();
    } catch (e) {
      setError(linuxErrorText(e));
    }
  };

  const HEADING = { label: 'Run agents in Linux (WSL)' };
  if (status === undefined) return <SettingBlock {...HEADING} />;
  if (status === null) {
    return (
      <SettingBlock {...HEADING}>
        <p className="text-sm text-pm-muted">Available in the desktop app.</p>
      </SettingBlock>
    );
  }
  if (status.state === 'not_windows') return null;

  let body: ReactNode;
  if (status.state === 'not_installed') {
    body = (
      <MissingWsl
        command={
          <>
            <p>
              WSL is not installed. To use Linux, open PowerShell as administrator and run: <code className="font-mono">{status.command}</code>
            </p>
            <p className="text-xs text-pm-muted">Puppet Master does not install it for you.</p>
          </>
        }
        onCopy={() => copyText(status.command)}
        onAgain={() => void checkAgain()}
      />
    );
  } else if (status.state === 'no_distro') {
    body = (
      <MissingWsl
        command={
          <p>
            WSL has no Linux yet. Run: <code className="font-mono">{status.command}</code>
          </p>
        }
        onCopy={() => copyText(status.command)}
        onAgain={() => void checkAgain()}
      />
    );
  } else if (status.state === 'broken') {
    body = (
      <MissingWsl
        command={
          <>
            <p>WSL did not answer as expected.</p>
            {status.line && <code className="font-mono text-xs text-pm-muted">{status.line}</code>}
          </>
        }
        onAgain={() => void checkAgain()}
      />
    );
  } else {
    body = (
      <div className="space-y-3">
        <ul className="space-y-2 text-sm" aria-label="Linux distros">
          {status.distros.map((d) => {
            const view = views[d.name];
            const busy = checking === d.name;
            return (
              <li key={d.name} data-distro={d.name} className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-pm-border px-3 py-2">
                <span className="font-medium">{`${d.name}${d.default ? ' (default)' : ''} · WSL ${d.version}`}</span>
                {d.version !== 2 && <span className="text-xs text-pm-muted">{`Uses WSL 1. Agents need WSL 2: wsl --set-version ${d.name} 2`}</span>}
                <span className="ml-auto flex items-center gap-2">
                  <Btn small disabled={d.version !== 2 || checking !== null} onClick={() => void check(d.name)}>
                    {`Check ${d.name}`}
                  </Btn>
                </span>
                {busy && (
                  <span className="flex basis-full items-center gap-2 text-xs text-pm-muted">
                    <DotSpinner status="running" size={3} label={`Checking ${d.name}`} />
                    {`Checking ${d.name}. Starting it can take a few seconds.`}
                  </span>
                )}
                {!busy && view && <span className="basis-full text-xs text-pm-muted">{checkedText(view)}</span>}
              </li>
            );
          })}
        </ul>
        {rows.length > 0 ? (
          <ol className="space-y-2 text-sm" aria-label="Where each agent runs">
            {rows.map((row) => {
              const found = foundText(row, views);
              const locked = isLockedToWindows(row);
              return (
                <li key={row.agentId} data-agent-id={row.agentId} className="flex flex-col gap-1 rounded-lg border border-pm-border px-3 py-2">
                  <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                    <span className="font-medium">{row.label}</span>
                    <span className="text-xs text-pm-muted">Runs on</span>
                    <Popup
                      aria-label={`Runs on for ${row.label}`}
                      value={row.distro ?? WINDOWS_VALUE}
                      disabled={locked}
                      onChange={(event) => void choose(row, event.target.value)}
                    >
                      <option value={WINDOWS_VALUE}>Windows</option>
                      {linuxOptions(status.distros, row).map((name) => (
                        <option key={name} value={name} disabled={Boolean(row.whyNot)}>
                          {`Linux · ${name}`}
                        </option>
                      ))}
                    </Popup>
                  </div>
                  {found && <span className="text-xs text-pm-muted">{found}</span>}
                  {row.whyNot && <span className="text-xs text-pm-muted">{row.whyNot}</span>}
                </li>
              );
            })}
          </ol>
        ) : (
          <p className="text-sm text-pm-muted">No agents can run in Linux yet.</p>
        )}
        <p className="text-xs text-pm-muted">{FOOTER}</p>
      </div>
    );
  }

  return (
    <SettingBlock {...HEADING}>
      {body}
      {error && <Note role="alert">{error}</Note>}
    </SettingBlock>
  );
}
