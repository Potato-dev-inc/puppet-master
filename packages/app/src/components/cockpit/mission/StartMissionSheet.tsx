// "+" → Start a mission… (DESIGN-MISSIONS 6.2): the goal, the checks it ends on, who works on it
// (one at a time, in this order), the budget and where. "Start mission" stays disabled with a line
// that says why. Every agent works in a visible pane; nothing merges on its own.
import { useEffect, useMemo, useState } from 'react';
import {
  commandText,
  goalNamesTests,
  parseCommand,
  startBlockedReason,
  tauriMissionsApi,
  type DetectedCheck,
  type MissionView,
  type MissionsApi,
} from '../../../lib/missions';
import type { LaunchEntry } from '../NewAgentMenu';
import { Sheet } from '../Sheet';
import { PreviewTag } from './MissionGlyph';
import '../../../styles/cockpit-changes.css';
import '../../../styles/missions.css';

export interface SheetProject {
  path: string;
  name: string;
  /** The folder is a git project with a commit (a separate copy can be made). */
  git: boolean;
  /** The branch the copy starts from, when known ("main"). */
  branch?: string;
}

type Check = DetectedCheck & { on: boolean };

function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className="ck-switch"
      disabled={disabled}
      onClick={() => onChange(!on)}
    >
      <span className="ck-switch-knob" />
    </button>
  );
}

