// The Tests chip of a task (SPEC n32 3.10): the commands that prove the work, each one a single editable line with
// Remove. Puppet Master's suggestions are shown with Use and are never used until the person presses Use. Every save
// goes through `night_checks_set`, and the popover reads the project's checks again after it.
import { useState } from 'react';
import {
  nightApi,
  nightErrorText,
  type NightCheckInput,
  type NightChecksView,
} from '../../../lib/night';

/** The timeout a new check gets (`CHECK_TIMEOUT_MS`, ten minutes). */
export const CHECK_TIMEOUT_MIN = 10;

/** The words of a command, joined with spaces. Words with a space are quoted, as `parse_command` reads them. */
export function commandText(argv: string[]): string {
  return argv.map((part) => (/\s/.test(part) ? `"${part}"` : part)).join(' ');
}

const RELATIVE_NOTE =
  "This runs a script inside the project. If an agent edits it, Puppet Master can't tell.";

export function TestsPopover({
  project,
  view,
  onView,
  onNoTests,
  onClose,
  api = nightApi,
}: {
  project: string;
  /** The project's checks, or null while they load. */
  view: NightChecksView | null;
  onView: (view: NightChecksView) => void;
  onNoTests: () => void;
  onClose: () => void;
  api?: Pick<typeof nightApi, 'checksSet' | 'checksGet'>;
}) {
  const [adding, setAdding] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const declared: NightCheckInput[] = (view?.declared ?? []).map((check) => ({
    name: check.name,
    command: commandText(check.argv),
    timeoutMin: check.timeoutMin,
  }));

  /** Saves the list, then reads the project's checks again so the suggestions and warnings are current. */
  const save = async (list: NightCheckInput[]): Promise<boolean> => {
    setError(null);
    setBusy(true);
    try {
      await api.checksSet(project, list);
      onView(await api.checksGet(project));
      return true;
    } catch (err) {
      setError(nightErrorText(err));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const addTyped = async () => {
    const text = adding.trim();
    if (!text) return;
    if (await save([...declared, { name: text, command: text, timeoutMin: CHECK_TIMEOUT_MIN }])) setAdding('');
  };

  return (
    <div className="ck-night-pop" role="dialog" aria-label="Tests">
      <div className="ck-night-pop-head">
        <span className="ck-night-label">Tests</span>
        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" aria-label="Close tests" onClick={onClose}>
          ×
        </button>
      </div>

      {!view ? (
        <p className="ck-night-note">Looking for tests…</p>
      ) : (
        <>
          <ul className="ck-night-checks">
            {view.declared.map((check, index) => (
              <DeclaredRow
                key={`${check.name}-${index}`}
                text={commandText(check.argv)}
                disabled={busy}
                onSave={(text) => {
                  const next = declared.map((item, i) =>
                    i === index ? { name: text, command: text, timeoutMin: item.timeoutMin } : item,
                  );
                  void save(next);
                }}
                onRemove={() => void save(declared.filter((_, i) => i !== index))}
              />
            ))}
          </ul>

          {view.warnings.length > 0 && <p className="ck-night-warn">{RELATIVE_NOTE}</p>}

          {view.suggestions.length > 0 && (
            <ul className="ck-night-checks">
              {view.suggestions.map((found) => (
                <li key={`${found.name}-${found.source}`} className="ck-night-check ck-night-check--found">
                  <span className="ck-night-hint">
                    Found: {found.name} in {found.source}
                  </span>
                  <button
                    type="button"
                    className="ck-btn ck-btn--secondary ck-btn--sm"
                    disabled={busy}
                    onClick={() =>
                      void save([...declared, { name: found.name, command: commandText(found.argv), timeoutMin: CHECK_TIMEOUT_MIN }])
                    }
                  >
                    Use
                  </button>
                </li>
              ))}
            </ul>
          )}

          <div className="ck-night-check ck-night-check--add">
            <input
              className="ck-night-input"
              aria-label="Add a test command"
              placeholder="Add a command, like npm test"
              value={adding}
              disabled={busy}
              onChange={(e) => setAdding(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && void addTyped()}
            />
            {adding.trim() && (
              <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={busy} onClick={() => void addTyped()}>
                Add
              </button>
            )}
          </div>

          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-night-nochecks" disabled={busy} onClick={onNoTests}>
            No tests for this project
          </button>
        </>
      )}

      {error && (
        <p className="ck-night-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

/** One declared command: an editable line that saves on Enter or when it loses focus, and Remove. */
function DeclaredRow({
  text,
  disabled,
  onSave,
  onRemove,
}: {
  text: string;
  disabled: boolean;
  onSave: (text: string) => void;
  onRemove: () => void;
}) {
  const [draft, setDraft] = useState(text);
  const commit = () => {
    const next = draft.trim();
    if (next && next !== text) onSave(next);
    else setDraft(text);
  };
  return (
    <li className="ck-night-check">
      <input
        className="ck-night-input ck-night-input--cmd"
        aria-label={`Command ${text}`}
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === 'Enter' && commit()}
      />
      <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" disabled={disabled} onClick={onRemove}>
        Remove
      </button>
    </li>
  );
}
