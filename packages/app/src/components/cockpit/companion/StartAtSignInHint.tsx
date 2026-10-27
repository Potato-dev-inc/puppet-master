// The Companion page's Setup card: one line that says a restart leaves the companion asleep until the app opens,
// with a "Turn on" button. Shown only while the sign-in start is off and the desktop shell answers.
import { useStartAtSignIn } from '../../../hooks/useStartAtSignIn';

export function StartAtSignInHint() {
  const start = useStartAtSignIn();
  if (!start.ready || start.enabled) return null;
  return (
    <>
      <div className="ck-companion-meta">
        <span className="ck-sub">After a restart, your companion waits until you open Puppet Master.</span>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void start.change(true)}>
          Turn on
        </button>
      </div>
      {start.error && (
        <p className="ck-sub" role="alert">
          {start.error}
        </p>
      )}
    </>
  );
}
