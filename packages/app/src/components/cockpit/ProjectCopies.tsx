// Project view, "Copies" tab: what Puppet Master does so a separate copy starts ready.
//   Set up with       commands run in the new copy (one per line, never through a shell)
//   Copy these files  files copied in from the project, never committed
//   Start with        the command that starts the project (kept for later)
// The settings live in the project, in `.puppet-master/project.toml`. A `conductor.json` can be imported.
import { useEffect, useState } from 'react';
import { shipApi, type PresetView, type ShipApi } from '../../lib/ship-api';
import '../../styles/cockpit-ship.css';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const lines = (text: string): string[] =>
  text
    .split(/\r?\n|,/)
    .map((l) => l.trim())
    .filter(Boolean);

export function ProjectCopies({ project, ship = shipApi }: { project: string | undefined; ship?: ShipApi }) {
  const [view, setView] = useState<PresetView | null>(null);
  const [setup, setSetup] = useState('');
  const [copy, setCopy] = useState('');
  const [run, setRun] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const apply = (next: PresetView) => {
    setView(next);
    setSetup(next.setup.join('\n'));
    setCopy(next.copy.join('\n'));
    setRun(next.run);
  };

  useEffect(() => {
    if (!project) return;
    let live = true;
    ship
      .presetGet(project)
      .then((v) => live && apply(v))
      .catch((err) => live && setError(errText(err)));
    return () => {
      live = false;
    };
  }, [ship, project]);

  if (!project) {
    return <p className="ck-sub ck-pad">Open a project first: select an agent, then come back here.</p>;
  }

  const dirty =
    view !== null &&
    (setup.trim() !== view.setup.join('\n').trim() ||
      lines(copy).join('\n') !== view.copy.join('\n') ||
      run.trim() !== view.run.trim());

  const job = async (work: () => Promise<PresetView>, done: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      apply(await work());
      setNote(done);
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    job(() => ship.presetSet(project, setup.split(/\r?\n/).map((l) => l.trim()).filter(Boolean), lines(copy), run.trim()), 'Saved. The next separate copy uses it.');

  const conductor = view?.conductor ?? null;

  return (
    <div className="ck-copies">
      <h2 className="ck-section">When Puppet Master makes a separate copy</h2>

      <label className="ck-copies-row">
        <span className="ck-copies-label">Set up with</span>
        <textarea
          className="ck-pr-body ck-copies-input"
          aria-label="Set up with"
          rows={3}
          value={setup}
          placeholder={'npm ci'}
          spellCheck={false}
          onChange={(e) => setSetup(e.target.value)}
          disabled={view === null}
        />
        <span className="ck-copies-fine">One command per line. They run in the copy, in order, and the output is kept in a log.</span>
      </label>

      <label className="ck-copies-row">
        <span className="ck-copies-label">Copy these files</span>
        <textarea
          className="ck-pr-body ck-copies-input"
          aria-label="Copy these files"
          rows={2}
          value={copy}
          placeholder={'.env\n.env.local'}
          spellCheck={false}
          onChange={(e) => setCopy(e.target.value)}
          disabled={view === null}
        />
        <span className="ck-copies-fine">Copied, never committed.</span>
      </label>

      <label className="ck-copies-row">
        <span className="ck-copies-label">Start with</span>
        <input
          className="ck-pr-input"
          aria-label="Start with"
          value={run}
          placeholder="npm run dev"
          spellCheck={false}
          onChange={(e) => setRun(e.target.value)}
          disabled={view === null}
        />
        <span className="ck-copies-fine">Kept with the project. Puppet Master does not start it for you yet.</span>
      </label>

      {conductor && (
        <div className="ck-copies-conductor">
          {conductor.unsupported ? (
            <p className="ck-sub">Found conductor.json, but it can&apos;t be imported: {conductor.unsupported}</p>
          ) : (
            <>
              <p className="ck-sub">Found conductor.json in this project.</p>
              <button
                type="button"
                className="ck-btn ck-btn--secondary"
                disabled={busy}
                onClick={() => void job(() => ship.presetImportConductor(project), 'Imported its setup.')}
              >
                Use its setup
              </button>
            </>
          )}
        </div>
      )}

      {view?.problem && (
        <p className="ck-changes-error" role="alert">
          {view.problem}
        </p>
      )}

      <div className="ck-memory-actions">
        <button type="button" className="ck-btn ck-btn--primary" disabled={!dirty || busy} onClick={() => void save()}>
          Save
        </button>
        <span className="ck-memory-msg" role="status" aria-live="polite">
          {error ? <span className="ck-memory-err">{error}</span> : (note ?? (dirty ? 'Unsaved changes' : ''))}
        </span>
      </div>
    </div>
  );
}
