// Settings > Potato Bot > Voice: the system voice a reply is read with (filtered by language), the speed, and
// "Test voice". The voices are the ones this PC has (speechSynthesis): no key, and nothing leaves the PC. Cloud
// voices (ElevenLabs, OpenAI and the rest) are not here yet; they need a key and a server, a later phase. Which bots
// read aloud is chosen per bot, in each bot's Advanced tab.
import { useEffect, useMemo, useState } from 'react';
import { plainError, tauriPotatoTransport, type PotatoTransport, type PotatoVoice } from '../../../lib/potato';
import {
  DEFAULT_RATE,
  SAMPLE_TEXT,
  appLanguage,
  languageOf,
  languagesOf,
  listVoices,
  normalizeVoice,
  onVoicesChanged,
  pickVoice,
  speakReply,
  speechSupported,
  voicesInLanguage,
  type SpeechVoiceLike,
} from '../../../lib/potato-speech';
import type { RowDef, SettingsEnv } from '../model';
import { Btn, FieldLabel, FieldSelect, Note, SettingBlock, SettingRow } from '../settings-ui';
import { NumberControl } from '../controls';

export const VOICE_COPY = {
  noSpeech: 'This computer has no speech. Add a voice in Windows settings, then come back.',
  noVoices: 'This computer has no voices for speech yet.',
  systemDefault: 'System default',
  failed: 'Could not save that.',
  testFailed: 'This computer cannot say the test sentence.',
};

/** The voice choice: a language filter, then the voice itself. Saved as soon as it is picked. */
function VoiceRow({ transport, onSaved, setError }: { transport: PotatoTransport; onSaved: () => void; setError: (m: string | null) => void }) {
  const [saved, setSaved] = useState<PotatoVoice | null>(null);
  const [voices, setVoices] = useState<SpeechVoiceLike[]>(() => listVoices());
  const [language, setLanguage] = useState<string>(() => appLanguage());

  useEffect(() => {
    let live = true;
    transport
      .settingsGet()
      .then((found) => {
        if (live) setSaved(normalizeVoice(found.voice));
      })
      .catch((error) => {
        if (live) setError(plainError(error, 'Could not read the voice settings.'));
      });
    return () => {
      live = false;
    };
  }, [transport, setError]);

  // Chrome and WebView2 fill in the voice list late: read it again when it changes.
  useEffect(() => onVoicesChanged(() => setVoices(listVoices())), []);

  // The language follows the saved voice the first time it is known, so the picked voice is in the list.
  useEffect(() => {
    const found = saved && pickVoice(voices, saved.voiceUri);
    if (found) setLanguage(languageOf(found));
  }, [saved, voices]);

  const languages = useMemo(() => {
    const all = languagesOf(voices);
    return all.includes(language) || !language ? all : [language, ...all];
  }, [voices, language]);
  const choices = voicesInLanguage(voices, language);
  const current = saved ? pickVoice(voices, saved.voiceUri) : null;
  const value = current ? current.voiceURI : '';

  const pick = (voiceUri: string) => {
    setError(null);
    transport
      .settingsGet()
      .then(async (fresh) => {
        const voice = normalizeVoice(fresh.voice);
        const out = await transport.settingsSet({ ...fresh, voice: { ...voice, voiceUri } });
        setSaved(normalizeVoice(out.voice));
        onSaved();
      })
      .catch((error) => setError(plainError(error, VOICE_COPY.failed)));
  };

  return (
    <SettingBlock label="Voice" description="The voice that reads replies. Only the voices on this PC are listed.">
      {!speechSupported() ? (
        <Note>{VOICE_COPY.noSpeech}</Note>
      ) : (
        <>
          <FieldLabel>
            Language
            <FieldSelect aria-label="Voice language" value={language} disabled={!saved || voices.length === 0} onChange={(e) => setLanguage(e.target.value)}>
              {languages.map((code) => (
                <option key={code} value={code}>
                  {code}
                </option>
              ))}
            </FieldSelect>
          </FieldLabel>
          <FieldLabel>
            Voice
            <FieldSelect aria-label="Voice" value={value} disabled={!saved || voices.length === 0} onChange={(e) => pick(e.target.value)}>
              <option value="">{VOICE_COPY.systemDefault}</option>
              {choices.map((v) => (
                <option key={v.voiceURI} value={v.voiceURI}>
                  {v.name}
                </option>
              ))}
            </FieldSelect>
          </FieldLabel>
          {voices.length === 0 && <Note>{VOICE_COPY.noVoices}</Note>}
        </>
      )}
    </SettingBlock>
  );
}

