// "What {name} knows" (SPEC section 8): the three memory files, a character counter against each cap, Edit and Save,
// the "notes are full" line, and the outside-change rows. The counter counts characters as the backend does.
import { useState } from 'react';
import type { CompanionTransport, MemoryFile, MemoryView } from '../../../lib/companion';
import { SegTabs } from '../SegTabs';
import { MEMORY_TABS, countChars, fileNameOf, formatCount } from './model';

export function MemoryCard({
  companionId,
  name,
  memory,
  transport,
  onChanged,
}: {
  companionId: string;
  name: string;
  memory: MemoryView;
  transport: CompanionTransport;
  /** The view a command returned: the parent keeps it. */
  onChanged: (next: MemoryView) => void;
}) {
  const [tab, setTab] = useState<MemoryFile>('soul');
  // `null` while reading; the text being edited otherwise.
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const saved = memory[tab];
  const cap = memory.caps[tab];
  const shown = draft ?? saved;
  const count = countChars(shown);
  const over = count > cap;
  const full = countChars(memory.memory) >= memory.caps.memory;
  const dirty = draft !== null && draft !== saved;

  const switchTab = (next: MemoryFile) => {
    setTab(next);
    setDraft(null);
    setError(null);
  };

  const save = async () => {
    if (draft === null || over) return;
    setBusy(true);
    setError(null);
    try {
      onChanged(await transport.saveMemory(companionId, tab, draft));
      setDraft(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save.');
    } finally {
      setBusy(false);
    }
  };

  const decideOutside = async (file: MemoryFile, decision: 'keep' | 'undo') => {
    setError(null);
    try {
      onChanged(await transport.decideOutsideChange(companionId, file, decision));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not change it.');
    }
  };

  const tidy = async () => {
    setError(null);
    try {
      await transport.tidyUp(companionId);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not ask.');
    }
  };

  return (
    <section className="ck-companion-card" aria-labelledby="ck-companion-memory-title">
      <h2 className="ck-companion-card-title" id="ck-companion-memory-title">
        What {name} knows
      </h2>

      {memory.outsideChanges.map((change) => (
        <div className="ck-companion-notice" key={change.file} role="status">
          <span>{fileNameOf(change.file)} changed outside Puppet Master.</span>
          <span className="ck-companion-notice-actions">
            <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void decideOutside(change.file, 'keep')}>
              Keep the change
            </button>
            <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={() => void decideOutside(change.file, 'undo')}>
              Undo
            </button>
          </span>
        </div>
      ))}

      <SegTabs label="Memory file" value={tab} onChange={switchTab} options={MEMORY_TABS.map((t) => ({ id: t.file, label: t.label }))} small />

      <textarea
        className="ck-memory-editor ck-companion-editor"
        aria-label={MEMORY_TABS.find((t) => t.file === tab)?.label}
        value={shown}
        readOnly={draft === null}
        onChange={(e) => setDraft(e.target.value)}
        spellCheck={false}
      />

      <div className="ck-companion-meta">
        <span className={over ? 'ck-memory-err' : undefined} aria-live="polite">
          {formatCount(count)} / {formatCount(cap)}
        </span>
        <span className="ck-companion-meta-actions">
          {draft === null ? (
            <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => setDraft(saved)}>
              Edit
            </button>
          ) : (
            <>
              <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={() => setDraft(null)} disabled={busy}>
                Cancel
              </button>
              <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" onClick={() => void save()} disabled={!dirty || over || busy}>
                Save
              </button>
            </>
          )}
        </span>
      </div>

      {full && (
        <div className="ck-companion-notice">
          <span>
            Its notes are full. Ask {name} to tidy up.
          </span>
          <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void tidy()}>
            Ask to tidy up
          </button>
        </div>
      )}
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
