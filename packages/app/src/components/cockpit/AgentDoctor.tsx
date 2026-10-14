// "Set up agents": which of my AI agents work, and how do I fix the ones that do not?
// Monochrome list; the glyph reuses DotSpinner's static patterns:
//   idle = installed or missing but not tested, exited (check) = ready,
//   waiting_input (plus) = needs you to sign in, error (cross) = problem.
import { useCallback, useEffect, useRef, useState } from 'react';
import { agentHealthApi, type AgentHealth, type AgentHealthApi, type HealthState } from '../../lib/agent-health';
import { agentInstallApi, type AgentInstallApi, type InstallPlan } from '../../lib/agent-install';
import { DotSpinner, type DotSpinnerProps } from './DotSpinner';
import { InstallSheet } from './InstallSheet';
import '../../styles/agent-doctor.css';

export function glyphFor(state: HealthState): DotSpinnerProps['status'] {
  switch (state) {
    case 'ready':
      return 'exited';
    case 'needs_login':
      return 'waiting_input';
    case 'error':
      return 'error';
    default:
      return 'idle';
  }
}

export function stateLabel(state: HealthState): string {
  switch (state) {
    case 'ready':
      return 'Ready';
    case 'needs_login':
      return 'Needs login';
    case 'error':
      return 'Problem';
    case 'not_installed':
      return 'Not installed';
    default:
      return 'Not tested';
  }
}

export interface AgentDoctorProps {
  /** Starts the agent in a visible pane so the user can sign in. */
  onOpenAgent: (agentId: string) => void;
  onNotify?: (text: string) => void;
  onClose?: () => void;
  api?: AgentHealthApi;
  installApi?: AgentInstallApi;
  /** Brings the install window (a visible pane) to the front. */
  onShowPane?: (paneId: string) => void;
}

/** Where one row's install is. Absent = nothing going on. */
type Install =
  | { phase: 'loading' }
  | { phase: 'running'; paneId: string; label: string; tool: string | null }
  | { phase: 'finishing' }
  | { phase: 'result'; tone: 'ok' | 'problem'; text: string };

const POLL_MS = 1500;

/** Installs outlive this screen: Show window leaves it, and the install goes on in its window. */
let keptInstalls: Record<string, Install> = {};
export function resetKeptInstalls(): void {
  keptInstalls = {};
}

