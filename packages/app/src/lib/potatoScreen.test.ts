import { describe, expect, it } from 'vitest';
import { fakeScreenTransport, FAKE_ORIGIN, headerWords, originOf, parseViewerMessage, viewerNote, type ScreenState } from './potatoScreen';

const state = (over: Partial<ScreenState> = {}): ScreenState => ({ enabled: true, phase: 'free', bot: null, cardId: null, reason: null, cardExpiresMs: null, ...over });

describe('parseViewerMessage', () => {
  it('takes the five statuses of the viewer and its five reasons', () => {
    for (const status of ['loading', 'connecting', 'connected', 'disconnected', 'failed']) {
      expect(parseViewerMessage({ type: 'pm-sbx-screen-status', status })).toEqual({ status, reason: null });
    }
    for (const reason of ['load', 'timeout', 'security', 'closed', 'error']) {
      expect(parseViewerMessage({ type: 'pm-sbx-screen-status', status: 'failed', reason })).toEqual({ status: 'failed', reason });
    }
  });

  it('refuses anything else', () => {
    for (const bad of [null, undefined, 'connected', 5, [], {}, { type: 'other', status: 'connected' }, { type: 'pm-sbx-screen-status' }, { type: 'pm-sbx-screen-status', status: 'ok' }, { type: 'pm-sbx-screen-status', status: 7 }]) {
      expect(parseViewerMessage(bad)).toBeNull();
    }
  });

  it('drops a reason it does not know, and keeps the status', () => {
    expect(parseViewerMessage({ type: 'pm-sbx-screen-status', status: 'failed', reason: '<script>' })).toEqual({ status: 'failed', reason: null });
  });
});

describe('originOf', () => {
  it('gives the origin of an http address and nothing for the rest', () => {
    expect(originOf('http://127.0.0.1:41999/bridge/x/embed.html?control=1#vnc=abc')).toBe('http://127.0.0.1:41999');
    expect(originOf('https://pm.example.test/bridge/x')).toBe('https://pm.example.test');
    expect(originOf('/bridge/x/embed.html', 'https://pm.example.test/phone')).toBe('https://pm.example.test');
    expect(originOf('javascript:alert(1)')).toBeNull();
    expect(originOf('data:text/html,hi')).toBeNull();
    expect(originOf('file:///x')).toBeNull();
  });
});

describe('words', () => {
  it('names who has the screen', () => {
    expect(headerWords(state({ phase: 'bot', bot: 'fish' }), 'fish', 'Fish')).toBe('Fish is using it');
    expect(headerWords(state({ phase: 'bot', bot: 'other' }), 'fish', 'Fish')).toBe('Another bot is using it');
    expect(headerWords(state({ phase: 'you' }), 'fish', 'Fish')).toBe('You have control');
    expect(headerWords(state({ phase: 'needs_you', bot: 'fish', cardId: 'needs-1' }), 'fish', 'Fish')).toBe('Needs you');
    expect(headerWords(state({ phase: 'needs_you', bot: 'other' }), 'fish', 'Fish')).toBe('Another bot needs you');
    expect(headerWords(state(), 'fish', 'Fish')).toBe('Not in use');
  });

  it('says plainly why the screen is not showing', () => {
    expect(viewerNote('loading', null)).toBe('Connecting…');
    expect(viewerNote('connecting', null)).toBe('Connecting…');
    expect(viewerNote('connected', null)).toBeNull();
    expect(viewerNote('disconnected', 'closed')).toBe('The connection closed.');
    expect(viewerNote('failed', 'timeout')).toBe('It took too long to connect.');
    expect(viewerNote('failed', 'security')).toBe('The screen did not accept the link.');
    expect(viewerNote('failed', 'load')).toBe('The screen viewer could not load.');
    expect(viewerNote('failed', 'error')).toBe('Could not connect to the screen.');
    expect(viewerNote('failed', null)).toBe('Could not connect to the screen.');
  });
});

describe('fakeScreenTransport', () => {
  it('moves like the app: take over, hand back, skip, and tells its listeners', async () => {
    const t = fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-1', reason: 'x' });
    let told = 0;
    await t.onChanged!(() => void told++);
    expect((await t.takeOver()).phase).toBe('you');
    expect((await t.handBack()).phase).toBe('free');
    t.set({ phase: 'needs_you', bot: 'fish', cardId: 'needs-2' });
    expect(told).toBe(1);
    expect((await t.skip('needs-2')).phase).toBe('bot');
    const link = await t.link('fish', 'control');
    expect(link.src.startsWith(FAKE_ORIGIN)).toBe(true);
    expect(link.src).toContain('?control=1#vnc=');
    t.failNext('link', 'SCREEN_NOT_RUNNING: The sandbox is not running yet.');
    await expect(t.link('fish', 'view')).rejects.toThrow('SCREEN_NOT_RUNNING');
    await expect(t.link('fish', 'view')).resolves.toBeTruthy();
  });
});

describe('parseViewerMessage title', () => {
  it('takes the window title as plain text, cut short, and leaves it out when there is none', () => {
    const base = { type: 'pm-sbx-screen-status', status: 'connected' };
    expect(parseViewerMessage({ ...base, title: '  Inbox - Chromium  ' })).toEqual({ status: 'connected', reason: null, title: 'Inbox - Chromium' });
    expect(parseViewerMessage({ ...base, title: 'a\u0000b\nc' })?.title).toBe('a b c');
    expect(parseViewerMessage({ ...base, title: 'x'.repeat(200) })?.title).toHaveLength(80);
    for (const title of [undefined, '', '   ', 5, null, {}]) expect(parseViewerMessage({ ...base, title })).toEqual({ status: 'connected', reason: null });
  });
});
