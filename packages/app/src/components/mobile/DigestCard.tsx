// "While you were away" card for the All view of the phone PWA. Asks the PC once per visit for the
// digest since this device's last visit (`seen: true` stores the visit), shows it until "Got it".
import { useEffect, useState } from 'react';
import type { BridgeClient } from '../../lib/bridge-types';
import type { Digest } from '../../lib/cockpit-api';

type DigestResult = Digest & { empty: boolean; previousSeenMs: number | null };

const AWAY_MS = 30_000;
/** A visit within this long of the last one is not an absence: the card stays quiet. */
const MIN_ABSENCE_MS = 2 * 60_000;
const MAX_LINES = 3;

/** Survives unmounts (the All view unmounts on tab switches) until dismissed. */
let held: DigestResult | null = null;
/** nowMs of the last digest the user dismissed; older or equal digests are not shown again. */
let dismissedNowMs: number | null = null;

export function resetDigestCardForTests() {
  held = null;
  dismissedNowMs = null;
}

function plural(n: number, one: string, many: string) {
  return n === 1 ? one : many;
}

export function DigestCard({
  bridge,
  bridgeReady,
  onOpen,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  onOpen: (paneId: string) => void;
}) {
  const [digest, setDigest] = useState<DigestResult | null>(held);

  useEffect(() => {
    if (!bridgeReady) return;
    let live = true;
    const load = async () => {
      if (!bridge.digest) return;
      try {
        const result = await bridge.digest({ seen: true });
        if (!live || result.empty || result.nowMs - result.sinceMs < MIN_ABSENCE_MS) return;
        if (dismissedNowMs !== null && result.nowMs <= dismissedNowMs) return;
        held = result;
        setDigest(result);
      } catch {
        // The card is optional; a failed fetch shows nothing.
      }
    };
    void load();

    let hiddenAt: number | null = null;
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') {
        hiddenAt = Date.now();
        return;
      }
      if (hiddenAt !== null && Date.now() - hiddenAt >= AWAY_MS) void load();
      hiddenAt = null;
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      live = false;
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [bridge, bridgeReady]);

  if (!bridge.digest || !digest || digest.empty) return null;

  const dismiss = () => {
    dismissedNowMs = digest.nowMs;
    held = null;
    setDigest(null);
  };

  const shown = digest.lines.slice(0, MAX_LINES);
  const more = digest.lines.length - shown.length;
  const needs = digest.needsYou.length;

  return (
    <section aria-label="While you were away" className="mb-2 rounded-lg border border-pm-border bg-pm-panel p-3 text-xs">
      <div className="flex items-center justify-between gap-2">
        <span className="text-pm-muted">While you were away</span>
        <button
          type="button"
          onClick={dismiss}
          className="min-h-[32px] rounded px-2 text-pm-muted hover:text-pm-text"
        >
          Got it
        </button>
      </div>
      <p className="mt-1 font-semibold">
        {digest.agentsWorked} {plural(digest.agentsWorked, 'agent worked', 'agents worked')}
      </p>
      {shown.length > 0 && (
        <ul className="mt-2 space-y-1">
          {shown.map((line) => (
            <li key={line.paneId}>
              <button
                type="button"
                onClick={() => onOpen(line.paneId)}
                className="block w-full rounded px-1 py-1 text-left line-clamp-2 hover:bg-pm-border/30"
              >
                {line.sentence}
              </button>
            </li>
          ))}
        </ul>
      )}
      {more > 0 && <p className="mt-1 text-pm-muted">+{more} more</p>}
      {needs > 0 && (
        <p className="mt-1 text-pm-muted">
          {needs} {plural(needs, 'needs', 'need')} you
        </p>
      )}
    </section>
  );
}
