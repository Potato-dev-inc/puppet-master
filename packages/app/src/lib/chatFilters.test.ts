import { describe, expect, it } from 'vitest';
import { dropShortcut, hasChatFilters, mergeChatFilters, parseChatQuery, searchFilters, whenSince } from './chatFilters';

const known = { agents: ['codex', 'claude-code'], folders: ['demo-a', 'My Shop'] };
const DAY = 86_400_000;

describe('parseChatQuery', () => {
  it('takes agent, folder and who shortcuts out of the words and keeps the rest', () => {
    expect(parseChatQuery('deploy agent:codex in:demo-a from:me', known)).toEqual({
      text: 'deploy',
      typed: { agent: 'codex', folder: 'demo-a', who: 'you' },
    });
  });

  it('matches agent and folder names the index holds, in any case', () => {
    expect(parseChatQuery('AGENT:Claude-Code IN:my-shop', known).typed).toEqual({ agent: 'claude-code', folder: 'my-shop' });
    expect(parseChatQuery('in:"My Shop" login', known)).toEqual({ text: 'login', typed: { folder: 'My Shop' } });
  });

  it('reads who words: me and you are the person, agent, tool and tools', () => {
    const who = (word: string) => parseChatQuery(`from:${word}`, known).typed.who;
    expect([who('me'), who('You'), who('agent'), who('tool'), who('tools')]).toEqual(['you', 'you', 'agent', 'tool', 'tool']);
  });

  it('keeps a shortcut it cannot read as plain words, and an empty one too', () => {
    expect(parseChatQuery('from:robot agent: login', known)).toEqual({ text: 'from:robot agent: login', typed: {} });
  });

  it('keeps a colon inside a word and a shortcut inside a word', () => {
    expect(parseChatQuery('10:30 sprint', known)).toEqual({ text: '10:30 sprint', typed: {} });
    expect(parseChatQuery('magent:codex', known).typed).toEqual({});
  });

  it('leaves no text when only shortcuts were typed (the newest items that pass are listed)', () => {
    expect(parseChatQuery('agent:codex', known)).toEqual({ text: '', typed: { agent: 'codex' } });
  });

  it('keeps a name it does not know as typed, so the search just finds nothing for it', () => {
    expect(parseChatQuery('agent:gemini', known).typed).toEqual({ agent: 'gemini' });
  });
});

describe('merging chips with typed shortcuts', () => {
  it('a typed shortcut wins over a chip for the same filter; the rest of the chips stay', () => {
    expect(mergeChatFilters({ agent: 'claude-code', when: '7d' }, { agent: 'codex' })).toEqual({ agent: 'codex', when: '7d' });
  });

  it('a shortcut that was not typed does not wipe a chip', () => {
    expect(mergeChatFilters({ folder: 'demo-a' }, {})).toEqual({ folder: 'demo-a' });
  });

  it('dropShortcut removes the shortcut for one filter only', () => {
    expect(dropShortcut('deploy agent:codex in:demo-a', 'agent')).toBe('deploy in:demo-a');
    expect(dropShortcut('from:me login', 'who')).toBe('login');
    expect(dropShortcut('in:"My Shop" login', 'folder')).toBe('login');
    expect(dropShortcut('deploy', 'when')).toBe('deploy');
  });

  it('knows whether any filter is set', () => {
    expect(hasChatFilters({})).toBe(false);
    expect(hasChatFilters({ when: '30d' })).toBe(true);
  });
});

describe('the filters the backend gets', () => {
  const now = new Date(2026, 9, 28, 15, 30).getTime();

  it('turns the When chip into a start time: today is local midnight, 7 and 30 days are days back', () => {
    expect(whenSince(undefined, now)).toBeUndefined();
    expect(whenSince('today', now)).toBe(new Date(2026, 9, 28).getTime());
    expect(whenSince('7d', now)).toBe(now - 7 * DAY);
    expect(whenSince('30d', now)).toBe(now - 30 * DAY);
  });

  it('sends only the filters that are set, with names the backend knows', () => {
    expect(searchFilters({}, now)).toEqual({});
    expect(searchFilters({ agent: 'codex', folder: 'demo-a', when: 'today', who: 'tool' }, now)).toEqual({
      agent: 'codex',
      folder: 'demo-a',
      sinceMs: new Date(2026, 9, 28).getTime(),
      role: 'tool',
    });
  });
});
