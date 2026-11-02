// A shortcut field: click, press the new keys, Esc cancels. Pure helpers are exported for tests.
import { useEffect, useState } from 'react';

const MODIFIER_KEYS = new Set(['Control', 'Alt', 'Shift', 'Meta', 'AltGraph', 'OS']);
const NAMED: Record<string, string> = {
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
  ' ': 'space',
  ScrollLock: 'scrolllock',
};

/** `ctrl+alt+right` -> `Ctrl Alt Right`. `{n}` shows as `1…9`. */
export function formatKeys(keys: string): string {
  return keys
    .split('+')
    .map((t) => {
      if (t === '{n}') return '1…9';
      if (t === 'ctrl') return 'Ctrl';
      if (t === 'alt') return 'Alt';
      if (t === 'shift') return 'Shift';
      if (t === 'win') return 'Win';
      if (t === 'scrolllock') return 'Scroll Lock';
      if (/^f\d+$/.test(t)) return t.toUpperCase();
      return t.length === 1 ? t.toUpperCase() : t.charAt(0).toUpperCase() + t.slice(1);
    })
    .join(' ');
}

/** The same keys in one fixed order, so two shortcuts can be compared. */
export function normalizeKeys(keys: string): string {
  return keys
    .split('+')
    .map((t) => t.trim().toLowerCase())
    .sort()
    .join('+');
}

export type RecordResult = { ok: true; keys: string } | { ok: false; message: string };

/** Turn one key press into a shortcut string the engine reads. Modifier-only presses are ignored by the caller. */
export function chordFromEvent(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'> & { code?: string },
): RecordResult {
  let key: string | undefined;
  if (NAMED[e.key]) key = NAMED[e.key];
  else if (/^[a-zA-Z]$/.test(e.key)) key = e.key.toLowerCase();
  else if (/^[0-9]$/.test(e.key)) key = e.key;
  else if (/^F([1-9]|1[0-2])$/.test(e.key)) key = e.key.toLowerCase();
  // With Shift held, `key` is the shifted symbol ("&" for 7): the physical key says which digit or letter it was.
  else if (e.code && /^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
  else if (e.code && /^Key[A-Z]$/.test(e.code)) key = e.code.slice(3).toLowerCase();
  if (!key) return { ok: false, message: "That key can't be used for a shortcut." };
  const modifiers = [e.ctrlKey && 'ctrl', e.altKey && 'alt', e.shiftKey && 'shift', e.metaKey && 'win'].filter(
    Boolean,
  ) as string[];
  const bareOk = key === 'scrolllock' || /^f\d+$/.test(key);
  if (!bareOk && modifiers.length === 0) {
    return { ok: false, message: 'Add Ctrl, Alt or Shift, so typing still works.' };
  }
  return { ok: true, keys: [...modifiers, key].join('+') };
}

export function KeyRecorder({
  label,
  value,
  check,
  onRecord,
  disabled,
}: {
  /** Names the row for screen readers, like "Next screen". */
  label: string;
  /** The shortcut now in use, as the engine writes it. */
  value: string;
  /** Returns a message when the new keys cannot be used, otherwise null. */
  check: (keys: string) => string | null;
  onRecord: (keys: string) => void;
  disabled?: boolean;
}) {
  const [recording, setRecording] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setRecording(false);
        setMessage(null);
        return;
      }
      if (MODIFIER_KEYS.has(e.key)) return;
      const result = chordFromEvent(e);
      if (!result.ok) {
        setMessage(result.message);
        return;
      }
      const conflict = check(result.keys);
      if (conflict) {
        setMessage(conflict);
        return;
      }
      setRecording(false);
      setMessage(null);
      onRecord(result.keys);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [recording, check, onRecord]);

  const cancel = () => {
    setRecording(false);
    setMessage(null);
  };

  return (
    <div className="ck-desk-recorder">
      {recording ? (
        <>
          <span className="ck-desk-recording" role="status">
            Press the new keys… Esc to cancel.
          </span>
          <button type="button" className="ck-desk-btn" onClick={cancel} aria-label={`Cancel changing ${label}`}>
            Cancel
          </button>
        </>
      ) : (
        <button
          type="button"
          className="ck-desk-keys"
          disabled={disabled}
          aria-label={`Change ${label}, now ${formatKeys(value)}`}
          onClick={() => {
            setMessage(null);
            setRecording(true);
          }}
        >
          {formatKeys(value)}
        </button>
      )}
      {message && (
        <span className="ck-desk-hint" role="alert">
          {message}
        </span>
      )}
    </div>
  );
}
