// "Branch from here": the sheet a chat turn's Branch button or a History row opens. It shows the save point, lets the
// person pick Before or After this turn (chat only), pick an agent and say what it should do. Nothing is written
// until Start: then Puppet Master makes a separate copy from the save point and starts the agent in it
// (src/lib/branch-from.ts). Only the agents the server marks ok can be picked; the others say why not.
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import {
  branchPreview,
  branchStart,
  type BranchFrom,
  type BranchOffer,
  type BranchPointView,
  type BranchPreview,
  type BranchView,
  type BranchWhen,
} from '../../lib/branch-from';
import '../../styles/cockpit-branch.css';
import { briefSize } from './BriefSheet';
import { Sheet } from './Sheet';

/** The most characters the task box takes. The server refuses more. */
export const MESSAGE_MAX_CHARS = 4_000;
/** The counter shows once the box holds more than this many characters. */
export const COUNTER_FROM_CHARS = 3_500;
/** The brief preview asks again this long after the last key press. */
const BRIEF_DEBOUNCE_MS = 400;

const GENERIC_ERROR = 'Something went wrong. Try again.';
const NOT_HERE = "Branching isn't available for this pane.";
const TOO_LONG = 'Make your message shorter (4,000 characters at most).';

/** Backend errors read `CODE: message`. The person sees only the message, in plain words. */
function friendlyError(err: unknown): string {
  const raw = err instanceof Error ? err.message : typeof err === 'string' ? err : '';
  const text = raw.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '').trim();
  if (!text || /^[A-Z][A-Z0-9_]+$/.test(text)) return GENERIC_ERROR;
  return text;
}

/** The clock time of a moment, as "14:02". */
function clockText(ms: number): string {
  const at = new Date(ms);
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
}

/** "Save point: After turn 5 · 14:02 · 3 files changed in that turn". The file part is left out when it is 0. */
function savePointLine(point: BranchPointView): string {
  const files = point.files > 0 ? ` · ${point.files} ${point.files === 1 ? 'file' : 'files'} changed in that turn` : '';
  return `Save point: ${point.label} · ${clockText(point.atMs)}${files}`;
}

/** `agent` or `agent@account`: the text the server takes as the target. */
function offerKey(offer: BranchOffer): string {
  return offer.target.account ? `${offer.target.agent_id}@${offer.target.account}` : offer.target.agent_id;
}

function firstOk(offers: BranchOffer[]): string | null {
  const first = offers.find((o) => o.ok);
  return first ? offerKey(first) : null;
}

/** The "From" of a chat turn, with the switch's side. A History row has no side. */
function withWhen(from: BranchFrom, when: BranchWhen): BranchFrom {
  return from.kind === 'message' ? { kind: 'message', user_item_id: from.user_item_id, when } : from;
}

