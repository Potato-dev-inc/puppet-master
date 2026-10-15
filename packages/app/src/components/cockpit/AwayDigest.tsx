// "While you were away": a full-pane digest layer over the cockpit main area.
// Built from the Digest the desktop shell sends on return; dismissed with "Got it" or Escape.
import { useEffect } from 'react';
import type { Digest } from '../../lib/cockpit-api';
import { DotSpinner } from './DotSpinner';
import { formatAge } from './inbox-age';
import { formatElapsed, spinnerStatus } from './model';

export interface AwayDigestProps {
  digest: Digest;
  now: number;
  onDismiss: () => void;
  onOpenPane: (paneId: string) => void;
  onOpenInbox: () => void;
}

export function AwayDigest({ digest, now, onDismiss, onOpenPane, onOpenInbox }: AwayDigestProps) {
  const count = digest.agentsWorked;
  const hasNeeds = digest.needsYou.length > 0;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDismiss();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onDismiss]);

  return (
    <section className="ck-away" role="region" aria-label="While you were away">
      <div className="ck-away-column">
        <header className="ck-inbox-hero">
          {/* A plain title. The count lives in the sentence: the big number belongs to the Inbox, where it means "things I must do". */}
          <h1 className="ck-title">While you were away</h1>
          <p className="ck-sub">
            {count === 1 ? '1 agent worked' : `${count} agents worked`}. You were away for {formatElapsed(now - digest.sinceMs)}. Here is what happened.
          </p>
        </header>

        <ul className="ck-list ck-away-lines">
          {digest.lines.map((line) => (
            <li key={`${line.paneId}-${line.endedAtMs}`} className="ck-away-line">
              <span className="ck-row-glyph">
                <DotSpinner status={spinnerStatus(line.end)} />
              </span>
              <span className="ck-away-sentence">{line.sentence}</span>
              {line.turns > 1 && <span className="ck-row-sub">{line.turns} turns</span>}
              <span className="ck-row-time" title={`${formatElapsed(now - line.endedAtMs)} ago`}>{formatAge(now - line.endedAtMs)}</span>
            </li>
          ))}
        </ul>

        {hasNeeds && (
          <>
            <div className="ck-caption">Needs you</div>
            <ul className="ck-list ck-away-needs">
              {digest.needsYou.map((n) => {
                const status = spinnerStatus(n.status);
                const fallback = n.status === 'error' ? 'Hit an error' : 'Waiting for you';
                return (
                  <li key={n.id ?? n.paneId}>
                    <button type="button" className="ck-away-need" onClick={() => (n.paneId ? onOpenPane(n.paneId) : onOpenInbox())}>
                      <span className="ck-row-glyph">
                        <DotSpinner status={status} />
                      </span>
                      <span className="ck-row-title">{n.agent}</span>
                      <span className="ck-row-sub">{n.prompt || fallback}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </>
        )}

        <div className="ck-away-actions">
          <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={onDismiss}>
            Got it
          </button>
          {hasNeeds && (
            <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenInbox}>
              Open Inbox
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
