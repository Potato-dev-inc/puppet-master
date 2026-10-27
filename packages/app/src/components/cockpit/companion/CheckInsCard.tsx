// "Check-ins" (SPEC sections 5 and 8): the scheduled messages the companion sends through its pane. The list comes
// from the view when the backend sends it; until then the card shows only the next one and no add button, so a save
// can never replace check-ins the card cannot see.
import { useState } from 'react';
import type { CheckIn, CompanionTransport, CompanionView, NextCheckIn } from '../../../lib/companion';
import { formatWhen } from '../../../lib/companion';
import { Sheet } from '../Sheet';
import { DAY_NAMES, WHEN_OPTIONS, buildWhen, checkInLabel, newCheckInId, parseClock, type WhenKind } from './model';

const MAX_CHECKINS = 8;
const MAX_TEXT = 2000;

export function CheckInsCard({
  companionId,
  checkins,
  nextCheckin,
  pushReplies,
  transport,
  onChanged,
}: {
  companionId: string;
  /** `undefined`: the view does not carry the list yet. */
  checkins: CheckIn[] | undefined;
  nextCheckin: NextCheckIn | null;
  /** The companion's "Tell me on my phone" setting. `undefined` (an older view) counts as on. */
  pushReplies?: boolean;
  transport: CompanionTransport;
  onChanged: (next: CompanionView) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pushOn = pushReplies ?? true;

  const save = async (next: CheckIn) => {
    setError(null);
    try {
      onChanged(await transport.setCheckIns(companionId, [...(checkins ?? []), next]));
      setAdding(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save the check-in.');
    }
  };

  const togglePush = async () => {
    if (busy) return;
    setError(null);
    setBusy(true);
    try {
      onChanged(await transport.update(companionId, { pushReplies: !pushOn }));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save the setting.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="ck-companion-card" aria-labelledby="ck-companion-checkins-title">
      <h2 className="ck-companion-card-title" id="ck-companion-checkins-title">
        Check-ins
      </h2>
      <div className="ck-companion-push">
        <span className="ck-companion-push-text">
          <span>Tell me on my phone</span>
          <span className="ck-sub">A push when it replies.</span>
        </span>
        <button
          type="button"
          role="switch"
          aria-checked={pushOn}
          aria-label="Tell me on my phone"
          className="ck-switch"
          disabled={busy}
          onClick={() => void togglePush()}
        >
          <span className="ck-switch-knob" />
        </button>
      </div>
      {checkins === undefined ? (
        nextCheckin ? (
          <p className="ck-sub">Next: {nextCheckin.label}</p>
        ) : (
          <p className="ck-sub">No check-ins yet.</p>
        )
      ) : (
        <ul className="ck-companion-list">
          {checkins.map((c) => (
            <li className={`ck-companion-checkin${c.enabled ? '' : ' is-off'}`} key={c.id}>
              <span className="ck-companion-checkin-label">{c.label}</span>
              <span className="ck-sub">{formatWhen(c.when)}</span>
            </li>
          ))}
          {checkins.length === 0 && <li className="ck-sub">No check-ins yet.</li>}
        </ul>
      )}
      {checkins !== undefined && checkins.length < MAX_CHECKINS && (
        <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary ck-companion-add" onClick={() => setAdding(true)}>
          Add a check-in
        </button>
      )}
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
      {adding && <CheckInSheet onCancel={() => setAdding(false)} onSave={save} />}
    </section>
  );
}

/** The add sheet: a `When` preset with its fields, and what to ask. The label is taken from the text. */
function CheckInSheet({ onCancel, onSave }: { onCancel: () => void; onSave: (next: CheckIn) => Promise<void> }) {
  const [kind, setKind] = useState<WhenKind>('daily');
  const [time, setTime] = useState('08:00');
  const [day, setDay] = useState(0);
  const [hours, setHours] = useState(2);
  const [onceAt, setOnceAt] = useState('');
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);

  const at = parseClock(time);
  const onceMs = onceAt ? new Date(onceAt).getTime() : NaN;
  const trimmed = text.trim();
  const valid =
    trimmed.length > 0 &&
    trimmed.length <= MAX_TEXT &&
    (kind === 'every' ? hours >= 1 && hours <= 24 : kind === 'once' ? Number.isFinite(onceMs) : at !== null);

  const submit = async () => {
    if (!valid || busy) return;
    const when = buildWhen(kind, { at: at ?? { h: 8, m: 0 }, day, hours, atMs: onceMs });
    setBusy(true);
    try {
      await onSave({ id: newCheckInId(), label: checkInLabel(trimmed), text: trimmed, when, enabled: true });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet onClose={busy ? undefined : onCancel} labelledBy="ck-checkin-sheet-title" className="ck-companion-sheet">
      <h3 className="ck-section" id="ck-checkin-sheet-title">
        Add a check-in
      </h3>
      <label className="ck-companion-field">
        <span className="ck-caption">When</span>
        <select className="ck-companion-input" value={kind} onChange={(e) => setKind(e.target.value as WhenKind)}>
          {WHEN_OPTIONS.map((o) => (
            <option key={o.kind} value={o.kind}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      {(kind === 'daily' || kind === 'weekdays' || kind === 'weekly') && (
        <div className="ck-companion-row-fields">
          {kind === 'weekly' && (
            <select className="ck-companion-input" aria-label="Day" value={day} onChange={(e) => setDay(Number(e.target.value))}>
              {DAY_NAMES.map((d, i) => (
                <option key={d} value={i}>
                  {d}
                </option>
              ))}
            </select>
          )}
          <input className="ck-companion-input" type="time" aria-label="Time" value={time} onChange={(e) => setTime(e.target.value)} />
        </div>
      )}
      {kind === 'every' && (
        <label className="ck-companion-field">
          <span className="ck-caption">Hours</span>
          <input className="ck-companion-input" type="number" min={1} max={24} value={hours} onChange={(e) => setHours(Number(e.target.value))} />
        </label>
      )}
      {kind === 'once' && (
        <label className="ck-companion-field">
          <span className="ck-caption">Date and time</span>
          <input className="ck-companion-input" type="datetime-local" value={onceAt} onChange={(e) => setOnceAt(e.target.value)} />
        </label>
      )}
      <label className="ck-companion-field">
        <span className="ck-caption">What to ask</span>
        <textarea className="ck-companion-input ck-companion-input--area" value={text} maxLength={MAX_TEXT} onChange={(e) => setText(e.target.value)} />
      </label>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" onClick={() => void submit()} disabled={!valid || busy}>
          Add
        </button>
      </div>
    </Sheet>
  );
}
