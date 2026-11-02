// Keys and scrolling for one paired computer, folded away until wanted. Four switches and a speed, each saved at once.
// Plain words: what the swap does, and which way round it works. The state is a switch, never colour alone.
import { useId } from 'react';
import { DEFAULT_PEER_PREFS, SCROLL_SPEEDS, type PeerPrefs } from '../../lib/desk-link';

/** "1×", "0.5×". */
export function speedLabel(speed: number): string {
  return `${Number.isInteger(speed) ? speed : String(speed)}×`;
}

function Row({
  label,
  hint,
  on,
  aria,
  onChange,
}: {
  label: string;
  hint?: string;
  on: boolean;
  aria: string;
  onChange: (on: boolean) => void;
}) {
  return (
    <li className="ck-desk-switch-row">
      <div className="ck-desk-stack">
        <span className="ck-desk-row-label">{label}</span>
        {hint && <span className="ck-desk-hint">{hint}</span>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={aria}
        className={`ck-desk-switch${on ? ' is-on' : ''}`}
        onClick={() => onChange(!on)}
      >
        <span className="ck-desk-switch-knob" aria-hidden="true" />
      </button>
    </li>
  );
}

export function PeerPrefsPanel({
  name,
  prefs = DEFAULT_PEER_PREFS,
  onKeymap,
  onScroll,
}: {
  name: string;
  prefs?: PeerPrefs;
  onKeymap: (altCmd: boolean, winOption: boolean) => void;
  onScroll: (natural: boolean, speed: number, reverseIncoming: boolean) => void;
}) {
  const id = useId();
  // A speed saved by hand that is not on the list still shows, so the select never lies about it.
  const speeds = SCROLL_SPEEDS.includes(prefs.speed) ? SCROLL_SPEEDS : [...SCROLL_SPEEDS, prefs.speed].sort((a, b) => a - b);
  return (
    <details className="ck-desk-more">
      <summary className="ck-desk-row-label">Keys and scrolling with {name}</summary>
      <ul className="ck-desk-caps" aria-label={`Keys and scrolling with ${name}`}>
        <Row
          label="Alt and Command trade places"
          hint={`For keys typed on ${name} when one is a Windows PC and the other a Mac.`}
          on={prefs.alt_cmd}
          aria={`Swap Alt and Command for ${name}`}
          onChange={(on) => onKeymap(on, prefs.win_option)}
        />
        <Row
          label="Windows key and Option trade places"
          on={prefs.win_option}
          aria={`Swap Windows key and Option for ${name}`}
          onChange={(on) => onKeymap(prefs.alt_cmd, on)}
        />
        <Row
          label={`Reverse scrolling when I use ${name}`}
          hint="Turns the wheel the other way, like a Mac's natural scrolling."
          on={prefs.natural}
          aria={`Reverse scrolling when I use ${name}`}
          onChange={(on) => onScroll(on, prefs.speed, prefs.reverse_incoming)}
        />
        <Row
          label={`Reverse scrolling when ${name} uses this computer`}
          on={prefs.reverse_incoming}
          aria={`Reverse scrolling when ${name} uses this computer`}
          onChange={(on) => onScroll(prefs.natural, prefs.speed, on)}
        />
        <li className="ck-desk-switch-row">
          <label htmlFor={`${id}-speed`} className="ck-desk-row-label">
            Scroll speed when I use {name}
          </label>
          <select
            id={`${id}-speed`}
            className="ck-desk-select"
            value={prefs.speed}
            onChange={(e) => onScroll(prefs.natural, Number(e.target.value), prefs.reverse_incoming)}
          >
            {speeds.map((s) => (
              <option key={s} value={s}>
                {speedLabel(s)}
              </option>
            ))}
          </select>
        </li>
      </ul>
    </details>
  );
}
