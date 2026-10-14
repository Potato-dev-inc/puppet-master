import { useEffect, useRef, useState } from 'react';
import { cardMeta, cardTitle, draftTitle, MAX_NOTE, tellLabel, teamApi, type TeamApi, type TeamDraft } from '../../../lib/team';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/** How long the toast stays when nobody touches it. The draft stays in Inbox › Team › Unsent. */
export const TOAST_MS = 8_000;

/**
 * "Tell the team?" for one of my pushes: Tell / Not now, an optional note, and a preview of the
 * exact card teammates will see. Nothing is sent until "Tell" is clicked.
 */
export function TellTeamPrompt({
  draft,
  members,
  api,
  variant,
  onDone,
  onTouched,
}: {
  draft: TeamDraft;
  members: string[];
  api: TeamApi;
  variant: 'toast' | 'row';
  onDone?: () => void;
  /** The person started interacting (the toast must not vanish under them). */
  onTouched?: () => void;
}) {
  const [preview, setPreview] = useState(false);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shown = { ...draft.event, note: note.trim() || undefined };

  const run = async (what: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await what();
      onDone?.();
    } catch (err) {
      setError(errText(err));
      setBusy(false);
    }
  };

  return (
    <div className={`ck-tell ck-tell--${variant}`} data-team-draft={draft.id} onFocusCapture={onTouched} onMouseDown={onTouched} onClickCapture={onTouched}>
      <div className="ck-tell-head">
        <span className="ck-row-title">{draftTitle(draft)}</span>
        <span className="ck-team-meta">Tell the team?</span>
      </div>
      {preview && (
        <div className="ck-tell-preview" aria-label="What your teammates will see">
          <p className="ck-row-title">{cardTitle(shown)}</p>
          <p className="ck-team-meta">{cardMeta(shown)}</p>
          {shown.note && <p className="ck-team-note">“{shown.note}”</p>}
          <input
            className="ck-team-input"
            type="text"
            maxLength={MAX_NOTE}
            placeholder="Add a note (optional)"
            aria-label="Note for your teammates"
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
          <p className="ck-team-meta">No code is shared. They see the branch, the commit and the file count.</p>
        </div>
      )}
      {error && (
        <p className="ck-team-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-tell-actions">
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(() => api.send(draft.id, note.trim() || undefined))}>
          {tellLabel(members)}
        </button>
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void run(() => api.dismissDraft(draft.id))}>
          Not now
        </button>
        <button type="button" className="ck-btn ck-btn--plain" aria-expanded={preview} onClick={() => setPreview((v) => !v)}>
          {preview ? 'Hide preview' : 'Preview'}
        </button>
      </div>
    </div>
  );
}

/**
 * The quiet toast at the bottom right when one of my pushes is seen. It goes away after
 * [`TOAST_MS`] unless touched; the same draft stays in Inbox › Team until I decide.
 */
export function TellTeamToast({ api = teamApi, members: initial }: { api?: TeamApi; members?: string[] }) {
  const [draft, setDraft] = useState<TeamDraft | null>(null);
  const [members, setMembers] = useState<string[]>(initial ?? []);
  const touched = useRef(false);

  useEffect(() => {
    let off: (() => void) | undefined;
    let disposed = false;
    // The cockpit may open just after the push (panes live in their own windows): show a draft
    // that is still fresh.
    api
      .state()
      .then((s) => {
        if (disposed) return;
        setMembers(s.members);
        const fresh = s.drafts.find((d) => Date.now() - d.createdAtMs < TOAST_MS);
        if (fresh) setDraft((current) => current ?? fresh);
      })
      .catch(() => {});
    api
      .onDraft((d) => {
        touched.current = false;
        setDraft(d);
        api
          .state()
          .then((s) => setMembers(s.members))
          .catch(() => {});
      })
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, [api]);

  useEffect(() => {
    if (!draft) return;
    const timer = window.setTimeout(() => {
      if (!touched.current) setDraft(null);
    }, TOAST_MS);
    return () => window.clearTimeout(timer);
  }, [draft]);

  if (!draft) return null;
  return (
    <div className="ck-tell-toast" role="status" aria-live="polite">
      <TellTeamPrompt
        draft={draft}
        members={members}
        api={api}
        variant="toast"
        onDone={() => setDraft(null)}
        onTouched={() => {
          touched.current = true;
        }}
      />
    </div>
  );
}
