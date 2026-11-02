// One paired computer: name, whether it is there, a switch for each thing it may do (all off at first) and Remove,
// which asks first. State is told by a dot AND words, never by colour alone.
import { useId, useState } from 'react';
import { DESK_CAPABILITIES, type PairDevice } from '../../lib/desk-pair';
import { linkStateText, SIDES, type LinkDevice, type LinkSide } from '../../lib/desk-link';
import { PeerPrefsPanel } from './PeerPrefsPanel';

/** "Connected" / "Offline" / "Paired": plain words for what is known. */
export function deviceStatus(online: boolean | null): string {
  if (online === true) return 'Connected';
  if (online === false) return 'Offline';
  return 'Paired';
}

/** What to check when "Mouse and keyboard" is on here but the link does not come up. */
export function offlineHint(name: string): string {
  return `Open My computers on ${name} and turn on Mouse and keyboard for this computer there too.`;
}

export function DeviceRow({
  device,
  link,
  onToggle,
  onRemove,
  onSide,
  onKeymap,
  onScroll,
  onPointerSpeed,
}: {
  device: PairDevice;
  /** What the desk link says about this computer; absent until it answers. */
  link?: LinkDevice;
  onToggle: (capability: string, on: boolean) => void;
  onRemove: () => void;
  onSide?: (side: LinkSide) => void;
  /** Keys and scrolling for this computer; the panel shows only when both are given. */
  onKeymap?: (altCmd: boolean, winOption: boolean) => void;
  onScroll?: (natural: boolean, speed: number, reverseIncoming: boolean) => void;
  /** Pointer speed when I drive this computer. */
  onPointerSpeed?: (speed: number) => void;
}) {
  const [asking, setAsking] = useState(false);
  const id = useId();
  // The link knows better than the pairing list once "Mouse and keyboard" is on; before that, "Paired".
  const linkText = linkStateText(link?.state);
  const statusText = linkText ?? deviceStatus(device.online);
  const isOn = link ? link.state === 'online' : device.online === true;
  return (
    <li className="ck-desk-device" aria-labelledby={`${id}-name`}>
      <div className="ck-desk-switch-row">
        <div className="ck-desk-stack">
          <span className="ck-desk-row-label" id={`${id}-name`}>
            {device.name}
          </span>
          <span className="ck-desk-device-status" role="status">
            <span className={`ck-desk-dot${isOn ? ' is-on' : ''}`} aria-hidden="true" />
            {statusText}
            {link?.mouse_here ? ' · Mouse is here' : ''}
          </span>
          {link?.state === 'offline' && (
            <span className="ck-desk-hint">{offlineHint(device.name)}</span>
          )}
        </div>
        {!asking && (
          <button type="button" className="ck-desk-btn" aria-label={`Remove ${device.name}`} onClick={() => setAsking(true)}>
            Remove…
          </button>
        )}
      </div>

      {asking ? (
        <div className="ck-desk-confirm" role="alertdialog" aria-label={`Remove ${device.name}?`}>
          <p className="ck-desk-hint">Remove {device.name}? It can't control this computer until you pair again.</p>
          <div className="ck-desk-actions">
            <button type="button" className="ck-desk-btn" onClick={onRemove}>
              Remove
            </button>
            <button type="button" className="ck-desk-btn" autoFocus onClick={() => setAsking(false)}>
              Keep
            </button>
          </div>
        </div>
      ) : (
        <>
        <ul className="ck-desk-caps" aria-label={`What ${device.name} may do`}>
          {DESK_CAPABILITIES.map((cap) => {
            const on = device.allowed.includes(cap.id);
            return (
              <li key={cap.id} className="ck-desk-switch-row">
                <span className="ck-desk-row-label">{cap.label}</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={on}
                  aria-label={`${cap.label} for ${device.name}`}
                  className={`ck-desk-switch${on ? ' is-on' : ''}`}
                  onClick={() => onToggle(cap.id, !on)}
                >
                  <span className="ck-desk-switch-knob" aria-hidden="true" />
                </button>
              </li>
            );
          })}
        </ul>
        {device.allowed.includes('desk/input') && onSide && (
          <label className="ck-desk-switch-row">
            <span className="ck-desk-row-label">Where {device.name} sits</span>
            <select
              className="ck-desk-select"
              aria-label={`Where ${device.name} sits`}
              value={link?.side ?? 'right'}
              onChange={(e) => onSide(e.target.value as LinkSide)}
            >
              {SIDES.map((s) => (
                <option key={s.side} value={s.side}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {device.allowed.includes('desk/input') && onKeymap && onScroll && (
          <PeerPrefsPanel name={device.name} prefs={link?.prefs} onKeymap={onKeymap} onScroll={onScroll} onPointerSpeed={onPointerSpeed} />
        )}
        </>
      )}
    </li>
  );
}
