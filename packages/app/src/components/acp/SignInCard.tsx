// "Sign in to Claude": shown when a chat session needs the agent's own sign-in. Sign-in happens in a normal, visible
// terminal pane (opened by "Open sign-in"), the same login the terminal uses; this card never sees a credential.
// "I'm signed in — try again" sends the message that was waiting.
import { useState } from 'react';
import '../../styles/cockpit-acp.css';

export interface SignInCardProps {
  label: string;
  /** What to do in the terminal that opens (from the agent's table). */
  hint: string;
  /** Opens the agent's own terminal pane. */
  onOpen: () => Promise<unknown> | void;
  /** Tries again. Rejects with a plain sentence when the agent still wants a sign-in. */
  onRetry: () => Promise<unknown> | void;
}

export function SignInCard({ label, hint, onOpen, onRetry }: SignInCardProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<unknown> | void) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError((e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong.').replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, ''));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ck-acp-card" data-testid="signin-card" role="group" aria-label={`Sign in to ${label}`}>
      <div className="ck-acp-card-body">
        <p className="ck-acp-card-title">Sign in to {label} to use Chat</p>
        <p className="ck-acp-card-line">{hint || 'Chat uses the same sign-in as the terminal. Sign in there, then come back.'}</p>
        {error && (
          <p className="ck-acp-card-line is-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="ck-acp-card-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void run(onOpen)}>
          Open sign-in
        </button>
        <button type="button" className="ck-btn" disabled={busy} onClick={() => void run(onRetry)}>
          I’m signed in — try again
        </button>
      </div>
    </div>
  );
}
