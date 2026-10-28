// "Save this team": the pane and the helpers under it, as a crew you can start again in any folder. The draft comes from
// the live panes (folder -> {folder}, secrets and personal folder names taken out); the person edits names and tasks here
// before anything is saved.
import { useEffect, useState } from 'react';
import { crewErrorText, crewsApi, type Crew, type CrewProblem, type CrewsApi } from '../../../lib/crews-api';
import { Sheet } from '../Sheet';
import { blockedReason, dropRole, MAX_CREW_NAME, MAX_ROLE_NAME, renameRole, roleLine, setTask } from './model';
import '../../../styles/cockpit-crews.css';

export function SaveCrewSheet({
  paneId,
  api = crewsApi,
  agentName,
  onClose,
  onSaved,
}: {
  /** The pane whose helpers are saved with it. */
  paneId: string;
  api?: CrewsApi;
  /** Display name of an agent type ("Codex"). */
  agentName: (agentType: string) => string;
  onClose: () => void;
  onSaved: (crewName: string) => void;
}) {
  const [crew, setCrew] = useState<Crew | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [problems, setProblems] = useState<CrewProblem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api
      .draft(paneId)
      .then((d) => {
        if (!live) return;
        setCrew(d.crew);
        setOmitted(d.omitted);
      })
      .catch((err) => live && setError(crewErrorText(err)));
    return () => {
      live = false;
    };
  }, [api, paneId]);

  const edit = (next: Crew) => {
    setCrew(next);
    setProblems([]);
    setError(null);
  };

  const blocked = crew ? blockedReason(crew) : null;
  const save = async () => {
    if (!crew || blocked) return;
    setBusy(true);
    setError(null);
    try {
      const out = await api.save(crew);
      if (out.slug) {
        onSaved(crew.name.trim());
        return;
      }
      setProblems(out.errors);
      if (out.errors.some((e) => !e.role)) setError(out.errors.filter((e) => !e.role).map((e) => e.message).join(' '));
    } catch (err) {
      setError(crewErrorText(err));
    }
    setBusy(false);
  };

  return (
    <Sheet labelledBy="ck-crew-save-t" role="dialog" className="ck-sheet--crew" onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id="ck-crew-save-t">
        Save this team
      </h2>
      <p className="ck-sub">
        Saves each agent, its first task and who started whom. The folder becomes <code>{'{folder}'}</code>, so you can start this crew in any folder. Secrets and
        personal folder names are taken out.
      </p>

      {!crew && !error && <p className="ck-sub">Reading the team…</p>}

      {crew && (
        <>
          <label className="ck-pr-field">
            <span>Crew name</span>
            <input
              className="ck-pr-input"
              value={crew.name}
              maxLength={MAX_CREW_NAME}
              onChange={(e) => edit({ ...crew, name: e.target.value })}
              disabled={busy}
              aria-label="Crew name"
            />
          </label>
          <ul className="ck-crew-roles">
            {crew.roles.map((role, i) => {
              const own = problems.filter((p) => p.role === role.name);
              return (
                <li key={i} className="ck-crew-role" data-testid="crew-role">
                  <div className="ck-crew-role-head">
                    <input
                      className="ck-pr-input ck-crew-role-name"
                      value={role.name}
                      maxLength={MAX_ROLE_NAME}
                      onChange={(e) => edit(renameRole(crew, i, e.target.value))}
                      disabled={busy}
                      aria-label={`Name of role ${i + 1}`}
                    />
                    <span className="ck-crew-role-meta">
                      {agentName(role.agent_type)} · {roleLine(role)}
                    </span>
                    {role.parent && (
                      <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => edit(dropRole(crew, i))} disabled={busy} title={`Leave ${role.name} out`}>
                        Leave out
                      </button>
                    )}
                  </div>
                  <textarea
                    className="ck-pr-body ck-crew-role-task"
                    value={role.task}
                    onChange={(e) => edit(setTask(crew, i, e.target.value))}
                    disabled={busy}
                    spellCheck={false}
                    rows={3}
                    aria-label={`Task of ${role.name || `role ${i + 1}`}`}
                  />
                  {own.map((p) => (
                    <p key={p.code} className="ck-changes-error" role="alert">
                      {p.message}
                    </p>
                  ))}
                </li>
              );
            })}
          </ul>
          {omitted > 0 && (
            <p className="ck-sub" data-testid="crew-omitted">
              {omitted} more {omitted === 1 ? 'helper was' : 'helpers were'} left out. A crew holds at most 8 agents.
            </p>
          )}
        </>
      )}

      {blocked && crew && (
        <p className="ck-sub" role="status">
          {blocked}
        </p>
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
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy || !crew || !!blocked} onClick={() => void save()}>
          {busy ? 'Saving…' : 'Save crew'}
        </button>
      </div>
    </Sheet>
  );
}