/** The speed, in percent of normal. Saved when it is committed, between 50 and 200. */
function RateRow({ transport, onSaved, setError }: { transport: PotatoTransport; onSaved: () => void; setError: (m: string | null) => void }) {
  const [saved, setSaved] = useState<PotatoVoice | null>(null);
  useEffect(() => {
    let live = true;
    transport.settingsGet().then((found) => {
      if (live) setSaved(normalizeVoice(found.voice));
    }, () => {});
    return () => {
      live = false;
    };
  }, [transport]);
  return (
    <SettingRow label="Speed" description="How fast replies are read. 100% is normal speed.">
      <NumberControl
        aria-label="Speed in percent"
        value={saved?.ratePercent ?? DEFAULT_RATE}
        min={50}
        max={200}
        step={5}
        suffix="%"
        disabled={!saved}
        onCommit={(next) => {
          setError(null);
          transport
            .settingsGet()
            .then(async (fresh) => {
              const voice = normalizeVoice(fresh.voice);
              const out = await transport.settingsSet({ ...fresh, voice: { ...voice, ratePercent: next } });
              setSaved(normalizeVoice(out.voice));
              onSaved();
            })
            .catch((error) => setError(plainError(error, VOICE_COPY.failed)));
        }}
      />
    </SettingRow>
  );
}

/** Says one short sentence with the saved voice and speed. Nothing is saved by it. */
function TestRow({ transport }: { transport: PotatoTransport }) {
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const test = async () => {
    setResult(null);
    if (!speechSupported()) {
      setResult({ ok: false, text: VOICE_COPY.testFailed });
      return;
    }
    try {
      const voice = normalizeVoice((await transport.settingsGet()).voice);
      const said = speakReply('potato-voice-test', SAMPLE_TEXT, { voiceUri: voice.voiceUri, ratePercent: voice.ratePercent }, true);
      setResult(said ? null : { ok: false, text: VOICE_COPY.testFailed });
    } catch (error) {
      setResult({ ok: false, text: plainError(error, VOICE_COPY.testFailed) });
    }
  };
  return (
    <SettingBlock label="Test voice" description="Says one short sentence with the voice and speed above.">
      <Btn onClick={() => void test()}>Test voice</Btn>
      {result && <Note role={result.ok ? 'status' : 'alert'}>{result.text}</Note>}
    </SettingBlock>
  );
}

/** The three voice rows, in Settings > Potato Bot. They read and save the voice settings through the transport. */
export function makePotatoVoiceRows(transport: PotatoTransport = tauriPotatoTransport): RowDef[] {
  const KEYWORDS = 'potato voice speak speech read aloud tts language rate speed test sound';
  return [
    {
      id: 'potato-voice',
      label: 'Voice',
      hint: 'The voice that reads replies out loud. Only the voices on this PC are listed.',
      keywords: `${KEYWORDS} language voice pick`,
      group: 'Voice',
      render: (env: SettingsEnv) => <VoiceRow transport={transport} onSaved={env.markSaved} setError={env.setError} />,
    },
    {
      id: 'potato-voice-rate',
      label: 'Speed',
      hint: 'How fast replies are read. From 50% to 200%.',
      keywords: `${KEYWORDS} faster slower`,
      group: 'Voice',
      render: (env: SettingsEnv) => <RateRow transport={transport} onSaved={env.markSaved} setError={env.setError} />,
    },
    {
      id: 'potato-voice-test',
      label: 'Test voice',
      hint: 'Says one short sentence with the voice above.',
      keywords: `${KEYWORDS} sample hear`,
      group: 'Voice',
      render: () => <TestRow transport={transport} />,
    },
  ];
}
