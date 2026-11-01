// @vitest-environment jsdom
// Read aloud in the thread: the Speak button under a bot reply (Speak, then Stop), the bot's switch in the Advanced
// tab (saved, and the history never read), and the new replies that are read while the page is open and visible.
// The browser's speechSynthesis is a fake; nothing is spoken.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoMessage, type PotatoVoice } from '../../../lib/potato';
import { speakingMessageId, stopSpeaking, type SpeechUtteranceLike, type SpeechVoiceLike } from '../../../lib/potato-speech';
import { AdvancedTab } from './AdvancedTab';
import { MessageRow } from './MessageRow';
import { SpeakButton } from './SpeakButton';
import { threadRows } from './model';
import { useReadAloud } from './useReadAloud';
import { msg, you } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// ---- a fake speechSynthesis ----------------------------------------------------------------------------------------------

const VOICES: SpeechVoiceLike[] = [{ voiceURI: 'Microsoft Zira', name: 'Microsoft Zira', lang: 'en-US' }];

class FakeUtterance implements SpeechUtteranceLike {
  voice: SpeechVoiceLike | null = null;
  rate = 1;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}

function installSpeech() {
  const synth = {
    spoken: [] as FakeUtterance[],
    getVoices: vi.fn(() => VOICES),
    speak: vi.fn((u: FakeUtterance) => synth.spoken.push(u)),
    cancel: vi.fn(() => {
      synth.spoken.length = 0;
    }),
  };
  vi.stubGlobal('speechSynthesis', synth);
  vi.stubGlobal('SpeechSynthesisUtterance', FakeUtterance);
  return synth;
}

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  setVisibility('visible');
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  // Drop the module-level speaking state and the visibility override for the next test.
  stopSpeaking();
  delete (document as unknown as { visibilityState?: string }).visibilityState;
});

const reply = (text: string, over: Partial<PotatoMessage> = {}) => msg({ text, from: { kind: 'bot', name: 'Mochi' }, at: Date.now() + 1000, ...over });

// ---- the Speak button ------------------------------------------------------------------------------------------------------

describe('SpeakButton', () => {
  it('reads the cleaned reply with the chosen voice and speed, then shows Stop until it is stopped', async () => {
    const synth = installSpeech();
    const voice: PotatoVoice = { voiceUri: 'Microsoft Zira', ratePercent: 150, readAloudBots: [] };
    await act(async () => root.render(<SpeakButton id="r1" text="**Done**: see `a/b/store.rs` now." voice={voice} />));

    const speak = host.querySelector<HTMLButtonElement>('button.ck-potato-speak')!;
    expect(speak.textContent).toBe('Speak');
    await act(async () => speak.click());

    expect(synth.spoken.map((u) => u.text)).toEqual(['Done: see store.rs now.']);
    expect(synth.spoken[0].voice?.voiceURI).toBe('Microsoft Zira');
    expect(synth.spoken[0].rate).toBe(1.5);
    expect(speakingMessageId()).toBe('r1');
    expect(host.querySelector('button.ck-potato-speak')!.textContent).toBe('Stop');
    expect(host.querySelector('button.ck-potato-speak')!.getAttribute('aria-pressed')).toBe('true');

    await act(async () => host.querySelector<HTMLButtonElement>('button.ck-potato-speak')!.click());
    expect(synth.cancel).toHaveBeenCalled();
    expect(speakingMessageId()).toBeNull();
    expect(host.querySelector('button.ck-potato-speak')!.textContent).toBe('Speak');
  });

  it('a second reply Speak takes over from the first one', async () => {
    const synth = installSpeech();
    await act(async () => {
      root.render(
        <>
          <SpeakButton id="r1" text="The first reply is long enough." />
          <SpeakButton id="r2" text="The second reply is long enough." />
        </>,
      );
    });
    const [first, second] = [...host.querySelectorAll<HTMLButtonElement>('button.ck-potato-speak')];
    await act(async () => first.click());
    await act(async () => second.click());
    expect(speakingMessageId()).toBe('r2');
    expect(synth.spoken.map((u) => u.text)).toEqual(['The second reply is long enough.']);
  });

  it('is not shown without speech, or for a reply with nothing to say', async () => {
    await act(async () => root.render(<SpeakButton id="r1" text="A reply that is long enough." />));
    expect(host.querySelector('button')).toBeNull();

    installSpeech();
    await act(async () => root.render(<SpeakButton id="r2" text="🎉" />));
    expect(host.querySelector('button')).toBeNull();
  });

  it('shows on the bot bubbles of the thread and not on yours', async () => {
    installSpeech();
    const fake = fakePotatoTransport();
    const rows = threadRows([reply('Sure, I can help with that.'), you('Thanks!')]);
    await act(async () => {
      root.render(
        <ol>
          {rows.map((row) => (
            <MessageRow key={row.message.id} row={row} botId="mochi" botName="Mochi" agentLabel="its agent" transport={fake} onChanged={() => {}} now={Date.now()} />
          ))}
        </ol>,
      );
    });
    const lines = [...host.querySelectorAll('li.ck-potato-line')];
    expect(lines[0].querySelector('.ck-potato-speak')).not.toBeNull();
    expect(lines[1].querySelector('.ck-potato-speak')).toBeNull();
  });
});