export function BranchSheet({
  paneId,
  from,
  onClose,
  onStarted,
}: {
  /** The pane whose chat or History the branch starts from. */
  paneId: string;
  /** A chat turn (`message`) or a History row (`save_point`). */
  from: BranchFrom;
  onClose: () => void;
  /** Called with the new branch once Start has made it (its pane is starting). */
  onStarted?: (view: BranchView) => void;
}) {
  const uid = useId();
  const titleId = `${uid}-title`;
  const fieldId = `${uid}-field`;
  const [when, setWhen] = useState<BranchWhen>(from.kind === 'message' ? from.when : 'after');
  const effective = withWhen(from, when);
  const effectiveRef = useRef(effective);
  effectiveRef.current = effective;

  const [preview, setPreview] = useState<BranchPreview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [briefOpen, setBriefOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The message the last "Before this turn" filled in. A text the person typed is never overwritten.
  const prefilled = useRef<string | null>(null);
  // Only the newest preview counts: an older answer that arrives late is dropped.
  const seq = useRef(0);

  const load = useCallback(
    (next: BranchFrom, text: string | null, fillPrefill: boolean) => {
      const mine = ++seq.current;
      return branchPreview(paneId, next, null, text)
        .then((p) => {
          if (mine !== seq.current) return;
          setPreview(p);
          setLoadError(null);
          setPicked((prev) => (prev !== null && p.offers.some((o) => o.ok && offerKey(o) === prev) ? prev : firstOk(p.offers)));
          if (fillPrefill && p.prefill !== null) {
            const fill = p.prefill;
            const previous = prefilled.current;
            setMessage((cur) => (cur.trim() === '' || cur === previous ? fill : cur));
            prefilled.current = fill;
          }
        })
        .catch((err: unknown) => {
          if (mine === seq.current) setLoadError(friendlyError(err));
        });
    },
    [paneId],
  );

  useEffect(() => {
    void load(effectiveRef.current, null, false);
  }, [load]);

  // The brief preview follows the text while its disclosure is open, after a short pause. An empty box asks nothing.
  useEffect(() => {
    if (!briefOpen || message.trim() === '') return;
    const timer = setTimeout(() => void load(effectiveRef.current, message, false), BRIEF_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [briefOpen, message, when, load]);

  const choose = (next: BranchWhen) => {
    if (busy || next === when) return;
    setWhen(next);
    void load(withWhen(from, next), briefOpen ? message : null, next === 'before');
  };

  const blocked = preview !== null ? !preview.canBranch : loadError !== null;
  const chosen = preview?.offers.find((o) => o.ok && offerKey(o) === picked) ?? null;
  const tooLong = message.length > MESSAGE_MAX_CHARS;
  const canStart = !blocked && chosen !== null && message.trim() !== '' && !tooLong && !busy;

  const go = async () => {
    if (!chosen || !canStart) return;
    setBusy(true);
    setError(null);
    try {
      const view = await branchStart(paneId, effective, offerKey(chosen), message.trim());
      onStarted?.(view);
      onClose();
    } catch (err) {
      setError(friendlyError(err));
      setBusy(false);
    }
  };

  if (blocked) {
    const reason = preview?.whyNot ?? (preview === null ? loadError : null) ?? NOT_HERE;
    return (
      <Sheet onClose={onClose} role="dialog" labelledBy={titleId} className="ck-branch-sheet">
        <div id={titleId} className="ck-title">
          Branch from here
        </div>
        <p className="ck-sub">{reason}</p>
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" onClick={onClose}>
            Close
          </button>
        </div>
      </Sheet>
    );
  }

  // The brief shows only while the box holds text.
  const shown = message.trim() === '' ? null : (preview?.brief ?? null);
  const overText = message.length > COUNTER_FROM_CHARS;

  return (
    <Sheet onClose={busy ? undefined : onClose} role="dialog" labelledBy={titleId} className="ck-branch-sheet">
      <div id={titleId} className="ck-title">
        Branch from here
      </div>
      {preview?.point && <p className="ck-sub ck-branch-point">{savePointLine(preview.point)}</p>}
      {preview === null && loadError === null && <p className="ck-sub">Looking at the save point…</p>}
      {from.kind === 'message' && (
        <div className="ck-branch-seg" role="group" aria-label="Save point">
          <button type="button" className="ck-branch-seg-btn" aria-pressed={when === 'after'} disabled={busy} onClick={() => choose('after')}>
            After this turn
          </button>
          <button type="button" className="ck-branch-seg-btn" aria-pressed={when === 'before'} disabled={busy} onClick={() => choose('before')}>
            Before this turn
          </button>
        </div>
      )}
      <div className="ck-branch-label" id={`${uid}-agent`}>
        Agent
      </div>
      {preview !== null && preview.offers.length === 0 && <p className="ck-sub">No agent can run in this folder yet.</p>}
      {preview !== null && preview.offers.length > 0 && (
        <div className="ck-branch-list" role="radiogroup" aria-labelledby={`${uid}-agent`}>
          {preview.offers.map((o) => {
            const key = offerKey(o);
            return (
              <label key={key} className={`ck-branch-row${o.ok ? '' : ' is-off'}`}>
                <input
                  type="radio"
                  name={`${uid}-agent-pick`}
                  value={key}
                  checked={picked === key}
                  disabled={!o.ok || busy}
                  onChange={() => setPicked(key)}
                />
                <span className="ck-branch-name">{o.label}</span>
                <span className="ck-branch-state">{o.ok ? 'Ready' : (o.whyNot ?? 'Not available right now')}</span>
              </label>
            );
          })}
        </div>
      )}
      <div className="ck-branch-field">
        <label htmlFor={fieldId} className="ck-branch-label">
          What should it do?
        </label>
        <textarea
          id={fieldId}
          className="ck-branch-input"
          rows={4}
          maxLength={MESSAGE_MAX_CHARS}
          placeholder="Tell the agent what to try"
          value={message}
          disabled={busy}
          onChange={(e) => setMessage(e.target.value)}
        />
        {overText && (
          <span className={`ck-branch-count${tooLong ? ' is-over' : ''}`}>
            {`${message.length.toLocaleString('en-US')} / ${MESSAGE_MAX_CHARS.toLocaleString('en-US')}`}
          </span>
        )}
        {tooLong && <p className="ck-sub">{TOO_LONG}</p>}
      </div>
      <button type="button" className="ck-branch-disclosure" aria-expanded={briefOpen} onClick={() => setBriefOpen((open) => !open)}>
        Show what the agent gets
      </button>
      {briefOpen && (
        <div className="ck-branch-brief-box">
          {shown ? (
            <>
              <pre className="ck-branch-brief" tabIndex={0} aria-label="What the agent gets">
                {shown.text}
              </pre>
              <div className="ck-sub">{`${briefSize(shown.text)} · trimmed: ${shown.trimmed.length > 0 ? shown.trimmed.join(', ') : 'none'}`}</div>
            </>
          ) : (
            <p className="ck-sub">{message.trim() === '' ? 'Type what it should do to see the brief.' : 'Loading the brief…'}</p>
          )}
        </div>
      )}
      <p className="ck-sub ck-branch-copy">
        Puppet Master makes a separate copy of your files from this save point. Your folder and this chat stay as they are. Not in the copy:
        files Git ignores (like node_modules) and files over 20 MB.
      </p>
      {loadError && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {loadError}
        </p>
      )}
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" disabled={!canStart} onClick={() => void go()}>
          {busy ? 'Starting…' : chosen ? `Start ${chosen.label}` : 'Start'}
        </button>
      </div>
    </Sheet>
  );
}
