// "New routine": a small form in the Routines tab. A name, what to do, and when (every hour, every day at a time,
// weekdays at a time, every few minutes, or once at a date and time). Saving hands the routine to the Rust side,
// which checks it again and gives the answer.
import { useId, useState, type FormEvent } from 'react';
import { plainError } from '../../../lib/potato';
import {
  buildRoutineInput,
  DEFAULT_ROUTINE_FORM,
  ROUTINE_COPY,
  type RoutineFormValues,
  type RoutineFreq,
  type RoutineView,
  type RoutinesTransport,
} from '../../../lib/potato-routines';

export interface RoutineFormProps {
  botId: string;
  transport: RoutinesTransport;
  onSaved: (routine: RoutineView) => void;
  onCancel: () => void;
}

const FREQ_OPTIONS: readonly { value: RoutineFreq; label: string }[] = [
  { value: 'hourly', label: 'Every hour' },
  { value: 'daily', label: 'Every day' },
  { value: 'weekdays', label: 'Weekdays' },
  { value: 'every', label: 'Every few minutes' },
  { value: 'once', label: 'Once' },
];

export function RoutineForm({ botId, transport, onSaved, onCancel }: RoutineFormProps) {
  const [values, setValues] = useState<RoutineFormValues>(DEFAULT_ROUTINE_FORM);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const ids = { title: useId(), prompt: useId(), freq: useId(), time: useId(), minutes: useId(), at: useId() };

  const set = <K extends keyof RoutineFormValues>(key: K, value: RoutineFormValues[K]) =>
    setValues((current) => ({ ...current, [key]: value }));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    const built = buildRoutineInput(values);
    if (typeof built === 'string') {
      setError(built);
      return;
    }
    setError(null);
    setSaving(true);
    transport
      .create(botId, built)
      .then(
        (routine) => onSaved(routine),
        (e: unknown) => {
          setError(plainError(e, ROUTINE_COPY.saveFailed));
          setSaving(false);
        },
      );
  };

  return (
    <form className="ck-potato-routine-form" onSubmit={submit} aria-label="New routine" noValidate>
      <label className="ck-potato-routine-field" htmlFor={ids.title}>
        <span>Name</span>
        <input id={ids.title} type="text" value={values.title} placeholder="Morning sweep" maxLength={80} onChange={(e) => set('title', e.target.value)} />
      </label>
      <label className="ck-potato-routine-field" htmlFor={ids.prompt}>
        <span>What to do</span>
        <textarea id={ids.prompt} rows={3} value={values.prompt} placeholder="Check my inbox and list what needs a reply." onChange={(e) => set('prompt', e.target.value)} />
      </label>
      <label className="ck-potato-routine-field" htmlFor={ids.freq}>
        <span>How often</span>
        <select id={ids.freq} value={values.freq} onChange={(e) => set('freq', e.target.value as RoutineFreq)}>
          {FREQ_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      {values.freq === 'daily' || values.freq === 'weekdays' ? (
        <label className="ck-potato-routine-field" htmlFor={ids.time}>
          <span>Time</span>
          <input id={ids.time} type="time" value={values.time} onChange={(e) => set('time', e.target.value)} />
        </label>
      ) : null}
      {values.freq === 'every' ? (
        <label className="ck-potato-routine-field" htmlFor={ids.minutes}>
          <span>Every how many minutes (15 or more)</span>
          <input id={ids.minutes} type="number" min={15} max={1440} step={1} value={values.minutes} onChange={(e) => set('minutes', Number(e.target.value))} />
        </label>
      ) : null}
      {values.freq === 'once' ? (
        <label className="ck-potato-routine-field" htmlFor={ids.at}>
          <span>Date and time</span>
          <input id={ids.at} type="datetime-local" value={values.at} onChange={(e) => set('at', e.target.value)} />
        </label>
      ) : null}
      {error ? (
        <p role="alert" className="ck-potato-routine-error">
          {error}
        </p>
      ) : null}
      <div className="ck-potato-routine-actions">
        <button type="button" className="ck-potato-routine-btn" onClick={onCancel}>
          {ROUTINE_COPY.cancel}
        </button>
        <button type="submit" className="ck-potato-routine-btn ck-potato-routine-btn-primary" disabled={saving}>
          {saving ? ROUTINE_COPY.saving : ROUTINE_COPY.save}
        </button>
      </div>
    </form>
  );
}
