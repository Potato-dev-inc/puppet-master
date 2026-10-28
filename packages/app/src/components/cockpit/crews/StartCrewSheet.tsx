// "Start crew…": pick a saved crew and a folder, then start it. The roles start one at a time (the lead first, each
// helper under the pane of its parent); if one fails, the rest wait and the sheet says which. A started agent that asks
// something (trust, permission) asks in its own pane like any other; nothing here answers for it.
import { useCallback, useEffect, useState } from 'react';
import { crewErrorText, crewsApi, type CrewStartOutcome, type CrewSummary, type CrewsApi } from '../../../lib/crews-api';
import { shipApi, type ShipApi } from '../../../lib/ship-api';
import { Sheet } from '../Sheet';
import { failureText } from './model';
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
  const [crews, setCrews] = useState<CrewSummary[] | null>(null);
  const [slug, setSlug] = useState<string | null>(null);
  const [folder, setFolder] = useState<string | null>(defaultFolder ?? null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<CrewStartOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .list()
      .then((list) => {
        setCrews(list);
        setSlug((cur) => (cur && list.some((c) => c.slug === cur) ? cur : (list.find((c) => !c.problem)?.slug ?? null)));
      })
      .catch((err) => setError(crewErrorText(err)));
  }, [api]);
  useEffect(load, [load]);

  const chosen = crews?.find((c) => c.slug === slug) ?? null;
  const canStart = !!chosen && !chosen.problem && !!folder && !busy;

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
    if (!chosen || !folder) return;
    setBusy(true);
    setError(null);
    try {
      setOutcome(await api.start(chosen.slug, folder));
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

  return (
    <Sheet labelledBy="ck-crew-start-t" role="dialog" className="ck-sheet--crew" onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id="ck-crew-start-t">
        Start crew
      </h2>
      {!crews && !error && <p className="ck-sub">Looking for saved crews…</p>}
      {crews && crews.length === 0 && (
        <p className="ck-sub" data-testid="crew-empty">
          No crews yet. When an agent has helpers, choose Save this team in its side panel.
        </p>
      )}
      {crews && crews.length > 0 && (
        <ul className="ck-crew-list" role="radiogroup" aria-label="Saved crews">
          {crews.map((c) => (
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
                  {c.agents.length > 0 && ` · ${c.agents.map(agentName).join(', ')}`}
                </span>
                {c.problem && <span className="ck-crew-problem">{c.problem}</span>}
              </button>
              <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => void remove(c)} disabled={busy} title={`Remove ${c.name}`}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      {crews && crews.length > 0 && (
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
