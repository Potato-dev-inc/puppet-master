// @vitest-environment node
// The reply cleanup, the voices, and the queue. The browser's speechSynthesis is a fake: nothing is spoken here.
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  appLanguage,
  clampRate,
  languageOf,
  languagesOf,
  listVoices,
  normalizeVoice,
  pickVoice,
  speakable,
  speakingMessageId,
  speakReply,
  speechSupported,
  stopSpeaking,
  subscribeSpeaking,
  toUtterances,
  voicesInLanguage,
  type SpeechVoiceLike,
  type SpeechUtteranceLike,
} from './potato-speech';

// ---- a fake speechSynthesis ----------------------------------------------------------------------------------------------

const VOICES: SpeechVoiceLike[] = [
  { voiceURI: 'Microsoft David', name: 'Microsoft David', lang: 'en-US' },
  { voiceURI: 'Microsoft Zira', name: 'Microsoft Zira', lang: 'en-GB' },
  { voiceURI: 'Microsoft Hortense', name: 'Microsoft Hortense', lang: 'fr-FR' },
];

class FakeUtterance implements SpeechUtteranceLike {
  voice: SpeechVoiceLike | null = null;
  rate = 1;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}

function fakeSynth(voices: SpeechVoiceLike[] = VOICES) {
  const queue: FakeUtterance[] = [];
  const synth = {
    spoken: queue,
    getVoices: vi.fn(() => voices),
    speak: vi.fn((u: FakeUtterance) => queue.push(u)),
    cancel: vi.fn(() => queue.splice(0)),
  };
  return synth;
}

function install(synth: ReturnType<typeof fakeSynth> | null) {
  if (synth) {
    vi.stubGlobal('speechSynthesis', synth);
    vi.stubGlobal('SpeechSynthesisUtterance', FakeUtterance);
  } else {
    vi.stubGlobal('speechSynthesis', undefined);
    vi.stubGlobal('SpeechSynthesisUtterance', undefined);
  }
}

afterEach(() => {
  stopSpeaking();
  vi.unstubAllGlobals();
});

// ---- speakable: what the voice says ----------------------------------------------------------------------------------------

describe('speakable', () => {
  it('reads the prose and drops the markdown syntax', () => {
    expect(speakable('**Bold** and _soft_ and ~~gone~~ words.')).toBe('Bold and soft and gone words.');
    expect(speakable('- one\n- two')).toBe('one. two');
    expect(speakable('1. first\n2. second')).toBe('first. second');
    expect(speakable('> quoted line')).toBe('quoted line');
    expect(speakable('- [x] done thing')).toBe('done thing');
  });

  it('names a code block instead of reading it, and says the language when it knows it', () => {
    const spoken = speakable('Run this:\n```ts\nconst a = 1;\n```\nThen stop.');
    expect(spoken).toContain('(a TypeScript code block)');
    expect(spoken).not.toContain('const');
    expect(spoken).toMatch(/Then stop\.$/);
    expect(speakable('```\nplain\n```')).toBe('. (a code block)');
    expect(speakable('```weird\nx\n```')).toBe('. (a code block)');
  });

  it('keeps the words of a link and says a bare link is a link', () => {
    expect(speakable('See [the docs](https://example.com/docs) now.')).toBe('See the docs now.');
    expect(speakable('Go to https://example.com/a?b=1 today.')).toBe('Go to a link today.');
    expect(speakable('Mail <https://example.com> please')).toBe('Mail a link please');
  });

  it('describes an image by its alt text', () => {
    expect(speakable('![a cat](cat.png)')).toBe('. (image: a cat)');
    expect(speakable('![](cat.png)')).toBe('. (an image)');
  });

  it('reads a table row by row, cells separated by commas', () => {
    expect(speakable('| name | size |\n|---|---|\n| cat | big |')).toBe('name, size. cat, big');
  });

  it('keeps a short inline code span and replaces a long one', () => {
    expect(speakable('Run `npm ci` first.')).toBe('Run npm ci first.');
    expect(speakable(`Use \`${'x'.repeat(50)}\` here.`)).toBe('Use that snippet here.');
  });

  it('gives a heading a full stop unless it already ends a sentence', () => {
    expect(speakable('# Results')).toBe('Results.');
    expect(speakable('## Done!')).toBe('Done!');
  });

  it('keeps only the file name of a path', () => {
    expect(speakable('Edited src/lib/potato/store.rs today.')).toBe('Edited store.rs today.');
  });

  it('drops emoji and pictograms', () => {
    expect(speakable('Great job 🎉 ✅ team')).toBe('Great job team');
  });

  it('turns line breaks into pauses and collapses spaces', () => {
    expect(speakable('First line\n\nSecond   line')).toBe('First line. Second line');
    expect(speakable('Hello .  World ,ok')).toBe('Hello. World,ok');
  });

  it('returns nothing for empty input or input with no letters or digits', () => {
    expect(speakable('')).toBe('');
    expect(speakable('---')).toBe('');
    expect(speakable('🎉 🎉')).toBe('');
  });
});

