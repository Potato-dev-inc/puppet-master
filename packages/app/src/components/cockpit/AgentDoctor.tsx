// "Set up agents": which of my AI agents work, and how do I fix the ones that do not?
// Monochrome list; the glyph reuses DotSpinner's static patterns:
//   idle = installed or missing but not tested, exited (check) = ready,
//   waiting_input (plus) = needs you to sign in, error (cross) = problem.
import { useCallback, useEffect, useState } from 'react';
import { agentHealthApi, type AgentHealth, type AgentHealthApi, type HealthState } from '../../lib/agent-health';
import { DotSpinner, type DotSpinnerProps } from './DotSpinner';
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
}

export function AgentDoctor({ onOpenAgent, onNotify, onClose, api = agentHealthApi }: AgentDoctorProps) {
  const [rows, setRows] = useState<AgentHealth[] | null>(null);
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
            return (
              <li key={r.id} className="ck-doctor-row" data-state={r.state}>
                <span className="ck-row-glyph">
                  <DotSpinner status={busy ? 'running' : glyphFor(r.state)} />
                </span>
                <div className="ck-doctor-main">
                  <div className="ck-doctor-line">
                    <span className="ck-row-title">{r.label}</span>
                    <span className="ck-doctor-state">{busy ? 'Testing' : stateLabel(r.state)}</span>
                    {r.version && <span className="ck-doctor-version">{r.version}</span>}
                  </div>
                  <p className="ck-doctor-reason">
                    {busy ? 'Asking it a tiny question. This can take a minute.' : r.reason}
                  </p>
                  {r.state === 'not_installed' && r.install_hint && <code className="ck-doctor-cmd">{r.install_hint}</code>}
                  {r.state === 'needs_login' && r.login_hint && <p className="ck-doctor-hint">{r.login_hint}</p>}
                </div>
                <div className="ck-doctor-actions">
                  {r.state === 'not_installed' && r.install_hint && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => copy(r.install_hint!)}
                    >
                      Copy install command
                    </button>
                  )}
                  {r.installed && (
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => onOpenAgent(r.id)}
                      title={r.login_hint ?? undefined}
                    >
                      Open to log in
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
    </div>
  );
}
