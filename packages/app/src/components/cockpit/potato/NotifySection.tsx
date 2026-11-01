// Notifications of one bot: "When it replies" or "Never". The choice is saved at once. If saving fails,
// the old choice stays and the sentence shows under the list. "When it needs me" is not offered: nothing sends that push yet.
import { useEffect, useState } from 'react';
import { plainError } from '../../../lib/potato';
import { NOTIFY_CHOICES, tauriNotifyTransport, type NotifyChoice, type NotifyTransport } from '../../../lib/potato-notify';
import { PanelSection } from './ui/PanelSection';

export const NOTIFY_COPY = {
  title: 'Notifications',
  hint: 'Phone pushes from this bot.',
  loadFailed: 'Could not read this setting.',
  saveFailed: 'Could not save that choice.',
};

export interface NotifySectionProps {
  botId: string;
  transport?: NotifyTransport;
}

export function NotifySection({ botId, transport = tauriNotifyTransport }: NotifySectionProps) {
  const [choice, setChoice] = useState<NotifyChoice | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setChoice(null);
    setError(null);
    transport
      .get(botId)
      .then((value) => live && setChoice(value))
      .catch((err) => live && setError(plainError(err, NOTIFY_COPY.loadFailed)));
    return () => {
      live = false;
    };
  }, [botId, transport]);

  const pick = async (next: NotifyChoice) => {
    if (busy || next === choice) return;
    setBusy(true);
    setError(null);
    try {
      setChoice(await transport.set(botId, next));
    } catch (err) {
      setError(plainError(err, NOTIFY_COPY.saveFailed));
    } finally {
      setBusy(false);
    }
  };

  return (
    <PanelSection id="ck-potato-notify" title={NOTIFY_COPY.title}>
      <p className="ck-sub ck-potato-psec-hint">{NOTIFY_COPY.hint}</p>
      <fieldset className="ck-potato-radios" disabled={choice === null || busy}>
        <legend className="ck-potato-sr">{NOTIFY_COPY.title}</legend>
        {NOTIFY_CHOICES.map((option) => (
          <label key={option.id} className="ck-potato-radio">
            <input
              type="radio"
              name={`notify-${botId}`}
              value={option.id}
              checked={choice === option.id}
              onChange={() => void pick(option.id)}
            />
            <span>{option.label}</span>
          </label>
        ))}
      </fieldset>
      {error && (
        <p className="ck-sub ck-potato-psec-error" role="alert">
          {error}
        </p>
      )}
    </PanelSection>
  );
}