// ---- toUtterances: pieces a synthesizer can start on -----------------------------------------------------------------------

describe('toUtterances', () => {
  it('splits at sentence ends and keeps abbreviations together', () => {
    expect(toUtterances('This is the first sentence. Here comes the second one! And a third, e.g. this one.')).toEqual([
      'This is the first sentence.',
      'Here comes the second one!',
      'And a third, e.g. this one.',
    ]);
  });

  it('glues a short fragment onto its neighbour so the voice keeps its context', () => {
    expect(toUtterances('Yes. This sentence is long enough to stand alone.')).toEqual(['Yes. This sentence is long enough to stand alone.']);
  });

  it('cuts a long run at a comma or a space when it passes the limit', () => {
    const words = Array.from({ length: 60 }, (_, n) => `word${n}`).join(' ');
    const parts = toUtterances(words, { maxChars: 100 });
    expect(parts.length).toBeGreaterThan(1);
    for (const part of parts) expect(part.length).toBeLessThanOrEqual(100);
    expect(parts.join(' ')).toBe(words);
  });

  it('never splits a surrogate pair', () => {
    // U+1D400 is outside the pictograph ranges speakable() drops, so it survives into the split.
    const text = `${'a'.repeat(9)}\u{1D400}${'b'.repeat(9)}`;
    const parts = toUtterances(text, { maxChars: 10, minChars: 1 });
    for (const part of parts) {
      for (let i = 0; i < part.length; i += 1) {
        const code = part.charCodeAt(i);
        if (code >= 0xd800 && code <= 0xdbff) expect(part.charCodeAt(i + 1)).toBeGreaterThanOrEqual(0xdc00);
      }
    }
  });

  it('returns nothing for silent input and refuses a limit below two', () => {
    expect(toUtterances('')).toEqual([]);
    expect(toUtterances('🎉')).toEqual([]);
    expect(() => toUtterances('hi', { maxChars: 1 })).toThrow(RangeError);
    expect(() => toUtterances('hi', { maxChars: 2.5 })).toThrow(RangeError);
  });
});

// ---- voices and settings ---------------------------------------------------------------------------------------------------

describe('voices and settings', () => {
  it('says whether speech exists and lists the voices it has', () => {
    install(fakeSynth());
    expect(speechSupported()).toBe(true);
    expect(listVoices()).toHaveLength(3);
  });

  it('has no speech and no voices when the webview lacks them', () => {
    install(null);
    expect(speechSupported()).toBe(false);
    expect(listVoices()).toEqual([]);
    expect(speakReply('m1', 'Hello there friend.', { voiceUri: '', ratePercent: 100 }, true)).toBe(false);
  });

  it('reads the language of a voice tag and lists the languages once each', () => {
    expect(languageOf({ lang: 'en-US' })).toBe('en');
    expect(languageOf({ lang: 'pt_BR' })).toBe('pt');
    expect(languageOf({ lang: '' })).toBe('');
    expect(languagesOf(VOICES)).toEqual(['en', 'fr']);
    expect(voicesInLanguage(VOICES, 'EN').map((v) => v.voiceURI)).toEqual(['Microsoft David', 'Microsoft Zira']);
  });

  it('takes the app language from the browser tag, defaulting to English', () => {
    expect(appLanguage('fr-CA')).toBe('fr');
    expect(appLanguage('')).toBe('en');
  });

  it('finds a voice by its URI, or gives the system default when the URI is empty or gone', () => {
    expect(pickVoice(VOICES, 'Microsoft Zira')?.name).toBe('Microsoft Zira');
    expect(pickVoice(VOICES, '')).toBeNull();
    expect(pickVoice(VOICES, 'Uninstalled voice')).toBeNull();
  });

  it('keeps the speed between half and double, to whole percents', () => {
    expect(clampRate(0)).toBe(50);
    expect(clampRate(999)).toBe(200);
    expect(clampRate(120.4)).toBe(120);
    expect(clampRate(Number.NaN)).toBe(100);
  });

  it('fills in every missing voice setting', () => {
    expect(normalizeVoice(undefined)).toEqual({ voiceUri: '', ratePercent: 100, readAloudBots: [] });
    expect(normalizeVoice({ ratePercent: 400, readAloudBots: ['mochi'] })).toEqual({ voiceUri: '', ratePercent: 200, readAloudBots: ['mochi'] });
  });
});

