// The Point box: the small window that opens beside a picked element. It shows state and reports what the person
// does. PointBoxApp owns the state and the calls (see lib/point-api.ts). Every page-derived string is plain text.
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import type { PickView, PointState, Target } from '../../lib/point-api';
import { MAX_REQUEST, boxKey, groupTargets, sourceLine } from './point-model';

/** Most elements one message carries (the Rust session keeps the same cap). */
export const MAX_PICKS = 3;

export interface PointBoxProps {
  state: PointState;
  request: string;
  onRequest: (text: string) => void;
  /** The chosen pane id, or null when none is chosen yet. */
  target: string | null;
  onTarget: (paneId: string) => void;
  onSend: () => void;
  onEditInChat: () => void;
  onCopy: () => void;
  onPointAnother: () => void;
  onRemove: (pickId: string) => void;
  onDiscard: () => void;
  onHide: () => void;
  onOpenBlocked: () => void;
  /** One plain sentence about the last action that did not happen. The text stays in the box. */
  error?: string | null;
  /** One plain line about the last action that did happen, e.g. "Copied.". */
  info?: string | null;
  /** A send is in flight. */
  busy?: boolean;
}

function targetName(t: Target): string {
  return t.title ? `${t.agentLabel} · ${t.title}` : t.agentLabel;
}

function PickRow({ pick, onRemove }: { pick: PickView; onRemove: (id: string) => void }): JSX.Element {
  const line = sourceLine(pick);
  return (
    <li className="point-box-pick">
      {pick.thumb ? (
        <img className="point-box-thumb" src={pick.thumb} alt={`Screenshot of ${pick.label}`} />
      ) : (
        <div className="point-box-thumb point-box-thumb--none" title={pick.noShotReason || 'No screenshot'}>
          No screenshot
        </div>
      )}
      <div className="point-box-pick-body">
        <div className="point-box-pick-head">
          <code className="point-box-tag">{pick.label}</code>
          {pick.text && <span className="point-box-text">{`"${pick.text}"`}</span>}
        </div>
        {line && (
          <div className="point-box-chip" title={line}>
            {line}
          </div>
        )}
        <div className="point-box-page" title={pick.page}>
          {pick.page}
        </div>
      </div>
      <button type="button" className="point-box-x" aria-label={`Remove ${pick.label}`} onClick={() => onRemove(pick.id)}>
        ×
      </button>
    </li>
  );
}

export function PointBox(props: PointBoxProps): JSX.Element {
  const {
    state,
    request,
    onRequest,
    target,
    onTarget,
    onSend,
    onEditInChat,
    onCopy,
    onPointAnother,
    onRemove,
    onDiscard,
    onHide,
    onOpenBlocked,
    error,
    info,
    busy = false,
  } = props;
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const picks = state.picks;
  const chosen = state.targets.find((t) => t.paneId === target);
  const ready = picks.length > 0;
  const canSend = ready && !!chosen?.canSend && !busy;
  const canCopy = ready && !!chosen && !busy;

  // Focus the text when the box opens and again when another element is added.
  useEffect(() => {
    areaRef.current?.focus();
  }, [picks.length]);

  // Grow with the text. The CSS max-height stops it at six lines.
  useLayoutEffect(() => {
    const el = areaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [request]);

  const onAreaKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (boxKey({ key: e.key, shiftKey: e.shiftKey, isComposing: e.nativeEvent.isComposing }) === 'send') {
      e.preventDefault();
      if (canSend) onSend();
    }
  };

  // Escape hides the box from anywhere inside it.
  const onRootKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (boxKey({ key: e.key, shiftKey: e.shiftKey, isComposing: e.nativeEvent.isComposing }) === 'hide') onHide();
  };

  return (
    <div className="point-box" role="dialog" aria-label="Point and Fix" onKeyDown={onRootKeyDown}>
      <div className="point-box-head">
        <span className="point-box-title">Point and Fix</span>
        <span className="point-box-count">{`${picks.length} of ${MAX_PICKS}`}</span>
        <span className="point-box-spacer" />
        <button type="button" className="point-box-btn point-box-btn--plain" onClick={onDiscard}>
          Discard
        </button>
      </div>

      {state.notice && (
        <div className="point-box-notice" role="status">
          <span>{state.notice}</span>
          <button type="button" className="point-box-btn point-box-btn--plain" onClick={onOpenBlocked}>
            Open in browser
          </button>
        </div>
      )}

      {ready ? (
        <ul className="point-box-picks">
          {picks.map((p) => (
            <PickRow key={p.id} pick={p} onRemove={onRemove} />
          ))}
        </ul>
      ) : (
        <p className="point-box-empty">Click Point in the preview, then click the thing that is wrong.</p>
      )}

      <label className="point-box-to">
        <span>To</span>
        <select className="point-box-select" value={target ?? ''} onChange={(e) => onTarget(e.target.value)} disabled={state.targets.length === 0}>
          {!chosen && <option value="">Choose a pane</option>}
          {groupTargets(state.targets).map((g) => (
            <optgroup key={g.label} label={g.label}>
              {g.items.map((t) => (
                <option key={t.paneId} value={t.paneId} disabled={!t.canSend}>
                  {targetName(t)}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>
      {state.targets.length === 0 && <div className="point-box-reason">No agent pane is open to take the message.</div>}
      {chosen && !chosen.canSend && chosen.reason && <div className="point-box-reason">{chosen.reason}</div>}

      <label className="point-box-q" htmlFor="point-box-request">
        What should change?
      </label>
      <textarea
        id="point-box-request"
        ref={areaRef}
        className="point-box-textarea"
        rows={2}
        maxLength={MAX_REQUEST}
        value={request}
        onChange={(e) => onRequest(e.target.value)}
        onKeyDown={onAreaKeyDown}
      />
      <div className="point-box-hint">Enter sends. Shift+Enter starts a new line. Esc hides.</div>

      {(error || info) && (
        <div className={error ? 'point-box-error' : 'point-box-info'} role={error ? 'alert' : 'status'}>
          {error || info}
        </div>
      )}

      <div className="point-box-foot">
        <button type="button" className="point-box-btn point-box-btn--plain" onClick={onPointAnother} disabled={picks.length >= MAX_PICKS || busy}>
          Point another
        </button>
        <span className="point-box-spacer" />
        <button type="button" className="point-box-btn" onClick={onCopy} disabled={!canCopy}>
          Copy
        </button>
        <button type="button" className="point-box-btn" onClick={onEditInChat} disabled={!canSend}>
          Edit in Chat
        </button>
        <button type="button" className="point-box-btn point-box-btn--primary" onClick={onSend} disabled={!canSend}>
          {busy ? 'Sending…' : 'Send'}
        </button>
      </div>
    </div>
  );
}
