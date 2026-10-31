// @vitest-environment node
// The pure rules of the page: rows, open cards, which button the header offers, the open bot.
import { describe, expect, it } from 'vitest';
import { canWrite, openCardIds, pickOpenBot, runButton, threadRows, totalUnread } from './model';
import { bot, msg, note, you } from './test-helpers';

describe('threadRows', () => {
  it('makes a bubble for each reply segment and marks the first of a run', () => {
    const rows = threadRows([you('hi'), msg({ text: 'one' }), msg({ text: 'two' }), you('ok')]);
    expect(rows.map((r) => (r.type === 'bubble' ? `${r.side}:${r.first}` : r.type))).toEqual(['you:true', 'bot:true', 'bot:false', 'you:true']);
  });

  it('shows notes and helper results as note rows, and a note breaks a run', () => {
    const rows = threadRows([msg({ text: 'a' }), note('Now talking as Potato.'), msg({ text: 'b' })]);
    expect(rows.map((r) => r.type)).toEqual(['bubble', 'note', 'bubble']);
    expect(rows[2]).toMatchObject({ first: true });
  });

  it('keeps a helper card open until a result names its request', () => {
    const card = msg({ text: 'fix the build', kind: 'helper_card', requestId: 'r1' });
    expect(threadRows([card])[0]).toMatchObject({ type: 'card', open: true });
    const done = msg({ text: 'Helper finished in demo-a.', kind: 'helper_result', requestId: 'r1', from: { kind: 'note' } });
    const rows = threadRows([card, done]);
    expect(rows[0]).toMatchObject({ type: 'card', open: false });
    expect(rows[1].type).toBe('note');
    expect([...openCardIds([card, done])]).toEqual([]);
  });
});

describe('runButton and canWrite', () => {
  it('offers Stop while it runs, Start while it rests, and nothing while it waits on you', () => {
    expect(runButton('ready')).toBe('stop');
    expect(runButton('starting')).toBe('stop');
    expect(runButton('idle')).toBe('start');
    expect(runButton('stopped')).toBe('start');
    expect(runButton('needs_key')).toBeNull();
    expect(runButton('daily_limit')).toBeNull();
    expect(runButton('agents')).toBeNull();
  });

  it('lets the person write unless Potato Bot is off', () => {
    expect(canWrite('needs_key')).toBe(true);
    expect(canWrite('off')).toBe(false);
  });
});

describe('totalUnread and pickOpenBot', () => {
  it('adds up the unread replies', () => {
    expect(totalUnread([bot({ unread: 2 }), bot({ id: 'b', unread: 1 })])).toBe(3);
  });

  it('opens the remembered bot, else the first, else none', () => {
    const bots = [bot({ id: 'a' }), bot({ id: 'b' })];
    expect(pickOpenBot(bots, 'b')?.id).toBe('b');
    expect(pickOpenBot(bots, 'gone')?.id).toBe('a');
    expect(pickOpenBot([], 'a')).toBeNull();
  });
});
