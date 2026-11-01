import { describe, expect, it } from 'vitest';
import type { Account } from '../../../lib/accounts';
import type { AgentDescriptor } from '../../../lib/agentRegistry';
import {
  EXAMPLES,
  addCard,
  agentChoices,
  canAdd,
  chatAvailable,
  cleanName,
  defaultChoice,
  defaultRunAs,
  exampleAt,
  greeting,
  MAX_CARDS,
  moveCard,
  nextExample,
  parseCards,
  removeCard,
  sendBlock,
  withPaths,
  type AgentChoice,
  type HomeCard,
  type LaneView,
} from './model';

const agent = (id: string, extra: Partial<AgentDescriptor> = {}): AgentDescriptor =>
  ({ id, label: id.toUpperCase(), kind: 'cli', installed: true, modes: { headless: true, visible: true }, ...extra }) as unknown as AgentDescriptor;

describe('greeting', () => {
  it('says the name the person saved', () => {
    expect(greeting('Sam')).toBe('What\u2019s up next, Sam?');
  });
  it('has no name by default: nothing is derived from anywhere', () => {
    expect(greeting('')).toBe('What\u2019s up next?');
    expect(greeting('   ')).toBe('What\u2019s up next?');
    expect(greeting(undefined)).toBe('What\u2019s up next?');
    expect(greeting(null)).toBe('What\u2019s up next?');
  });
  it('keeps the name to one short line', () => {
    expect(cleanName('  Sam \n  Lee ')).toBe('Sam Lee');
    expect(cleanName('x'.repeat(100))).toHaveLength(60);
  });
});

describe('examples', () => {
  it('moves to the next and wraps round', () => {
    expect(nextExample(0)).toBe(1);
    expect(nextExample(EXAMPLES.length - 1)).toBe(0);
    expect(nextExample(Number.NaN)).toBe(0);
  });
  it('reads any index safely', () => {
    expect(exampleAt(0)).toBe(EXAMPLES[0]);
    expect(exampleAt(EXAMPLES.length)).toBe(EXAMPLES[0]);
    expect(exampleAt(-1)).toBe(EXAMPLES[EXAMPLES.length - 1]);
  });
  it('names no agent', () => {
    for (const text of EXAMPLES) expect(text).not.toMatch(/claude|codex|cursor|gemini|kilo|opencode/i);
  });
});

describe('the agent list', () => {
  const login: Account = { id: 'a1', agent_id: 'alpha', label: 'Work', home: 'x' };
  it('lists agents with a state, a login row under its agent, and no shells', () => {
    const list = agentChoices(
      [
        agent('alpha'),
        agent('beta', { installed: false }),
        agent('gamma', { modes: { headless: true, visible: false } }),
        agent('sh', { kind: 'shell' }),
      ],
      [login],
    );
    expect(list.map((c) => [c.key, c.state])).toEqual([
      ['alpha', 'ready'],
      ['alpha:a1', 'ready'],
      ['beta', 'missing'],
      ['gamma', 'unavailable'],
    ]);
    expect(list[1].label).toBe('ALPHA \u00b7 Work');
    expect(list[2].hint).toBe('not installed');
  });
  it('starts on the remembered agent if it is ready, else the first ready one', () => {
    const list = agentChoices([agent('alpha', { installed: false }), agent('beta'), agent('gamma')], []);
    expect(defaultChoice(list, 'gamma')?.agentId).toBe('gamma');
    expect(defaultChoice(list, 'alpha')?.agentId).toBe('beta');
    expect(defaultChoice(list, null)?.agentId).toBe('beta');
    expect(defaultChoice([], null)).toBeNull();
  });
});

describe('run as', () => {
  const lanes = (labsEnabled: boolean, disabled = false, lane: 'terminal' | 'acp' = 'terminal'): LaneView => ({ labsEnabled, agents: [{ id: 'alpha', lane, disabled }] });
  it('offers Chat only when Labs has it on and the agent has a chat way to run', () => {
    expect(chatAvailable(lanes(true), 'alpha')).toBe(true);
    expect(chatAvailable(lanes(false), 'alpha')).toBe(false);
    expect(chatAvailable(lanes(true, true), 'alpha')).toBe(false);
    expect(chatAvailable(lanes(true), 'beta')).toBe(false);
    expect(chatAvailable(null, 'alpha')).toBe(false);
    expect(chatAvailable(lanes(true), null)).toBe(false);
  });
  it('starts on the way the agent is set to run', () => {
    expect(defaultRunAs(lanes(true, false, 'acp'), 'alpha')).toBe('chat');
    expect(defaultRunAs(lanes(true), 'alpha')).toBe('terminal');
    expect(defaultRunAs(lanes(false, false, 'acp'), 'alpha')).toBe('terminal');
  });
});

