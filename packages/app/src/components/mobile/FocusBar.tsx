// Focus on the phone (SPEC n33 3.6 (7)): one card on the All view, above "While you were away". Off: "Focus" and
// three length chips. On: "Focus · 31 min left" and End. After an end: the one summary line with "Got it".
// Counts only: it reads the redacted `GET /focus` (PhoneView), so no pane ids, agent names or sentences. Start and
// End need the `control` cap; without it the card only reads (the PC refuses the same calls anyway).
// It reads on mount, when the app comes back to the front, when `focusRevision` changes (the bridge's `focus`
// event), and every 30 s while Focus is on. Without `bridge.focus` (an old PC) it draws nothing.
import { useEffect, useState } from 'react';
import type { BridgeClient } from '../../lib/bridge-types';
import { FOCUS_PRESETS, leftText, type PhoneFocus } from '../../lib/focus';

/** The read repeats this often while Focus is on; the countdown redraws on the shorter tick. */
const REFRESH_MS = 30_000;
const TICK_MS = 15_000;
const CHANGE_FAILED = "Couldn't change Focus. Try again.";

/** The `ended_at_ms` of the last summary this device dismissed with "Got it". Kept until the page reloads. */
let dismissedEndedAt: number | null = null;

export function resetFocusBarForTests() {
  dismissedEndedAt = null;
}

export function FocusBar({
  bridge,
  bridgeReady,
  canControl,
  focusRevision = 0,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  /** This phone may start or end Focus (`control`). Without it the bar only shows the state. */
  canControl: boolean;
  /** Bumped when the bridge says Focus changed (`event: focus`): the bar reads again. */
  focusRevision?: number;
}) {
  const [view, setView] = useState<PhoneFocus | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(dismissedEndedAt);
  const [now, setNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const on = view?.on === true;

  // Mount, return to the app, and a bumped revision: read the PC's view.
  useEffect(() => {
    if (!bridgeReady || !bridge.focus) return;
    let live = true;
    const read = () => {
      bridge
        .focus?.()
        .then((next) => {
          if (live) setView(next);
        })
        // The bar is optional: a refused or failed read keeps what is shown.
        .catch(() => undefined);
    };
    read();
    const onVisible = () => {
      if (document.visibilityState === 'visible') read();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      live = false;
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [bridge, bridgeReady, focusRevision]);

  // While Focus is on: the countdown redraws often and the PC's view is read again every 30 s.
  useEffect(() => {
    if (!on || !bridgeReady || !bridge.focus) return;
    const tick = window.setInterval(() => setNow(Date.now()), TICK_MS);
    const refresh = window.setInterval(() => {
      bridge
        .focus?.()
        .then(setView)
        .catch(() => undefined);
    }, REFRESH_MS);
    return () => {
      window.clearInterval(tick);
      window.clearInterval(refresh);
    };
  }, [bridge, bridgeReady, on]);

  if (!bridge.focus || !view) return null;

  /** Start, or End: the PC answers with the new view; a refusal shows one line and changes nothing here. */
  const change = (call: () => Promise<PhoneFocus> | undefined) => {
    setBusy(true);
    setError(null);
    call()
      ?.then((next) => {
        setView(next);
        setNow(Date.now());
      })
      .catch(() => setError(CHANGE_FAILED))
      .finally(() => setBusy(false));
  };
  const dismiss = (endedAt: number) => {
    dismissedEndedAt = endedAt;
    setDismissed(endedAt);
  };

  const summary = !view.on && view.last && view.last.ended_at_ms !== dismissed ? view.last : null;
  const endAt = view.endsAtMs;

  return (
    <section aria-label="Focus" className="ph-cards">
      <div className="ph-card">
        {summary ? (
          <>
            <div className="ph-card-cap">
              <span>Focus</span>
              <button type="button" onClick={() => dismiss(summary.ended_at_ms)} className="ph-btn ph-btn--plain">
                Got it
              </button>
            </div>
            <p className="ph-card-q">{summary.line}</p>
          </>
        ) : view.on && endAt !== null ? (
          <>
            <p className="ph-card-q">Focus · {leftText(endAt, now)}</p>
            {canControl && bridge.focusEnd && (
              <div className="ph-appr-actions">
                <button
                  type="button"
                  className="ph-btn"
                  aria-label="End Focus"
                  disabled={busy}
                  onClick={() => change(() => bridge.focusEnd?.())}
                >
                  End
                </button>
              </div>
            )}
          </>
        ) : (
          <>
            <p className="ph-card-q">Focus</p>
            {canControl && bridge.focusStart ? (
              <div className="ph-answers">
                {FOCUS_PRESETS.map((minutes) => (
                  <button
                    key={minutes}
                    type="button"
                    className="ph-btn"
                    aria-label={`Focus for ${minutes} minutes`}
                    disabled={busy}
                    onClick={() => change(() => bridge.focusStart?.(minutes))}
                  >
                    {minutes}
                  </button>
                ))}
              </div>
            ) : (
              <p className="ph-card-wait">Off</p>
            )}
          </>
        )}
        {error && <p className="ph-appr-status">{error}</p>}
      </div>
    </section>
  );
}