// ---- the queue --------------------------------------------------------------------------------------------------------------

describe('speakReply', () => {
  it('queues every piece with the chosen voice and the speed, and marks the message as being read', () => {
    const synth = fakeSynth();
    install(synth);
    const seen: boolean[] = [];
    const off = subscribeSpeaking(() => seen.push(speakingMessageId() === 'm1'));

    const ok = speakReply('m1', 'First sentence is here. Second sentence is here too.', { voiceUri: 'Microsoft Zira', ratePercent: 150 }, true);

    expect(ok).toBe(true);
    expect(synth.spoken.map((u) => u.text)).toEqual(['First sentence is here.', 'Second sentence is here too.']);
    for (const u of synth.spoken) {
      expect(u.voice?.voiceURI).toBe('Microsoft Zira');
      expect(u.rate).toBe(1.5);
    }
    expect(speakingMessageId()).toBe('m1');
    expect(seen.at(-1)).toBe(true);
    off();
  });

  it('uses the system voice when the chosen one is not installed', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'A sentence that is long enough.', { voiceUri: 'Gone', ratePercent: 100 }, true);
    expect(synth.spoken[0].voice).toBeNull();
  });

  it('says nothing for a reply with no words in it', () => {
    const synth = fakeSynth();
    install(synth);
    expect(speakReply('m1', '🎉', { voiceUri: '', ratePercent: 100 }, true)).toBe(false);
    expect(synth.speak).not.toHaveBeenCalled();
    expect(speakingMessageId()).toBeNull();
  });

  it('clears the mark when the last piece ends', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'First sentence is here. Second sentence is here too.', { voiceUri: '', ratePercent: 100 }, true);
    synth.spoken[0].onend?.();
    expect(speakingMessageId()).toBe('m1');
    synth.spoken[1].onend?.();
    expect(speakingMessageId()).toBeNull();
  });

  it('clears the mark on an error too', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'A sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, true);
    synth.spoken[0].onerror?.();
    expect(speakingMessageId()).toBeNull();
  });

  it('Speak replaces what is being read; a new reply without replace waits in the queue', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'A sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, true);
    synth.cancel.mockClear();
    speakReply('m2', 'Another sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, false);
    expect(synth.cancel).not.toHaveBeenCalled();
    expect(synth.spoken).toHaveLength(2);

    speakReply('m3', 'A third sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, true);
    expect(synth.cancel).toHaveBeenCalledTimes(1);
    expect(speakingMessageId()).toBe('m3');
  });

  it('a late end from a stopped batch does not clear the next message', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'A sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, true);
    const old = synth.spoken[0];
    speakReply('m2', 'Another sentence that is long enough.', { voiceUri: '', ratePercent: 100 }, true);
    old.onend?.();
    expect(speakingMessageId()).toBe('m2');
  });

  it('Stop cancels the queue, clears the mark, and tells the listeners once', () => {
    const synth = fakeSynth();
    install(synth);
    speakReply('m1', 'First sentence is here. Second sentence is here too.', { voiceUri: '', ratePercent: 100 }, true);
    const listener = vi.fn();
    const off = subscribeSpeaking(listener);

    stopSpeaking();

    expect(synth.cancel).toHaveBeenCalled();
    expect(speakingMessageId()).toBeNull();
    expect(listener).toHaveBeenCalledTimes(1);
    stopSpeaking();
    expect(listener).toHaveBeenCalledTimes(1);
    off();
  });
});
