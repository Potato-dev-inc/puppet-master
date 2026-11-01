// @vitest-environment jsdom
// Settings > Potato Bot > Voice: the voices this PC has, filtered by language; the pick and the speed are saved and
// keep the other settings; Test voice says the sample with the saved voice and saves nothing. speechSynthesis is a fake.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport } from '../../../lib/potato';
import { SAMPLE_TEXT, stopSpeaking, type SpeechSynthesisLike, type SpeechUtteranceLike, type SpeechVoiceLike } from '../../../lib/potato-speech';
import { blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto, type Mounted } from '../test-utils';
import { makePotatoVoiceRows } from './PotatoVoiceRows';

const VOICES: SpeechVoiceLike[] = [
  { voiceURI: 'Microsoft Zira', name: 'Microsoft Zira', lang: 'en-US' },
  { voiceURI: 'Microsoft Hazel', name: 'Microsoft Hazel', lang: 'en-GB' },
  { voiceURI: 'Microsoft Hortense', name: 'Microsoft Hortense', lang: 'fr-FR' },
];

class FakeUtterance implements SpeechUtteranceLike {
  voice: SpeechVoiceLike | null = null;
  rate = 1;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}

let spoken: FakeUtterance[];
let synth: SpeechSynthesisLike & { speak: ReturnType<typeof vi.fn>; cancel: ReturnType<typeof vi.fn> };
let fake: FakePotatoTransport;
let m: Mounted | null = null;

beforeEach(() => {
  spoken = [];
  synth = {
    getVoices: vi.fn(() => VOICES),
    speak: vi.fn((u: FakeUtterance) => spoken.push(u)),
    cancel: vi.fn(),
  };
  vi.stubGlobal('speechSynthesis', synth);
  vi.stubGlobal('SpeechSynthesisUtterance', FakeUtterance);
  fake = fakePotatoTransport();
});

afterEach(async () => {
  await m?.unmount();
  m = null;
  vi.unstubAllGlobals();
  stopSpeaking();
});

async function mount() {
  m = await mountRows(makePotatoVoiceRows(fake));
  await settle(6);
  return m;
}

const saves = () => fake.calls.filter((c) => c.method === 'settingsSet');
const voiceSelect = () => byLabel<HTMLSelectElement>(m!.host, 'Voice')!;
const languageSelect = () => byLabel<HTMLSelectElement>(m!.host, 'Voice language')!;
const optionTexts = (select: HTMLSelectElement) => [...select.options].map((o) => o.textContent);

describe('Settings > Potato Bot > Voice', () => {
  it('shows the three rows, in order, with the voice choices', async () => {
    await mount();
    expect([...m!.host.querySelectorAll('[data-row]')].map((el) => el.getAttribute('data-row'))).toEqual(['potato-voice', 'potato-voice-rate', 'potato-voice-test']);
    expect(hasText(m!.host, 'Test voice')).toBe(true);
  });

  it('lists the voices of the app language first, with the system default on top', async () => {
    await mount();
    expect(languageSelect().value).toBe('en');
    expect(optionTexts(voiceSelect())).toEqual(['System default', 'Microsoft Zira', 'Microsoft Hazel']);
    expect(optionTexts(languageSelect())).toEqual(['en', 'fr']);
  });

  it('switching the language lists that language\'s voices', async () => {
    await mount();
    await pickOption(languageSelect(), 'fr');
    await settle();
    expect(optionTexts(voiceSelect())).toEqual(['System default', 'Microsoft Hortense']);
  });

  it('saves the picked voice and keeps the other voice settings', async () => {
    await fake.settingsSet({ ...(await fake.settingsGet()), voice: { voiceUri: '', ratePercent: 130, readAloudBots: ['mochi'] } });
    await mount();
    await pickOption(voiceSelect(), 'Microsoft Hazel');
    await settle(4);
    const saved = saves().at(-1)!.args[0] as { voice: unknown };
    expect(saved.voice).toEqual({ voiceUri: 'Microsoft Hazel', ratePercent: 130, readAloudBots: ['mochi'] });
    expect(m!.env.markSaved).toHaveBeenCalled();
    expect(voiceSelect().value).toBe('Microsoft Hazel');
  });

  it('going back to the system default saves an empty voice', async () => {
    await fake.settingsSet({ ...(await fake.settingsGet()), voice: { voiceUri: 'Microsoft Zira', ratePercent: 100, readAloudBots: [] } });
    await mount();
    expect(voiceSelect().value).toBe('Microsoft Zira');
    await pickOption(voiceSelect(), '');
    await settle(4);
    expect((saves().at(-1)!.args[0] as { voice: { voiceUri: string } }).voice.voiceUri).toBe('');
  });

  it('shows the speed in percent and saves a typed value within 50 to 200', async () => {
    await mount();
    const field = byLabel<HTMLInputElement>(m!.host, 'Speed in percent')!;
    expect(field.value).toBe('100');
    await typeInto(field, '250');
    await blur(field);
    await settle(4);
    expect((saves().at(-1)!.args[0] as { voice: { ratePercent: number } }).voice.ratePercent).toBe(200);
    expect(field.value).toBe('200');
  });

  it('Test voice says the sample with the saved voice and speed, and saves nothing', async () => {
    await fake.settingsSet({ ...(await fake.settingsGet()), voice: { voiceUri: 'Microsoft Hazel', ratePercent: 75, readAloudBots: [] } });
    await mount();
    const before = saves().length;
    await click(buttonByText(m!.host, 'Test voice') ?? null);
    await settle(4);
    expect(synth.speak).toHaveBeenCalled();
    expect(spoken.map((u) => u.text).join(' ')).toContain(SAMPLE_TEXT);
    expect(spoken[0].voice?.voiceURI).toBe('Microsoft Hazel');
    expect(spoken[0].rate).toBe(0.75);
    expect(saves().length).toBe(before);
  });

  it('says so when this computer has no speech', async () => {
    vi.stubGlobal('speechSynthesis', undefined);
    await mount();
    expect(hasText(m!.host, 'This computer has no speech.')).toBe(true);
    await click(buttonByText(m!.host, 'Test voice') ?? null);
    await settle();
    expect(hasText(m!.host, 'cannot say the test sentence')).toBe(true);
  });

  it('says so when the computer has speech but no voices yet', async () => {
    synth.getVoices = vi.fn(() => []);
    await mount();
    expect(hasText(m!.host, 'no voices for speech yet')).toBe(true);
    expect(voiceSelect().disabled).toBe(true);
  });
});
