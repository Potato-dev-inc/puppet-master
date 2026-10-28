// "Start crew…": pick a crew (one of yours, or a starter) and a folder, check which agent each role uses, then start it.
// The roles start one at a time (the lead first, each helper under the pane of its parent); if one fails, the rest wait
// and the sheet says which. A started agent that asks something (trust, permission) asks in its own pane like any other;
// nothing here answers for it. A starter can be copied to My crews, where it is edited and saved like any crew.
import type { ReactNode } from 'react';
import { useCallback, useEffect, useState } from 'react';
import {
  crewErrorText,
  crewsApi,
  DEFAULT_AGENT,
  type Crew,
  type CrewAgents,
  type CrewRole,
  type CrewStartOutcome,
  type CrewSummary,
  type CrewsApi,
} from '../../../lib/crews-api';
import { shipApi, type ShipApi } from '../../../lib/ship-api';
import { Sheet } from '../Sheet';
import { failureText, roleLine } from './model';
import { SaveCrewSheet } from './SaveCrewSheet';
import '../../../styles/cockpit-crews.css';

const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

export function StartCrewSheet({
  api = crewsApi,
  ship = shipApi,
  defaultFolder,
  agentName,
  onClose,
}: {
  api?: CrewsApi;
  ship?: Pick<ShipApi, 'pickFolder'>;
  /** The folder of the pane the person is looking at, as a starting point. */
  defaultFolder?: string | null;
  agentName: (agentType: string) => string;
  onClose: () => void;
}) {
  const [saved, setSaved] = useState<CrewSummary[] | null>(null);
  const [starters, setStarters] = useState<CrewSummary[] | null>(null);
  const [agents, setAgents] = useState<CrewAgents | null>(null);
  const [slug, setSlug] = useState<string | null>(null);
  /** The roles of the crew picked last, with the agent each one will use (role name -> agent id). */
  const [picked, setPicked] = useState<{ slug: string; roles: CrewRole[] } | null>(null);
  const [chosen, setChosen] = useState<Record<string, string>>({});
  const [folder, setFolder] = useState<string | null>(defaultFolder ?? null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<CrewStartOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** A starter being copied: the same editor as "Save this team", with nothing saved until Save. */
  const [copying, setCopying] = useState<Crew | null>(null);

  const defaultAgent = agents?.default ?? null;
  /** A starter role shows the default agent's name until the crew starts. */
  const label = (id: string) => (id === DEFAULT_AGENT ? 'Default agent' : agentName(id));

  const load = useCallback(() => {
    Promise.all([api.list(), api.starters(), api.agents()])
      .then(([mine, built, choices]) => {
        setSaved(mine);
        setStarters(built);
        setAgents(choices);
        setSlug((cur) => {
          const all = [...mine, ...built];
          return cur && all.some((c) => c.slug === cur) ? cur : (all.find((c) => !c.problem)?.slug ?? null);
        });
      })
      .catch((err) => setError(crewErrorText(err)));
  }, [api]);
  useEffect(load, [load]);

  // The roles of the crew picked, each with its agent: a `{default}` role takes the default agent.
  useEffect(() => {
    if (!slug) return;
    let live = true;
    api
      .get(slug)
      .then((crew) => {
        if (!live) return;
        setPicked({ slug, roles: crew.roles });
        setChosen(Object.fromEntries(crew.roles.map((r) => [r.name, r.agent_type === DEFAULT_AGENT ? (defaultAgent ?? '') : r.agent_type])));
      })
      .catch((err) => live && setError(crewErrorText(err)));
    return () => {
      live = false;
    };
  }, [api, slug, defaultAgent]);

  const all = [...(saved ?? []), ...(starters ?? [])];
  const chosenCrew = all.find((c) => c.slug === slug) ?? null;
  const roles = picked?.slug === slug ? picked.roles : null;
  const ready = !!roles && roles.length > 0 && roles.every((r) => !!chosen[r.name]);
  const canStart = !!chosenCrew && !chosenCrew.problem && !!folder && !busy && ready;

  const pick = async () => {
    setError(null);
    try {
      const found = await ship.pickFolder();
      if (found) setFolder(found.path);
    } catch (err) {
      setError(crewErrorText(err));
    }
  };

  const start = async () => {
    if (!chosenCrew || !folder) return;
    setBusy(true);
    setError(null);
    try {
      setOutcome(await api.start(chosenCrew.slug, folder, chosen));
    } catch (err) {
      setError(crewErrorText(err));
    }
    setBusy(false);
  };

  const remove = async (c: CrewSummary) => {
    setError(null);
    try {
      await api.remove(c.slug);
      load();
    } catch (err) {
      setError(crewErrorText(err));
    }
  };

  const copy = async (c: CrewSummary) => {
    setError(null);
    setNotice(null);
    try {
      const crew = await api.get(c.slug);
      setCopying({ ...crew, roles: crew.roles.map((r) => ({ ...r, agent_type: r.agent_type === DEFAULT_AGENT ? (defaultAgent ?? r.agent_type) : r.agent_type })) });
    } catch (err) {
      setError(crewErrorText(err));
    }
  };

  if (copying) {
    return (
      <SaveCrewSheet
        initial={copying}
        api={api}
        agentName={label}
        onClose={() => setCopying(null)}
        onSaved={(name) => {
          setCopying(null);
          setNotice(`Copied to My crews as ${name}.`);
          load();
        }}
      />
    );
  }

  if (outcome) {
    const failure = failureText(outcome);
    return (
      <Sheet labelledBy="ck-crew-start-t" role="dialog" className="ck-sheet--crew" onClose={onClose}>
        <h2 className="ck-section" id="ck-crew-start-t">
          {outcome.failed ? 'The crew did not finish starting' : 'Crew started'}
        </h2>
        <ul className="ck-crew-result" data-testid="crew-result">
          {outcome.started.map((s) => (
            <li key={s.pane_id}>
              <span className="ck-crew-mark">Started</span>
              <span className="ck-crew-result-name">{s.role}</span>
              <span className="ck-crew-role-meta">{agentName(s.agent_type)}</span>
            </li>
          ))}
          {outcome.failed && (
            <li className="is-failed">
              <span className="ck-crew-mark">Failed</span>
              <span className="ck-crew-result-name">{outcome.failed.role}</span>
            </li>
          )}
          {outcome.not_started.map((name) => (
            <li key={name} className="is-waiting">
              <span className="ck-crew-mark">Not started</span>
              <span className="ck-crew-result-name">{name}</span>
            </li>
          ))}
        </ul>
        {failure && (
          <p className="ck-changes-error" role="alert">
            {failure}
          </p>
        )}
        {!outcome.failed && <p className="ck-sub">If an agent asks you something, the question waits in its own pane.</p>}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--primary" onClick={onClose} autoFocus>
            Done
          </button>
        </div>
      </Sheet>
    );
  }

  /** One crew in a list: the radio that picks it, and the action beside it (Remove, or Copy for a starter). */
  const row = (c: CrewSummary, action: ReactNode) => (
    <li key={c.slug} className="ck-crew-item">
      <button
        type="button"
        role="radio"
        aria-checked={c.slug === slug}
        className={`ck-crew-pick${c.slug === slug ? ' is-picked' : ''}`}
        onClick={() => setSlug(c.slug)}
        disabled={busy}
      >
        <span className="ck-crew-name">{c.name}</span>
        <span className="ck-crew-role-meta">
          {c.roles} {c.roles === 1 ? 'agent' : 'agents'}
          {c.agents.length > 0 && ` · ${c.agents.map(label).join(', ')}`}
        </span>
        {c.problem && <span className="ck-crew-problem">{c.problem}</span>}
      </button>
      {action}
    </li>
  );

  // A role's agent: the agents that can take a role, plus the one the role has now even if it is not set up.
  const agentOptions = (current: string) => {
    const list = agents?.agents ?? [];
    return current && !list.some((a) => a.id === current) ? [...list, { id: current, label: `${label(current)} (not set up)` }] : list;
  };

  return (
    <Sheet labelledBy="ck-crew-start-t" role="dialog" className="ck-sheet--crew" onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id="ck-crew-start-t">
        Start crew
      </h2>
      {!saved && !error && <p className="ck-sub">Looking for crews…</p>}
      {notice && (
        <p className="ck-sub" role="status">
          {notice}
        </p>
      )}
      {saved && (
        <section aria-labelledby="ck-crew-mine-t">
          <p className="ck-crew-folder-label" id="ck-crew-mine-t">
            My crews
          </p>
          {saved.length === 0 && (
            <p className="ck-sub" data-testid="crew-empty">
              No crews of your own yet. When an agent has helpers, choose Save this team in its side panel. Or copy a starter below.
            </p>
          )}
          {saved.length > 0 && (
            <ul className="ck-crew-list" role="radiogroup" aria-label="My crews">
              {saved.map((c) =>
                row(
                  c,
                  <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => void remove(c)} disabled={busy} title={`Remove ${c.name}`}>
                    Remove
                  </button>,
                ),
              )}
            </ul>
          )}
        </section>
      )}
      {starters && starters.length > 0 && (
        <section aria-labelledby="ck-crew-starters-t">
          <p className="ck-crew-folder-label" id="ck-crew-starters-t">
            Starters
          </p>
          <ul className="ck-crew-list" role="radiogroup" aria-label="Starters">
            {starters.map((c) =>
              row(
                c,
                <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => void copy(c)} disabled={busy} title={`Copy ${c.name} to My crews`}>
                  Copy to My crews
                </button>,
              ),
            )}
          </ul>
        </section>
      )}
      {roles && roles.length > 0 && (
        <div className="ck-crew-agents" data-testid="crew-start-roles">
          <p className="ck-crew-folder-label">Agents</p>
          <ul className="ck-crew-roles">
            {roles.map((role) => {
              const current = chosen[role.name] ?? '';
              return (
                <li key={role.name} className="ck-crew-role">
                  <div className="ck-crew-role-head">
                    <span className="ck-crew-result-name">{role.name}</span>
                    <span className="ck-crew-role-meta">{roleLine(role)}</span>
                    <select
                      className="ck-pr-input"
                      aria-label={`Agent for ${role.name}`}
                      value={current}
                      disabled={busy}
                      onChange={(e) => setChosen((cur) => ({ ...cur, [role.name]: e.target.value }))}
                    >
                      {!current && <option value="">Choose an agent</option>}
                      {agentOptions(current).map((a) => (
                        <option key={a.id} value={a.id}>
                          {a.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </li>
              );
            })}
          </ul>
          {!defaultAgent && roles.some((r) => r.agent_type === DEFAULT_AGENT) && (
            <p className="ck-sub">No agent is set up on this computer for the default role. Pick one above, or set one up first.</p>
          )}
        </div>
      )}
      {saved && starters && (
        <div className="ck-crew-folder">
          <span className="ck-crew-folder-label">Folder</span>
          <span className="ck-crew-folder-path ck-mono" title={folder ?? undefined}>
            {folder ? folderName(folder) : 'None chosen'}
          </span>
          <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => void pick()} disabled={busy}>
            Choose folder…
          </button>
        </div>
      )}
      {error && (
        <p className="ck-changes-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose} disabled={busy} autoFocus>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={!canStart} onClick={() => void start()}>
          {busy ? 'Starting…' : 'Start crew'}
        </button>
      </div>
    </Sheet>
  );
}