export function StartMissionSheet({
  project,
  entries,
  activeMissions,
  api = tauriMissionsApi,
  onClose,
  onStarted,
  initialChecks,
}: {
  project: SheetProject;
  /** The "+" menu's agents (labels and ids). */
  entries: LaunchEntry[];
  activeMissions: number;
  api?: MissionsApi;
  onClose: () => void;
  onStarted: (mission: MissionView) => void;
  /** Harness/tests: skip detection. */
  initialChecks?: DetectedCheck[];
}) {
  const usable = useMemo(() => entries.filter((e) => !e.disabled), [entries]);
  const label = (id: string) => usable.find((e) => e.type === id)?.label ?? id;
  const [objective, setObjective] = useState('');
  const [checks, setChecks] = useState<Check[]>(initialChecks ?? []);
  const [adding, setAdding] = useState('');
  const [agents, setAgents] = useState<string[]>(usable.length ? [usable[0].type] : []);
  const [turns, setTurns] = useState(30);
  const [hours, setHours] = useState(2);
  const [rollback, setRollback] = useState(true);
  const [turnMin, setTurnMin] = useState(30);
  const [checkMin, setCheckMin] = useState(5);
  const [more, setMore] = useState(false);
  const [mayChange, setMayChange] = useState(false);
  const [touchedMayChange, setTouchedMayChange] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialChecks) return;
    let cancelled = false;
    api
      .detect(project.path)
      .then((found) => !cancelled && setChecks(found.map((c) => ({ ...c }))))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [api, project.path, initialChecks]);

  // The goal names tests: agents may change them (with a visible line), unless the person chose.
  const autoTests = goalNamesTests(objective);
  const mayChangeTests = touchedMayChange ? mayChange : autoTests;

  const blocked = startBlockedReason({ objective, checks, agents, turns, hours, rollback, git: project.git, activeMissions });

  const start = async () => {
    if (blocked || busy) return;
    setBusy(true);
    setError(null);
    try {
      const mission = await api.start({
        project_path: project.path,
        objective,
        checks: checks.filter((c) => c.on).map((c) => ({ name: c.name, command: c.command })),
        agents,
        may_change_tests: mayChangeTests,
        rollback,
        budget_turns: turns > 0 ? turns : undefined,
        budget_minutes: hours > 0 ? Math.round(hours * 60) : undefined,
        round_timeout_min: turnMin,
        check_timeout_min: checkMin,
      });
      onStarted(mission);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  };

  const addCheck = () => {
    const command = parseCommand(adding);
    if (command.length === 0) return;
    setChecks((list) => [...list, { name: 'Check', command, source: 'added by you', on: true }]);
    setAdding('');
  };

  const backups = usable.filter((e) => !agents.includes(e.type));

  return (
    <Sheet labelledBy="ck-mission-t" role="dialog" className="ck-mission-sheet" onClose={busy ? undefined : onClose}>
      <div className="ck-mission-sheet-head">
        <h2 className="ck-section" id="ck-mission-t">
          New mission
        </h2>
        <PreviewTag />
      </div>

      <label className="ck-mission-label" htmlFor="ck-mission-goal">
        What should be true when it&apos;s done?
      </label>
      <textarea
        id="ck-mission-goal"
        className="ck-mission-goal"
        rows={2}
        value={objective}
        maxLength={2000}
        placeholder="The cart tests pass and totals round to cents."
        onChange={(e) => setObjective(e.target.value)}
        autoFocus
      />

      <div className="ck-mission-group-head">
        <span className="ck-mission-label">How we&apos;ll know</span>
        <span className="ck-mission-hint">At least one check is needed.</span>
      </div>
      <ul className="ck-mission-checks" aria-label="Checks">
        {checks.map((c, i) => (
          <li key={`${c.name}-${i}`} className="ck-mission-check">
            <Switch
              on={c.on}
              label={c.name}
              onChange={(on) => setChecks((list) => list.map((x, j) => (j === i ? { ...x, on } : x)))}
            />
            <span className="ck-mission-check-name">{c.name}</span>
            <span className="ck-mission-check-cmd">{commandText(c.command)}</span>
            <span className="ck-mission-hint">{c.source}</span>
          </li>
        ))}
        <li className="ck-mission-check ck-mission-check--add">
          <input
            className="ck-mission-input"
            aria-label="Add a check"
            placeholder="+ Add a check, like npm run lint"
            value={adding}
            onChange={(e) => setAdding(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addCheck()}
          />
          {adding.trim() && (
            <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={addCheck}>
              Add
            </button>
          )}
        </li>
        <li className="ck-mission-check">
          <Switch
            on={mayChangeTests}
            label="Agents may change tests"
            onChange={(on) => {
              setTouchedMayChange(true);
              setMayChange(on);
            }}
          />
          <span className="ck-mission-check-name">Agents may change tests</span>
        </li>
      </ul>
      <p className="ck-mission-note">
        {mayChangeTests
          ? autoTests && !touchedMayChange
            ? "Your goal mentions tests, so agents may change them. Changes are listed when it's done."
            : "Changes to tests are listed when it's done."
          : 'Off: the mission stops and asks you if tests are skipped, removed or loosened.'}
      </p>

      <div className="ck-mission-group-head">
        <span className="ck-mission-label">Who works on it</span>
        <span className="ck-mission-hint">One at a time, in this order</span>
      </div>
      <div className="ck-mission-agents">
        {agents.map((a, i) => (
          <span key={a} className="ck-mission-agent">
            <span className="ck-mission-agent-n">{i + 1}</span>
            <span>{label(a)}</span>
            {agents.length > 1 && (
              <button type="button" className="ck-mission-x" aria-label={`Remove ${label(a)}`} onClick={() => setAgents((l) => l.filter((x) => x !== a))}>
                ×
              </button>
            )}
          </span>
        ))}
        {backups.length > 0 && agents.length < 5 && (
          <select
            className="ck-mission-select"
            aria-label="Add a backup agent"
            value=""
            onChange={(e) => e.target.value && setAgents((l) => [...l, e.target.value])}
          >
            <option value="">{agents.length ? 'Add a backup agent…' : 'Pick an agent…'}</option>
            {backups.map((e) => (
              <option key={e.type} value={e.type}>
                {e.label}
              </option>
            ))}
          </select>
        )}
      </div>

      <div className="ck-mission-group-head">
        <span className="ck-mission-label">Budget</span>
        <span className="ck-mission-hint">It pauses when either runs out. Time counts from the start.</span>
      </div>
      <div className="ck-mission-budget">
        <label>
          Turns
          <input className="ck-mission-num" type="number" min={0} max={10000} value={turns} onChange={(e) => setTurns(Math.max(0, Number(e.target.value) || 0))} />
        </label>
        <label>
          Time (h)
          <input className="ck-mission-num" type="number" min={0} max={168} step={0.5} value={hours} onChange={(e) => setHours(Math.max(0, Number(e.target.value) || 0))} />
        </label>
      </div>

      <div className="ck-mission-group-head">
        <span className="ck-mission-label">Where</span>
      </div>
      <p className="ck-mission-where">
        {project.name} · in a separate copy{project.branch ? ` from ${project.branch}` : ''}. You review and merge at the end.
      </p>

      <button type="button" className="ck-mission-more" aria-expanded={more} onClick={() => setMore((v) => !v)}>
        {more ? '▾' : '▸'} More
      </button>
      {more && (
        <div className="ck-mission-more-body">
          <div className="ck-mission-check">
            <Switch on={rollback} label="Go back when checks get worse" onChange={setRollback} />
            <span className="ck-mission-check-name">Go back when checks get worse</span>
          </div>
          <label className="ck-mission-inline">
            Stop a turn after
            <input className="ck-mission-num" type="number" min={1} max={240} value={turnMin} onChange={(e) => setTurnMin(Math.min(240, Math.max(1, Number(e.target.value) || 30)))} />
            min
          </label>
          <label className="ck-mission-inline">
            Stop a check after
            <input className="ck-mission-num" type="number" min={1} max={10} value={checkMin} onChange={(e) => setCheckMin(Math.min(10, Math.max(1, Number(e.target.value) || 5)))} />
            min
          </label>
        </div>
      )}

      {error && (
        <p className="ck-mission-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={!!blocked || busy} title={blocked ?? undefined} onClick={() => void start()}>
          {busy ? 'Starting…' : 'Start mission'}
        </button>
      </div>
      {blocked && <p className="ck-mission-blocked">{blocked}</p>}
      <p className="ck-mission-foot">Missions are new. Every agent works in a visible pane so you can watch.</p>
    </Sheet>
  );
}