export function AgentDoctor({
  onOpenAgent,
  onNotify,
  onClose,
  api = agentHealthApi,
  installApi = agentInstallApi,
  onShowPane,
}: AgentDoctorProps) {
  const [rows, setRows] = useState<AgentHealth[] | null>(null);
  const [installs, setInstalls] = useState<Record<string, Install>>(() => keptInstalls);
  const [sheet, setSheet] = useState<{ agentId: string; plan: InstallPlan; busy: boolean; error: string | null } | null>(
    null,
  );
  const installsRef = useRef(installs);
  installsRef.current = installs;
  keptInstalls = installs;
  const [loadError, setLoadError] = useState<string | null>(null);
  const [testing, setTesting] = useState<Set<string>>(new Set());

  const load = useCallback(() => {
    setLoadError(null);
    api
      .check()
      .then(setRows)
      .catch((e) => setLoadError(e instanceof Error ? e.message : String(e)));
  }, [api]);

  useEffect(load, [load]);

  const test = (id: string) => {
    setTesting((s) => new Set(s).add(id));
    api
      .test(id)
      .then((result) => setRows((cur) => (cur ?? []).map((r) => (r.id === id ? result : r))))
      .catch((e) =>
        setRows((cur) =>
          (cur ?? []).map((r) =>
            r.id === id
              ? { ...r, state: 'error', reason: `The test could not run: ${e instanceof Error ? e.message : String(e)}` }
              : r,
          ),
        ),
      )
      .finally(() =>
        setTesting((s) => {
          const next = new Set(s);
          next.delete(id);
          return next;
        }),
      );
  };

  const setInstall = (id: string, next: Install | null) =>
    setInstalls((cur) => {
      const copy = { ...cur };
      if (next) copy[id] = next;
      else delete copy[id];
      return copy;
    });

  const errText = (e: unknown) => (e instanceof Error ? e.message : String(e)).replace(/^[A-Z_]+: /, '');

  /** Install: fetch the exact commands and show them. Nothing runs yet. */
  const openInstall = (id: string) => {
    setInstall(id, { phase: 'loading' });
    installApi
      .plan(id)
      .then((plan) => {
        setInstall(id, null);
        setSheet({ agentId: id, plan, busy: false, error: null });
      })
      .catch((e) => setInstall(id, { phase: 'result', tone: 'problem', text: errText(e) }));
  };

  /** Run: the confirmed step goes to a visible window. */
  const runInstall = (acknowledgedRisk: boolean) => {
    if (!sheet) return;
    const { agentId, plan } = sheet;
    const step = plan.steps[0];
    setSheet({ ...sheet, busy: true, error: null });
    installApi
      .run(agentId, step.index, step.command, acknowledgedRisk)
      .then((started) => {
        setSheet(null);
        setInstall(agentId, { phase: 'running', paneId: started.pane_id, label: step.label, tool: step.tool });
      })
      .catch((e) => setSheet((cur) => (cur ? { ...cur, busy: false, error: errText(e) } : cur)));
  };

  /** The window's command ended: refresh PATH, check the agent again, say what happened. */
  const finishInstall = (id: string, tool: string | null, exitCode: number | null, message: string | null) => {
    setInstall(id, { phase: 'finishing' });
    installApi
      .finish(id)
      .then((res) => {
        setRows((cur) => (cur ?? []).map((r) => (r.id === id ? res.health : r)));
        if (res.health.installed) {
          setInstall(id, { phase: 'result', tone: 'ok', text: 'Installed. Sign in next.' });
        } else if (tool && exitCode === 0) {
          // A prerequisite is in place: ask about the next step (a fresh plan, so it is accurate).
          openInstall(id);
        } else if (exitCode === 0) {
          setInstall(id, {
            phase: 'result',
            tone: 'problem',
            text: 'Installed, but Puppet Master cannot find it yet. Close Puppet Master and open it again.',
          });
        } else {
          setInstall(id, {
            phase: 'result',
            tone: 'problem',
            text: `${message ?? 'The install did not finish.'} Your PC is fine. You can try again.`,
          });
        }
      })
      .catch((e) => setInstall(id, { phase: 'result', tone: 'problem', text: errText(e) }));
  };

  // While an install window is open, ask whether its command has finished.
  const runningKey = Object.entries(installs)
    .filter(([, v]) => v.phase === 'running')
    .map(([k]) => k)
    .join(',');
  useEffect(() => {
    if (!runningKey) return;
    const timer = window.setInterval(() => {
      for (const id of runningKey.split(',')) {
        const cur = installsRef.current[id];
        if (!cur || cur.phase !== 'running') continue;
        installApi
          .status(cur.paneId)
          .then((st) => {
            const now = installsRef.current[id];
            if (!now || now.phase !== 'running' || now.paneId !== cur.paneId) return;
            if (st.done) finishInstall(id, cur.tool, st.exit_code, st.message);
          })
          .catch(() => {});
      }
    }, POLL_MS);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runningKey, installApi]);

  const copy = (text: string) => {
    const done = () => onNotify?.('Copied. Paste it into a terminal and press Enter.');
    const failed = () => onNotify?.('Could not copy. Select the command and copy it by hand.');
    if (!navigator.clipboard) return failed();
    navigator.clipboard.writeText(text).then(done).catch(failed);
  };

  const readyCount = rows?.filter((r) => r.state === 'ready').length ?? 0;

  return (
    <div className="ck-doctor">
      <header className="ck-doctor-head">
        <h1 className="ck-title">Set up your agents</h1>
        <p className="ck-sub">
          Agents are the AI helpers that do the work. Here is which ones are on this computer and what to do about the
          rest.
          {readyCount > 0 ? ` ${readyCount} tested and ready.` : ''}
        </p>
        <div className="ck-doctor-bar">
          <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={load}>
            Check again
          </button>
          {onClose && (
            <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={onClose}>
              Close
            </button>
          )}
        </div>
      </header>

      {loadError && <p className="ck-sub ck-doctor-error">Could not check your agents: {loadError}</p>}
      {!rows && !loadError && <p className="ck-sub">Looking for your agents</p>}

      {rows && (
        <ul className="ck-list ck-doctor-list" aria-label="Agents">
          {rows.map((r) => {
            const busy = testing.has(r.id);
            const inst = installs[r.id];
            const working = inst?.phase === 'loading' || inst?.phase === 'running' || inst?.phase === 'finishing';
            return (
              <li key={r.id} className="ck-doctor-row" data-state={r.state}>
                <span className="ck-row-glyph">
                  <DotSpinner status={busy || working ? 'running' : glyphFor(r.state)} />
                </span>
                <div className="ck-doctor-main">
                  <div className="ck-doctor-line">
                    <span className="ck-row-title">{r.label}</span>
                    <span className="ck-doctor-state">
                      {busy ? 'Testing' : working ? (inst?.phase === 'finishing' ? 'Checking' : 'Installing') : stateLabel(r.state)}
                    </span>
                    {r.version && <span className="ck-doctor-version">{r.version}</span>}
                  </div>
                  <p className="ck-doctor-reason">
                    {busy
                      ? 'Asking it a tiny question. This can take a minute.'
                      : inst?.phase === 'running'
                        ? `Installing ${inst.label} in the window. Puppet Master checks again when it is done.`
                        : inst?.phase === 'finishing'
                          ? 'Checking that it works.'
                          : r.reason}
                  </p>
                  {inst?.phase === 'result' && (
                    <p className="ck-doctor-hint ck-doctor-result" data-tone={inst.tone} role="status">
                      {inst.text}
                    </p>
                  )}
                  {r.state === 'not_installed' && !working && r.install_hint && <code className="ck-doctor-cmd">{r.install_hint}</code>}
                  {r.state === 'not_installed' && !working && r.install_fallback && (
                    <p className="ck-doctor-hint">
                      <span className="ck-sub">Or with npm: </span>
                      <code className="ck-doctor-cmd">{r.install_fallback}</code>
                    </p>
                  )}
                  {r.state === 'needs_login' && r.login_hint && <p className="ck-doctor-hint">{r.login_hint}</p>}
                  {(r.state === 'needs_login' || (r.installed && inst?.phase === 'result' && inst.tone === 'ok')) && (
                    <p className="ck-doctor-hint">A window opens where you sign in. Puppet Master never sees your password.</p>
                  )}
                </div>
                <div className="ck-doctor-actions">
                  {inst?.phase === 'running' && onShowPane && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => onShowPane(inst.paneId)}
                    >
                      Show window
                    </button>
                  )}
                  {r.state === 'not_installed' && !working && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--primary ck-btn--sm"
                      onClick={() => openInstall(r.id)}
                      title="Shows the command first. Nothing runs until you press Run."
                    >
                      Install
                    </button>
                  )}
                  {r.state === 'not_installed' && r.install_hint && !working && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => copy(r.install_hint!)}
                    >
                      Copy install command
                    </button>
                  )}
                  {r.installed && r.state !== 'ready' && !working && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => onOpenAgent(r.id)}
                      title={r.login_hint ?? undefined}
                    >
                      Sign in
                    </button>
                  )}
                  {r.installed && r.can_test && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--primary ck-btn--sm"
                      disabled={busy}
                      onClick={() => test(r.id)}
                      title="Asks the agent one tiny question. Uses a few tokens."
                    >
                      Test
                    </button>
                  )}
                </div>
              </li>
            );
          })}
          {rows.length === 0 && <li className="ck-sub ck-pad">No agents are registered.</li>}
        </ul>
      )}
      {sheet && (
        <InstallSheet
          plan={sheet.plan}
          busy={sheet.busy}
          error={sheet.error}
          onRun={runInstall}
          onCancel={() => setSheet(null)}
          onCopy={copy}
        />
      )}
    </div>
  );
}