// ---- read replies aloud -----------------------------------------------------------------------------------------------------

function Thread({ messages, voice }: { messages: PotatoMessage[]; voice?: PotatoVoice }) {
  useReadAloud('mochi', messages, voice);
  return null;
}

describe('read replies aloud', () => {
  const on: PotatoVoice = { voiceUri: '', ratePercent: 100, readAloudBots: ['mochi'] };

  it('reads a new reply of a bot that has the switch on, and never the history', async () => {
    const synth = installSpeech();
    const old = msg({ text: 'Old reply from earlier today.', from: { kind: 'bot' }, at: 1_000 });
    await act(async () => root.render(<Thread messages={[old]} voice={on} />));
    expect(synth.speak).not.toHaveBeenCalled();

    await act(async () => root.render(<Thread messages={[old, reply('A brand new reply for you.')]} voice={on} />));
    expect(synth.spoken.map((u) => u.text)).toEqual(['A brand new reply for you.']);
  });

  it('reads nothing while the switch is off for that bot, and does not read the backlog later', async () => {
    const synth = installSpeech();
    const late = reply('Arrived while the switch was off.');
    await act(async () => root.render(<Thread messages={[]} voice={{ ...on, readAloudBots: ['other'] }} />));
    await act(async () => root.render(<Thread messages={[late]} voice={{ ...on, readAloudBots: ['other'] }} />));
    expect(synth.speak).not.toHaveBeenCalled();
    // Switched on afterwards: the same reply is not read now.
    await act(async () => root.render(<Thread messages={[late]} voice={on} />));
    expect(synth.speak).not.toHaveBeenCalled();
  });

  it('skips a reply that arrives while the page is hidden', async () => {
    const synth = installSpeech();
    await act(async () => root.render(<Thread messages={[]} voice={on} />));
    setVisibility('hidden');
    await act(async () => root.render(<Thread messages={[reply('Arrived while hidden, long enough.')]} voice={on} />));
    expect(synth.speak).not.toHaveBeenCalled();
  });

  it('never reads the person\'s own messages or notes', async () => {
    const synth = installSpeech();
    await act(async () => root.render(<Thread messages={[]} voice={on} />));
    await act(async () =>
      root.render(<Thread messages={[you('My own words, long enough.'), msg({ text: 'A note line here.', from: { kind: 'note' }, kind: 'note', at: Date.now() + 1000 })]} voice={on} />),
    );
    expect(synth.speak).not.toHaveBeenCalled();
  });
});

// ---- the Advanced tab's switch ---------------------------------------------------------------------------------------------

describe('AdvancedTab read aloud switch', () => {
  let fake: FakePotatoTransport;
  beforeEach(() => {
    fake = fakePotatoTransport();
  });

  it('turns the switch on for this bot, keeps the other bots, and reads the page again', async () => {
    installSpeech();
    const onChanged = vi.fn();
    const settings = await fake.settingsGet();
    await fake.settingsSet({ ...settings, voice: { voiceUri: 'Microsoft Zira', ratePercent: 120, readAloudBots: ['nimbus'] } });
    const fresh = await fake.settingsGet();
    await act(async () =>
      root.render(<AdvancedTab bot={{ id: 'mochi', name: 'Mochi' }} settings={fresh} transport={fake} onChanged={onChanged} />),
    );

    const toggle = host.querySelector<HTMLButtonElement>('button[role="switch"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    await act(async () => toggle.click());

    const saved = await fake.settingsGet();
    expect(saved.voice).toEqual({ voiceUri: 'Microsoft Zira', ratePercent: 120, readAloudBots: ['nimbus', 'mochi'] });
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('turns it off again without touching the other bots', async () => {
    installSpeech();
    const settings = await fake.settingsGet();
    await fake.settingsSet({ ...settings, voice: { voiceUri: '', ratePercent: 100, readAloudBots: ['mochi', 'nimbus'] } });
    await act(async () => root.render(<AdvancedTab bot={{ id: 'mochi', name: 'Mochi' }} settings={await fake.settingsGet()} transport={fake} />));
    await act(async () => host.querySelector<HTMLButtonElement>('button[role="switch"]')!.click());
    expect((await fake.settingsGet()).voice?.readAloudBots).toEqual(['nimbus']);
  });

  it('is shown as off and disabled while the settings are still loading', async () => {
    installSpeech();
    await act(async () => root.render(<AdvancedTab bot={{ id: 'mochi', name: 'Mochi' }} />));
    const toggle = host.querySelector<HTMLButtonElement>('button[role="switch"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    expect(toggle.disabled).toBe(true);
  });

  it('says so when this computer cannot speak', async () => {
    vi.stubGlobal('speechSynthesis', undefined);
    await act(async () => root.render(<AdvancedTab bot={{ id: 'mochi', name: 'Mochi' }} settings={await fake.settingsGet()} transport={fake} />));
    expect(host.querySelector('button[role="switch"]')).toBeNull();
    expect(host.textContent).toContain('cannot read replies aloud');
  });
});
