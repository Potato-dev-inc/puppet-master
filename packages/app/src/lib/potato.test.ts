// @vitest-environment node
// The words and the fake: the copy is exact, errors lose their code, and the fake behaves like the app.
import { describe, expect, it, vi } from 'vitest';
import { brainLabel, brainStateWord, COPY, dollars, errorSentence, fakePotatoTransport, formatWhen, moodLine, plainError, tauriPotatoTransport } from './potato';
import { bot } from '../components/cockpit/potato/test-helpers';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

describe('copy', () => {
  it('has the exact sentences of the spec', () => {
    expect(COPY.emptyList).toBe('No bots yet. Make one to start chatting.');
    expect(COPY.composer('Fish')).toBe('Message Fish');
    expect(COPY.typing('Fish')).toBe('Fish is typing…');
    expect(COPY.limit('Fish', 50)).toBe("Fish has used today's limit ($0.50). Raise it in Settings or wait until tomorrow.");
    expect(COPY.stopped('Fish')).toBe("Fish's brain stopped.");
    expect(COPY.noKey('Fish')).toBe('Add your DeepSeek key in Settings to talk with Fish.');
    expect(COPY.needsNode).toBe('Potato Bot needs Node.js 20 or newer.');
    expect(COPY.agentsOnly).toBe('Mood and personas need the DeepSeek or OpenCode Go brain.');
  });

  it('writes cents as dollars, the mood in words, and the brain labels', () => {
    expect(dollars(50)).toBe('$0.50');
    expect(dollars(1250)).toBe('$12.50');
    expect(moodLine({ mood: 'tsundere', intensity: 1, energy: 72, hunger: 0 })).toBe('Tsundere · energy 72');
    expect(brainLabel('agents')).toBe('Through my agents');
    expect(brainStateWord('ready')).toBe('Running');
  });

  it('has a plain sentence for every error code and none for an unknown one', () => {
    for (const code of ['PROVIDER_AUTH', 'PROVIDER_LIMIT', 'PROVIDER_DOWN', 'NETWORK', 'PERSONA_TOO_BIG', 'SURFACE_AUTH', 'INTERNAL']) {
      expect(errorSentence('Fish', code), code).toBeTruthy();
    }
    expect(errorSentence('Fish', 'WHO_KNOWS')).toBeNull();
    expect(errorSentence('Fish', null)).toBeNull();
  });

  it('shows the clock today and the date on another day', () => {
    const now = new Date(2026, 10, 1, 15, 0).getTime();
    expect(formatWhen(new Date(2026, 10, 1, 9, 5).getTime(), now)).toMatch(/9:05/);
    expect(formatWhen(new Date(2026, 9, 30, 9, 5).getTime(), now)).toBe('Oct 30');
  });
});

describe('plainError', () => {
  it('drops the code in front of the sentence', () => {
    expect(plainError(new Error('POTATO_RATE: That is too fast.'), 'x')).toBe('That is too fast.');
    expect(plainError('POTATO_OFF: Potato Bot is switched off.', 'x')).toBe('Potato Bot is switched off.');
  });

  it('keeps an ordinary sentence and falls back when there is none', () => {
    expect(plainError(new Error('Coming soon.'), 'x')).toBe('Coming soon.');
    expect(plainError(undefined, 'Could not send that.')).toBe('Could not send that.');
  });
});

describe('the real transport', () => {
  it('calls the helper, install and key commands by name, with a folder name only and no key', async () => {
    invoke.mockResolvedValue(undefined);
    await tauriPotatoTransport.helperFolders('fish');
    await tauriPotatoTransport.helperStart('fish', 'r1', 'demo-a');
    await tauriPotatoTransport.helperStart('fish', 'r2');
    await tauriPotatoTransport.helperDecline('fish', 'r1');
    await tauriPotatoTransport.installNode();
    await tauriPotatoTransport.checkKey();
    expect(invoke.mock.calls.map((c) => c[0])).toEqual(['potato_helper_folders', 'potato_helper_start', 'potato_helper_start', 'potato_helper_decline', 'potato_install_node', 'potato_check_key']);
    expect(invoke.mock.calls[1][1]).toEqual({ botId: 'fish', requestId: 'r1', folder: 'demo-a' });
    expect(invoke.mock.calls[2][1]).toEqual({ botId: 'fish', requestId: 'r2', folder: null });
  });
});

describe('fakePotatoTransport', () => {
  it('saves a message, records the call and tells the listeners', async () => {
    const fake = fakePotatoTransport({ bots: [bot()] });
    const heard: (string | null)[] = [];
    await fake.onChanged!((id) => heard.push(id));
    const out = await fake.send('fish', ' hello ');
    expect(out.delivered).toBe(true);
    expect((await fake.messages('fish')).map((m) => m.text)).toEqual(['hello']);
    expect(fake.calls.map((c) => c.method)).toContain('send');
    expect(heard).toEqual(['fish']);
  });

  it('adds the note row when the voice changes, and only then', async () => {
    const fake = fakePotatoTransport({ bots: [bot()] });
    await fake.setPersona('fish', 'fish');
    expect(await fake.messages('fish')).toEqual([]);
    await fake.setPersona('fish', 'potato');
    expect((await fake.messages('fish')).map((m) => m.text)).toEqual(['Now talking as Potato.']);
    expect((await fake.state()).bots[0].talkPersona).toBe('potato');
  });

  it('refuses an unknown bot like the app', async () => {
    const fake = fakePotatoTransport();
    await expect(fake.send('nobody', 'hi')).rejects.toThrow('No bot has that name.');
  });

  it('turns the Labs switch off and on', async () => {
    const fake = fakePotatoTransport();
    fake.setEnabled(false);
    expect((await fake.state()).enabled).toBe(false);
  });
});
