// The Advanced tab's "Read replies aloud" switch for this bot. It saves the bot's id in the Potato Bot voice settings
// (the list that the Voice page's settings also reads). The save reads the settings again first, so a change made on
// the Voice page is never lost. `onChanged` reads the page again, so the thread picks the switch up at once.
import { useState } from 'react';
import { plainError, type PotatoSettings, type PotatoTransport } from '../../../lib/potato';
import { normalizeVoice, speechSupported } from '../../../lib/potato-speech';
import { AdvancedSection } from './AdvancedSection';
import '../../../styles/cockpit-potato-voice.css';

export const READ_ALOUD_COPY = {
  title: 'Read replies aloud',
  about: 'Reads each new reply out loud while this page is open. It uses a voice on this PC, and nothing is sent anywhere.',
  noSpeech: 'This computer cannot read replies aloud.',
  failed: 'Could not save that.',
};

export interface ReadAloudSectionProps {
  bot: { id: string; name: string };
  settings?: PotatoSettings;
  transport?: PotatoTransport;
  onChanged?: () => void;
}

export function ReadAloudSection({ bot, settings, transport, onChanged }: ReadAloudSectionProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!speechSupported()) {
    return (
      <AdvancedSection title={READ_ALOUD_COPY.title} line={READ_ALOUD_COPY.noSpeech}>{null}</AdvancedSection>
    );
  }
  const on = normalizeVoice(settings?.voice).readAloudBots.includes(bot.id);

  const toggle = async (value: boolean) => {
    if (!transport) return;
    setBusy(true);
    setError(null);
    try {
      const fresh = await transport.settingsGet();
      const voice = normalizeVoice(fresh.voice);
      const readAloudBots = value ? [...voice.readAloudBots.filter((id) => id !== bot.id), bot.id] : voice.readAloudBots.filter((id) => id !== bot.id);
      await transport.settingsSet({ ...fresh, voice: { ...voice, readAloudBots } });
      onChanged?.();
    } catch (failure) {
      setError(plainError(failure, READ_ALOUD_COPY.failed));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AdvancedSection title={READ_ALOUD_COPY.title} line={READ_ALOUD_COPY.about}>
      <div className="ck-potato-adv-row ck-potato-readaloud-row">
        <span className="ck-potato-adv-switch-label">{bot.name}</span>
        <button
          type="button"
          role="switch"
          aria-checked={on}
          aria-label={`${READ_ALOUD_COPY.title} for ${bot.name}`}
          className={`ck-potato-switch${on ? ' is-on' : ''}`}
          disabled={!transport || !settings || busy}
          onClick={() => void toggle(!on)}
        >
          <span className="ck-potato-adv-sr">{on ? 'On' : 'Off'}</span>
        </button>
      </div>
      {error && (
        <p className="ck-potato-readaloud-error" role="alert">
          {error}
        </p>
      )}
    </AdvancedSection>
  );
}
