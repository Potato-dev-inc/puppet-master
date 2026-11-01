// One paired computer: name, whether it is there, a switch for each thing it may do (all off at first) and Remove,
// which asks first. State is told by a dot AND words, never by colour alone.
import { useId, useState } from 'react';
import { DESK_CAPABILITIES, type PairDevice } from '../../lib/desk-pair';

/** "Connected" / "Offline" / "Paired": plain words for what is known. */
export function deviceStatus(online: boolean | null): string {
  if (online === true) return 'Connected';
  if (online === false) return 'Offline';
  return 'Paired';
}

export function DeviceRow({
  device,
  onToggle,
  onRemove,
}: {
  device: PairDevice;
  onToggle: (capability: string, on: boolean) => void;
  onRemove: () => void;
}) {
  const [asking, setAsking] = useState(false);
  const id = useId();
  return (
    <li className="ck-desk-device" aria-labelledby={`${id}-name`}>
      <div className="ck-desk-switch-row">
        <div className="ck-desk-stack">
          <span className="ck-desk-row-label" id={`${id}-name`}>
            {device.name}
          </span>
          <span className="ck-desk-device-status" role="status">
            <span className={`ck-desk-dot${device.online === true ? ' is-on' : ''}`} aria-hidden="true" />
            {deviceStatus(device.online)}
          </span>
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
      )}
    </li>
  );
}
