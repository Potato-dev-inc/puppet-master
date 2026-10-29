// Settings > General > Labs > Red to Green. Off by default. Shows the GitHub sign-in, the four caps (tries per
// pull request, turns and minutes per try, fixes a day) and the pull requests Puppet Master watches, with their
// buttons. Every change is one `ciFixSaveSettings` or one `ciFixAct`; the backend clamps the numbers again.
import { useCallback, useEffect, useState } from 'react';
import {
  ciFixAct,
  ciFixSaveSettings,
  ciFixView,
  onCiFixChanged,
  watchLine,
  type CiFixAction,
  type CiFixSettings as CiFixConfig,
  type CiFixView,
  type GhView,
  type WatchView,
} from '../../../lib/ci-fix';
import { NumberControl } from '../controls';
import { Btn, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';

/** The ranges of the four caps. The backend clamps to the same numbers. */
export const CAPS = {
  tries: { min: 1, max: 3 },
  turns: { min: 1, max: 50 },
  minutes: { min: 5, max: 60 },
  fixes: { min: 1, max: 20 },
} as const;

/** What the block promises. Shown as the footer; the only place "merge", "re-run" and "force" may appear. */
export const FOOTER =
  "Puppet Master only watches pull requests it opened. It asks before the agent reads the log, and again before anything is pushed. The agent can't push during a fix. Puppet Master never re-runs CI, merges or force-pushes.";

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** The GitHub line under the switch: whose sign-in it is, or what to install. */
export function ciGhLine(gh: GhView): string {
  if (gh.state === 'ready') return gh.login ? `Signed in to GitHub as @${gh.login} (gh)` : 'Signed in to GitHub (gh)';
  return 'Install the GitHub CLI and run gh auth login. Nothing is checked until then.';
}

/** One button of a watched pull request row. `id` is the watch id, or the id of its last fix for "Ask again". */
export interface WatchButton {
  label: string;
  id: string;
  action: CiFixAction;
}

/** The buttons a watched row offers, in order: Stop watching, Watch again, then Ask again for a declined or expired fix. */
export function watchButtons(w: WatchView): WatchButton[] {
  const out: WatchButton[] = [];
  if (w.state === 'watching' || w.state === 'paused') out.push({ label: 'Stop watching', id: w.id, action: 'stop_watching' });
  if (w.state === 'paused' || w.state === 'stopped') out.push({ label: 'Watch again', id: w.id, action: 'watch_again' });
  const fix = w.lastFix;
  if (fix && (fix.state === 'declined' || fix.state === 'expired')) out.push({ label: 'Ask again', id: fix.id, action: 'ask_again' });
  return out;
}

function WatchRow({ watch, busy, onAct }: { watch: WatchView; busy: boolean; onAct: (id: string, action: CiFixAction) => void }) {
  return (
    <li className="st-device" aria-label={`#${watch.prNumber} ${watch.repo}`}>
      <div className="st-device-head">
        <div className="st-label">
          <span>{watchLine(watch)}</span>
        </div>
        <div className="st-control">
          {watchButtons(watch).map((b) => (
            <Btn key={`${b.action}-${b.id}`} small disabled={busy} onClick={() => onAct(b.id, b.action)}>
              {b.label}
            </Btn>
          ))}
        </div>
      </div>
    </li>
  );
}

export function CiFixSettings({ onSaved }: { onSaved?: () => void }) {
  const [view, setView] = useState<CiFixView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setView(await ciFixView());
      setError(null);
    } catch (e) {
      // No desktop shell (a browser preview) or the backend is not ready: say so, do not crash.
      setError(`Could not read this setting: ${errText(e)}`);
    }
  }, []);

  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    void load();
    void onCiFixChanged(() => void load())
      .then((off) => {
        if (alive) stop = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      stop?.();
    };
  }, [load]);

  /** Save the whole settings object. Errors show here; the view is replaced by what the backend returns. */
  const save = async (next: CiFixConfig) => {
    setBusy(true);
    setError(null);
    try {
      setView(await ciFixSaveSettings(next));
      onSaved?.();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  const act = async (id: string, action: CiFixAction) => {
    setBusy(true);
    setError(null);
    try {
      setView(await ciFixAct(id, action));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return error ? <Note role="alert">{error}</Note> : <Note role="status">Loading…</Note>;

  const { settings, gh, watches } = view;
  return (
    <>
      {error && <Note role="alert">{error}</Note>}
      <SettingToggle
        label="Offer to fix failed CI"
        description="When CI fails on a pull request Puppet Master opened, you get a card to have the same agent fix it."
        checked={settings.enabled}
        disabled={busy}
        onChange={(on) => void save({ ...settings, enabled: on })}
      />
      <SettingRow label="GitHub" description={ciGhLine(gh)} />
      <SettingRow label="Tries per pull request" description="How many fix tries one pull request may get, 1 to 3.">
        <NumberControl
          aria-label="Tries per pull request"
          min={CAPS.tries.min}
          max={CAPS.tries.max}
          value={settings.tries_per_pr}
          disabled={busy}
          onCommit={(next) => save({ ...settings, tries_per_pr: next })}
        />
      </SettingRow>
      <SettingRow label="Turns per try" description="How many turns the agent may take in one try, 1 to 50.">
        <NumberControl
          aria-label="Turns per try"
          min={CAPS.turns.min}
          max={CAPS.turns.max}
          value={settings.turns_per_try}
          disabled={busy}
          onCommit={(next) => save({ ...settings, turns_per_try: next })}
        />
      </SettingRow>
      <SettingRow label="Minutes per try" description="How long one try may run, 5 to 60 minutes.">
        <NumberControl
          aria-label="Minutes per try"
          min={CAPS.minutes.min}
          max={CAPS.minutes.max}
          value={settings.minutes_per_try}
          disabled={busy}
          onCommit={(next) => save({ ...settings, minutes_per_try: next })}
        />
      </SettingRow>
      <SettingRow label="Fixes a day" description="How many fixes Puppet Master starts in one day, across all pull requests, 1 to 20.">
        <NumberControl
          aria-label="Fixes a day"
          min={CAPS.fixes.min}
          max={CAPS.fixes.max}
          value={settings.fixes_per_day}
          disabled={busy}
          onCommit={(next) => save({ ...settings, fixes_per_day: next })}
        />
      </SettingRow>

      <SettingBlock label="Watched pull requests" description="Pull requests Puppet Master watches, with the state of each one.">
        {watches.length === 0 ? (
          <Note>No pull requests yet. Puppet Master watches one once it opens it.</Note>
        ) : (
          <ul className="st-devices" aria-label="Watched pull requests">
            {watches.map((w) => (
              <WatchRow key={w.id} watch={w} busy={busy} onAct={(id, action) => void act(id, action)} />
            ))}
          </ul>
        )}
      </SettingBlock>

      <Note>{FOOTER}</Note>
    </>
  );
}
