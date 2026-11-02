// The buttons that belong to the grey line a turn ends with. The words come from the backend (`ending:<reason>`
// notice); the buttons are chosen here by the reason: Retry (nothing came back, or it went wrong), Sign in (probably
// signed out) and Keep going with <other agent> (usage limit). Only the newest turn gets buttons: an old turn is
// history. A button the pane cannot honour (no message box, no other agent set up) is simply not drawn.
import { useState } from 'react';
import type { TurnEnding } from '../../lib/sessionChatModel';

/** What the buttons do. Each is optional: without it the button is not drawn. */
export interface EndingActions {
  /** Send the turn's message again. Resolves to null, or one plain sentence saying why not. */
  onRetry?: () => Promise<string | null>;
  /** Open the "Check my setup" row where the agent's sign-in lives. */
  onSignIn?: () => void;
  /** Hand the task to another agent that is set up. `label` is that agent's name. */
  keepGoing?: { label: string; run: () => Promise<string | null> } | null;
}

/** Which buttons a reason gets, in order. */
export function endingButtons(reason: string, actions: EndingActions): ('retry' | 'signIn' | 'keepGoing')[] {
  const out: ('retry' | 'signIn' | 'keepGoing')[] = [];
  if (reason === 'usage_limit' && actions.keepGoing) out.push('keepGoing');
  if (reason === 'signed_out' && actions.onSignIn) out.push('signIn');
  if ((reason === 'empty' || reason === 'failed' || reason === 'no_answer') && actions.onRetry) out.push('retry');
  return out;
}

export function TurnEndingButtons({ ending, actions }: { ending: TurnEnding; actions: EndingActions }) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const buttons = endingButtons(ending.reason, actions);
  if (buttons.length === 0 && !note) return null;

  const run = (job: () => Promise<string | null>) => {
    if (busy) return;
    setBusy(true);
    setNote(null);
    job()
      .then(setNote, () => setNote('That did not work. Try again.'))
      .finally(() => setBusy(false));
  };

  return (
    <>
      {buttons.map((button) => (
        <span key={button} className="ck-chat-ending-action">
          {' '}
          <span aria-hidden="true">·</span>{' '}
          {button === 'retry' && (
            <button type="button" className="ck-chat-link" data-ending-action="retry" disabled={busy} onClick={() => run(actions.onRetry!)}>
              {busy ? 'Sending…' : 'Retry'}
            </button>
          )}
          {button === 'signIn' && (
            <button type="button" className="ck-chat-link" data-ending-action="sign-in" onClick={() => actions.onSignIn!()}>
              Sign in
            </button>
          )}
          {button === 'keepGoing' && (
            <button type="button" className="ck-chat-link" data-ending-action="keep-going" disabled={busy} onClick={() => run(actions.keepGoing!.run)}>
              {busy ? 'Starting…' : `Keep going with ${actions.keepGoing!.label}`}
            </button>
          )}
        </span>
      ))}
      {note && (
        <span role="alert" className="ck-chat-ending-note" data-testid="ending-note">
          {' '}
          · {note}
        </span>
      )}
    </>
  );
}