describe('sendBlock', () => {
  const ready: AgentChoice = { key: 'a', agentId: 'a', label: 'A', state: 'ready', hint: 'ready' };
  const ok = { prompt: 'do it', choice: ready, runAs: 'terminal' as const, folder: null, busy: false };
  it('lets a terminal start without a folder (the app default is used)', () => {
    expect(sendBlock(ok)).toBeNull();
  });
  it('says why not', () => {
    expect(sendBlock({ ...ok, busy: true })).toBe('Starting\u2026');
    expect(sendBlock({ ...ok, choice: null })).toMatch(/No agent is ready/);
    expect(sendBlock({ ...ok, choice: { ...ready, state: 'missing', hint: 'not installed' } })).toBe('A is not installed.');
    expect(sendBlock({ ...ok, runAs: 'chat' })).toMatch(/folder/);
    expect(sendBlock({ ...ok, runAs: 'chat', folder: '/p' })).toBeNull();
    expect(sendBlock({ ...ok, prompt: '  ' })).toMatch(/Write what/);
  });
});

describe('withPaths', () => {
  it('adds one path per line, quoting a path with a space', () => {
    expect(withPaths('', ['/a/b.txt'])).toBe('/a/b.txt\n');
    expect(withPaths('Fix this', ['/a/b.txt', 'C:\\My Docs\\c.md'])).toBe('Fix this\n/a/b.txt\n"C:\\My Docs\\c.md"\n');
    expect(withPaths('Fix', [])).toBe('Fix');
  });
});

describe('home cards', () => {
  const one: HomeCard[] = [{ id: 'a', kind: 'recent' }];
  it('reads what was saved and drops what is unreadable', () => {
    expect(parseCards(undefined)).toEqual([]);
    expect(parseCards('x')).toEqual([]);
    expect(
      parseCards([{ id: 'a', kind: 'recent' }, { id: 'a', kind: 'needs' }, { id: 'b', kind: 'nope' }, null, { id: 'c', kind: 'prompt', text: 'hi' }, { kind: 'running' }]),
    ).toEqual([
      { id: 'a', kind: 'recent' },
      { id: 'c', kind: 'prompt', text: 'hi' },
    ]);
  });
  it('lets a kind be pinned once, and a prompt many times', () => {
    expect(canAdd(one, 'recent')).toBe(false);
    expect(canAdd(one, 'running')).toBe(true);
    expect(canAdd(one, 'prompt')).toBe(true);
    expect(addCard(one, 'recent')).toEqual(one);
    expect(addCard(one, 'running', undefined, 'r').map((c) => c.id)).toEqual(['a', 'r']);
  });
  it('pins a prompt only with text', () => {
    expect(addCard([], 'prompt', '   ')).toEqual([]);
    expect(addCard([], 'prompt', ' Be brief ', 'p')).toEqual([{ id: 'p', kind: 'prompt', text: 'Be brief' }]);
  });
  it('stops at the limit', () => {
    const full = Array.from({ length: MAX_CARDS }, (_, i) => ({ id: `p${i}`, kind: 'prompt' as const, text: 't' }));
    expect(canAdd(full, 'prompt')).toBe(false);
  });
  it('removes and moves, and stays put at an end', () => {
    const three: HomeCard[] = [
      { id: 'a', kind: 'recent' },
      { id: 'b', kind: 'running' },
      { id: 'c', kind: 'needs' },
    ];
    expect(removeCard(three, 'b').map((c) => c.id)).toEqual(['a', 'c']);
    expect(moveCard(three, 'b', -1).map((c) => c.id)).toEqual(['b', 'a', 'c']);
    expect(moveCard(three, 'b', 1).map((c) => c.id)).toEqual(['a', 'c', 'b']);
    expect(moveCard(three, 'a', -1).map((c) => c.id)).toEqual(['a', 'b', 'c']);
    expect(moveCard(three, 'c', 1).map((c) => c.id)).toEqual(['a', 'b', 'c']);
    expect(moveCard(three, 'zzz', 1)).toEqual(three);
  });
});
