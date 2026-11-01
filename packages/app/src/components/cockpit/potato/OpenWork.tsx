// Open work of one bot: the things it promised to do, with a round check to complete or reopen each, a remove button, and
// an add field behind the "+" button. The helper jobs still running for the bot show under the items (read-only: they end
// by themselves). Errors stay inline. After a change the list is read again; if that read fails, the change is kept and
// the card says so.
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { plainError } from '../../../lib/potato';
import { tauriOpenWorkTransport, type OpenWorkItem, type OpenWorkTransport, type OpenWorkView } from '../../../lib/potato-open-work';
import { PanelSection } from './ui/PanelSection';
import { Glyph } from './ui/Glyph';
import { StatusPill } from './ui/StatusPill';

export const OPEN_WORK_COPY = {
  title: 'Open work',
  add: 'Add an item',
  placeholder: 'What should it do?',
  addButton: 'Add',
  empty: 'Nothing open right now.',
  complete: 'Complete',
  reopen: 'Reopen',
  remove: 'Remove',
  working: 'Working',
  helper: (project: string) => `A helper is working in ${project}.`,
  showMore: (count: number) => `Show all ${count}`,
  loadFailed: 'Could not load the list.',
  addFailed: 'Could not add it.',
  changeFailed: 'Could not change that item.',
  removeFailed: 'Could not remove it.',
  savedNotRefreshed: 'Saved, but the list could not refresh.',
};

/** The list shows this many items; "Show all" opens the rest. */
export const SHOWN_AT_FIRST = 8;

export interface OpenWorkProps {
  botId: string;
  transport?: OpenWorkTransport;
}

export function OpenWork({ botId, transport = tauriOpenWorkTransport }: OpenWorkProps) {
  const [view, setView] = useState<OpenWorkView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [showAll, setShowAll] = useState(false);

  const load = useCallback(
    async (live: () => boolean = () => true) => {
      try {
        const next = await transport.list(botId);
        if (live()) setView(next);
        return true;
      } catch (err) {
        if (live()) setError(plainError(err, OPEN_WORK_COPY.loadFailed));
        return false;
      }
    },
    [botId, transport],
  );

  useEffect(() => {
    let live = true;
    setView(null);
    setError(null);
    setShowAll(false);
    void load(() => live);
    return () => {
      live = false;
    };
  }, [load]);

  /** Runs one change, then reads the list again. */
  const change = async (failed: string, run: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      await run();
    } catch (err) {
      setError(plainError(err, failed));
      setBusy(false);
      return false;
    }
    const refreshed = await load();
    if (!refreshed) setError(OPEN_WORK_COPY.savedNotRefreshed);
    setBusy(false);
    return true;
  };

  const add = async (event: FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    if (await change(OPEN_WORK_COPY.addFailed, () => transport.add(botId, text))) {
      setDraft('');
      setAdding(false);
    }
  };

  const toggle = (item: OpenWorkItem) => change(OPEN_WORK_COPY.changeFailed, () => transport.setDone(botId, item.id, !item.done));
  const drop = (item: OpenWorkItem) => change(OPEN_WORK_COPY.removeFailed, () => transport.remove(botId, item.id));

  const items = view?.items ?? [];
  const helpers = view?.helpers ?? [];
  const shown = showAll ? items : items.slice(0, SHOWN_AT_FIRST);
  const addButton = (
    <button
      type="button"
      className="ck-icon-btn"
      aria-label={OPEN_WORK_COPY.add}
      title={OPEN_WORK_COPY.add}
      aria-expanded={adding}
      onClick={() => setAdding((on) => !on)}
    >
      <Glyph name="plus" />
    </button>
  );

  return (
    <PanelSection id="ck-potato-openwork" title={OPEN_WORK_COPY.title} action={addButton}>
      {adding && (
        <form className="ck-potato-add" onSubmit={(event) => void add(event)}>
          <input
            className="ck-potato-add-input"
            aria-label={OPEN_WORK_COPY.add}
            placeholder={OPEN_WORK_COPY.placeholder}
            value={draft}
            maxLength={400}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
          />
          <button type="submit" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={busy || !draft.trim()}>
            {OPEN_WORK_COPY.addButton}
          </button>
        </form>
      )}
      {view === null && !error && <p className="ck-sub ck-potato-psec-hint">Loading…</p>}
      {view !== null && items.length === 0 && helpers.length === 0 && <p className="ck-sub ck-potato-psec-hint">{OPEN_WORK_COPY.empty}</p>}
      {shown.length > 0 && (
        <ul className="ck-potato-todo">
          {shown.map((item) => (
            <li key={item.id} className={`ck-potato-todo-row${item.done ? ' is-done' : ''}`}>
              <input
                type="checkbox"
                className="ck-potato-todo-check"
                checked={item.done}
                disabled={busy}
                aria-label={`${item.done ? OPEN_WORK_COPY.reopen : OPEN_WORK_COPY.complete}: ${item.text}`}
                onChange={() => void toggle(item)}
              />
              <span className="ck-potato-todo-text">{item.text}</span>
              <button
                type="button"
                className="ck-icon-btn"
                disabled={busy}
                aria-label={`${OPEN_WORK_COPY.remove}: ${item.text}`}
                title={OPEN_WORK_COPY.remove}
                onClick={() => void drop(item)}
              >
                <Glyph name="close" size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {!showAll && items.length > SHOWN_AT_FIRST && (
        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-potato-show-more" onClick={() => setShowAll(true)}>
          {OPEN_WORK_COPY.showMore(items.length)}
        </button>
      )}
      {helpers.map((helper) => (
        <p key={helper.requestId} className="ck-potato-helper-row">
          <StatusPill tone="strong">{OPEN_WORK_COPY.working}</StatusPill>
          <span className="ck-sub">{OPEN_WORK_COPY.helper(helper.project)}</span>
        </p>
      ))}
      {error && (
        <p className="ck-sub ck-potato-psec-error" role="alert">
          {error}
        </p>
      )}
    </PanelSection>
  );
}
