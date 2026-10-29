// Settings > General > Focus Time: the default length of a Focus and the quiet switch for cards that wait for
// the person. Blocking cards are never held, in any setting; the switch only decides whether they buzz during
// Focus. Saved through the desktop `focus_settings_set` command (not settings.json), like the loop guard block.
import { useEffect, useState } from 'react';
import { FOCUS_MAX_MINUTES, FOCUS_MIN_MINUTES, focusSettingsSet, focusState, type FocusSettings } from '../../../lib/focus';
import { NumberControl } from '../controls';
import { Note, SettingRow, SettingToggle } from '../settings-ui';

/** The length a new Focus gets when nothing valid was typed (the Rust default). */
export const FOCUS_DEFAULT_MINUTES = 45;

/** A whole number of minutes from 5 to 240. Not a number: the default. */
export function clampFocusMinutes(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return FOCUS_DEFAULT_MINUTES;
  return Math.min(FOCUS_MAX_MINUTES, Math.max(FOCUS_MIN_MINUTES, Math.round(value)));
}

const LENGTH_HINT = 'How long a Focus runs when you start one without picking a length. 5 to 240 minutes.';
const QUIET_HINT =
  "Cards that wait for you are never held. They still show up in the Inbox. Your phone and Telegram just don't buzz. After 20 minutes you get one reminder.";
const FOOTER =
  'Urgent things always come through: a stuck agent, a spent budget, a high-risk request, or an agent asking for more control. Focus never answers anything for you.';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

export function FocusSettingsBlock({ onSaved }: { onSaved?: () => void }) {
  const [settings, setSettings] = useState<FocusSettings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    focusState()
      .then((view) => {
        if (live) setSettings(view.settings);
      })
      .catch(() => {}); // outside the desktop shell there is no Focus to configure: the block stays hidden
    return () => {
      live = false;
    };
  }, []);

  if (!settings) return null;

  /** Saves the whole pair. On a refusal the saved values stay on screen and the reason shows under the rows. */
  const save = (next: FocusSettings) => {
    setError(null);
    focusSettingsSet(next)
      .then((view) => {
        setSettings(view.settings);
        onSaved?.();
      })
      .catch((err) => setError(`Not saved: ${errText(err)}`));
  };

  return (
    <>
      <SettingRow label="Default length" description={LENGTH_HINT}>
        <NumberControl
          aria-label="Default length (minutes)"
          min={FOCUS_MIN_MINUTES}
          max={FOCUS_MAX_MINUTES}
          value={settings.default_minutes}
          normalize={clampFocusMinutes}
          suffix="min"
          onCommit={(minutes) => save({ ...settings, default_minutes: minutes })}
        />
      </SettingRow>
      <SettingToggle
        label="Make cards that wait for you quiet during Focus"
        description={QUIET_HINT}
        checked={settings.quiet_blocking}
        onChange={(quiet_blocking) => save({ ...settings, quiet_blocking })}
      />
      {error && <Note role="alert">{error}</Note>}
      <Note>{FOOTER}</Note>
    </>
  );
}
