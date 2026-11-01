// The hop shortcuts: one row per action, each with a recorder, a conflict check and one "Reset to defaults".
import { useCallback } from 'react';
import { KeyRecorder, formatKeys, normalizeKeys } from './KeyRecorder';
import type { DeskBinding, DeskCommand, DeskHotkeyConfig, DeskHotkeys } from '../../lib/desk';

type Field = keyof DeskHotkeyConfig;

const ROWS: { field: Field; label: string; action: string }[] = [
  { field: 'next', label: 'Next screen', action: 'next' },
  { field: 'prev', label: 'Previous screen', action: 'prev' },
  { field: 'screen_n', label: 'Go to screen N', action: 'screen' },
  { field: 'find_pointer', label: 'Find my pointer', action: 'find' },
  { field: 'pause', label: 'Pause sharing', action: 'pause' },
];

/** "screen:3" becomes "Go to screen 3"; other actions use the row label. */
function bindingName(action: string): string {
  if (action.startsWith('screen:')) return `Go to screen ${action.slice('screen:'.length)}`;
  return ROWS.find((r) => r.action === action)?.label ?? action;
}

/** The message when `keys` would clash with another shortcut, or null. */
export function conflictMessage(rowAction: string, keys: string, bindings: DeskBinding[]): string | null {
  const wanted = normalizeKeys(keys);
  const clash = bindings.find((b) => {
    const sameRow = rowAction === 'screen' ? b.action.startsWith('screen:') : b.action === rowAction;
    return !sameRow && normalizeKeys(b.keys) === wanted;
  });
  return clash ? `${formatKeys(keys)} is already used by ${bindingName(clash.action)}.` : null;
}

/** The screen row records one digit; the engine stores it as `{n}`. */
export function toTemplate(keys: string): string {
  return keys.replace(/\+([1-9])$/, '+{n}');
}

export function KeysView({
  hotkeys,
  run,
}: {
  hotkeys: DeskHotkeys;
  run: (cmd: DeskCommand) => Promise<boolean>;
}) {
  const config = hotkeys.config;
  const atDefaults = JSON.stringify(config) === JSON.stringify(hotkeys.defaults);

  const save = useCallback(
    (field: Field, keys: string) => {
      const value = field === 'screen_n' ? toTemplate(keys) : keys;
      void run({ type: 'set_hotkeys', hotkeys: { ...config, [field]: value } });
    },
    [config, run],
  );

  return (
    <section className="ck-desk-section" aria-labelledby="ck-desk-keys-title">
      <div className="ck-desk-section-head">
        <h3 id="ck-desk-keys-title" className="ck-desk-h3">
          Keys
        </h3>
        <button
          type="button"
          className="ck-desk-btn"
          disabled={atDefaults}
          onClick={() => void run({ type: 'reset_hotkeys' })}
        >
          Reset to defaults
        </button>
      </div>
      <ul className="ck-desk-list">
        {ROWS.map((row) => {
          const value = config[row.field];
          return (
            <li key={row.field} className="ck-desk-row">
              <span className="ck-desk-row-label">{row.label}</span>
              <KeyRecorder
                label={row.label}
                value={value}
                check={(keys) => {
                  if (row.field === 'screen_n' && !/\+[1-9]$/.test(keys)) {
                    return 'Press a number from 1 to 9 with the same keys.';
                  }
                  return conflictMessage(row.action, keys, hotkeys.bindings);
                }}
                onRecord={(keys) => save(row.field, keys)}
              />
            </li>
          );
        })}
      </ul>
    </section>
  );
}
