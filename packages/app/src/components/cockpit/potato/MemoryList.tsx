// Memory of one bot: the notes it keeps (its journal, diary and notes), newest first. A row opens its text; Delete asks
// first ("Confirm delete"). Read-only apart from Delete. The paths come from the list; the Rust side checks them again.
import { useEffect, useState } from 'react';
import { formatWhen, plainError } from '../../../lib/potato';
import { noteTitle, tauriMemoryTransport, type MemoryNote, type MemoryTransport } from '../../../lib/potato-memory';
import { PanelSection } from './ui/PanelSection';

export const MEMORY_COPY = {
  title: 'Memory',
  empty: 'Nothing remembered yet.',
  loading: 'Loading…',
  loadFailed: 'Could not load the notes.',
  readFailed: 'Could not open this note.',
  deleteFailed: 'Could not delete this note.',
  delete: 'Delete',
  confirm: 'Confirm delete',
  cancel: 'Cancel',
  confirmQuestion: 'Delete this note? The bot will forget it.',
  showMore: (count: number) => `Show all ${count}`,
  kind: { journal: 'Journal', diary: 'Diary', nodes: 'Note' } as Record<string, string>,
};

/** The notes shown at first; "Show all" opens the rest. */
export const NOTES_SHOWN_AT_FIRST = 10;

/** `2 KB`, `120 B`: a short size for the row. */
export function noteSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${Math.round(bytes / 1024)} KB`;
}

export interface MemoryListProps {
  botId: string;
  transport?: MemoryTransport;
}

export function MemoryList({ botId, transport = tauriMemoryTransport }: MemoryListProps) {
  const [notes, setNotes] = useState<MemoryNote[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [text, setText] = useState<{ path: string; body: string } | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    let live = true;
    setNotes(null);
    setError(null);
    setOpenPath(null);
    setText(null);
    setAsking(null);
    setShowAll(false);
    transport
      .list(botId)
      .then((list) => live && setNotes(list))
      .catch((err) => live && setError(plainError(err, MEMORY_COPY.loadFailed)));
    return () => {
      live = false;
    };
  }, [botId, transport]);

  const open = async (path: string) => {
    if (openPath === path) {
      setOpenPath(null);
      return;
    }
    setOpenPath(path);
    setAsking(null);
    setError(null);
    setText(null);
    try {
      setText({ path, body: await transport.read(botId, path) });
    } catch (err) {
      setError(plainError(err, MEMORY_COPY.readFailed));
    }
  };

  const remove = async (path: string) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await transport.remove(botId, path);
      setNotes((list) => (list ?? []).filter((n) => n.path !== path));
      setOpenPath(null);
      setText(null);
      setAsking(null);
    } catch (err) {
      setError(plainError(err, MEMORY_COPY.deleteFailed));
    } finally {
      setBusy(false);
    }
  };

  const all = notes ?? [];
  const shown = showAll ? all : all.slice(0, NOTES_SHOWN_AT_FIRST);

  return (
    <PanelSection id="ck-potato-memory" title={MEMORY_COPY.title}>
      {notes === null && !error && <p className="ck-sub ck-potato-psec-hint">{MEMORY_COPY.loading}</p>}
      {notes !== null && all.length === 0 && <p className="ck-sub ck-potato-psec-hint">{MEMORY_COPY.empty}</p>}
      {shown.length > 0 && (
        <ul className="ck-potato-notes">
          {shown.map((note) => {
            const expanded = openPath === note.path;
            const folder = note.path.split('/')[0];
            return (
              <li key={note.path} className="ck-potato-note-row">
                <button
                  type="button"
                  className="ck-potato-note-open"
                  aria-expanded={expanded}
                  onClick={() => void open(note.path)}
                >
                  <span className="ck-potato-note-title">{noteTitle(note.path)}</span>
                  <span className="ck-sub">
                    {MEMORY_COPY.kind[folder] ?? 'Note'} · {formatWhen(note.modifiedMs, Date.now())} · {noteSize(note.bytes)}
                  </span>
                </button>
                {expanded && (
                  <div className="ck-potato-note-body">
                    {text && text.path === note.path ? <pre className="ck-potato-note-text">{text.body}</pre> : <p className="ck-sub">{MEMORY_COPY.loading}</p>}
                    {asking === note.path ? (
                      <div className="ck-potato-note-ask">
                        <p className="ck-sub">{MEMORY_COPY.confirmQuestion}</p>
                        <div className="ck-potato-note-actions">
                          <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={busy} onClick={() => void remove(note.path)}>
                            {MEMORY_COPY.confirm}
                          </button>
                          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => setAsking(null)}>
                            {MEMORY_COPY.cancel}
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="ck-potato-note-actions">
                        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" disabled={busy} onClick={() => setAsking(note.path)}>
                          {MEMORY_COPY.delete}
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {!showAll && all.length > NOTES_SHOWN_AT_FIRST && (
        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-potato-show-more" onClick={() => setShowAll(true)}>
          {MEMORY_COPY.showMore(all.length)}
        </button>
      )}
      {error && (
        <p className="ck-sub ck-potato-psec-error" role="alert">
          {error}
        </p>
      )}
    </PanelSection>
  );
}
