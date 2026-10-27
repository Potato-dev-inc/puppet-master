// The Inbox card for a companion that keeps stopping (Rust key `companion:<id>`, kind setup). One action,
// "Try again", asks the keeper to start it again; "Open terminal" shows the pane so the person can see why.
import { useState } from 'react';
import type { CompanionView } from '../../../lib/companion';
import { CardButton, InboxCard } from '../InboxCard';

export function CompanionStoppedCard({
  companion,
  onTryAgain,
  onOpenPane,
}: {
  companion: CompanionView;
  /** Restart it (the desktop calls `companion_restart`). A rejection leaves the card up with a short line. */
  onTryAgain: (id: string) => Promise<unknown> | void;
  onOpenPane?: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const tryAgain = () => {
    setBusy(true);
    setFailed(false);
    Promise.resolve(onTryAgain(companion.id))
      .catch(() => setFailed(true))
      .finally(() => setBusy(false));
  };
  return (
    <InboxCard
      kind="companion"
      glyph="error"
      title={`${companion.name} stopped 3 times`}
      meta={companion.agentLabel}
      reason={failed ? "Could not start it. Open the terminal to see why." : 'Open the terminal to see why.'}
      data={{ 'data-companion-id': companion.id }}
      actions={
        <CardButton tone="primary" role="answer" disabled={busy} onClick={tryAgain}>
          Try again
        </CardButton>
      }
      more={
        onOpenPane ? (
          <CardButton tone="plain" role="open" onClick={onOpenPane}>
            Open terminal
          </CardButton>
        ) : null
      }
    />
  );
}
