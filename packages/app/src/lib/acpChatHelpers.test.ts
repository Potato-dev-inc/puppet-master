// @vitest-environment node
// The pure rules of the chat workspace: settings chips, the slash menu's filter, the start fallback, hand-over asks.
import { describe, expect, it } from 'vitest';
import {
  chatCapable,
  chipsDisabled,
  chipValueName,
  commandHint,
  commandText,
  configChips,
  errorSentence,
  filterCommands,
  handOverAsk,
  HANDOVER_WORDS,
  laneChoiceFor,
  slashQuery,
  startFailure,
  type AcpChatState,
  type AcpCommand,
  type AcpConfigOption,
} from './acpChat';

const state = (over: Partial<AcpChatState> = {}): AcpChatState => ({
  lane: 'acp',
  agent: 'codex',
  label: 'Codex',
  phase: 'idle',
  status: 'idle',
  turnInFlight: false,
  stopping: false,
  canSteer: false,
  turn: 1,
  title: null,
  usage: null,
  permissions: [],
  closed: false,
  ...over,
});

const select = (id: string, over: Partial<AcpConfigOption> = {}): AcpConfigOption => ({
  id,
  name: id,
  type: 'select',
  currentValue: 'a',
  options: [
    { value: 'a', name: 'Alpha' },
    { value: 'b', name: 'Beta', description: 'second' },
  ],
  ...over,
});

describe('configChips', () => {
  it('has no chips without settings or without a state', () => {
    expect(configChips(null)).toEqual([]);
    expect(configChips(state())).toEqual([]);
    expect(configChips(state({ configOptions: [] }))).toEqual([]);
  });

  it('makes one chip per plain select, model first, then mode, then effort, the rest in the agent order', () => {
    const chips = configChips(
      state({
        configOptions: [
          select('x1', { category: 'other' }),
          select('effort', { category: 'thought_level', name: 'Effort' }),
          select('mode', { category: 'mode', name: 'Mode' }),
          select('model', { category: 'model', name: 'Model', currentValue: 'b' }),
          select('x2'),
        ],
      }),
    );
    expect(chips.map((c) => c.id)).toEqual(['model', 'mode', 'effort', 'x1', 'x2']);
    expect(chips[0]).toMatchObject({ label: 'Model', value: 'b', category: 'model' });
    expect(chips[0].choices[1]).toEqual({ value: 'b', name: 'Beta', description: 'second' });
    expect(chipValueName(chips[0])).toBe('Beta');
  });

  it('ignores a setting with grouped choices, another type, or fewer than two choices', () => {
    const chips = configChips(
      state({
        configOptions: [
          select('grouped', { options: [{ group: 'g', name: 'G', options: [{ value: 'a', name: 'A' }] }] }),
          select('toggle', { type: 'boolean' }),
          select('one', { options: [{ value: 'a', name: 'Alpha' }] }),
          select('bad', { options: ['a', 'b'] }),
          select('ok'),
        ],
      }),
    );
    expect(chips.map((c) => c.id)).toEqual(['ok']);
  });

  it('shows the raw value when the agent lists no choice by it', () => {
    const [chip] = configChips(state({ configOptions: [select('m', { currentValue: 'zzz' })] }));
    expect(chipValueName(chip)).toBe('zzz');
  });

  it('switches the chips off while a turn runs, when closed, when handed to a terminal, or unknown', () => {
    expect(chipsDisabled(state({ turnInFlight: true }))).toBe(true);
    expect(chipsDisabled(state({ closed: true }))).toBe(true);
    expect(chipsDisabled(state({ inTerminal: { paneId: 't1' } }))).toBe(true);
    expect(chipsDisabled(null)).toBe(true);
    expect(chipsDisabled(state())).toBe(false);
  });
});

const cmds: AcpCommand[] = [
  { name: 'compact', description: 'Shrink the conversation', hint: null },
  { name: '/review', description: 'Review the changes', hint: 'what to review' },
  { name: 'rename', description: 'Rename the session', hint: 'new name' },
];

describe('slash commands', () => {
  it('only applies to text that starts with a slash and has no space yet', () => {
    expect(slashQuery('/')).toBe('');
    expect(slashQuery('/re')).toBe('re');
    expect(slashQuery('/review ')).toBeNull();
    expect(slashQuery('/review it')).toBeNull();
    expect(slashQuery('hello /re')).toBeNull();
    expect(slashQuery('')).toBeNull();
    expect(slashQuery('/a\nb')).toBeNull();
  });

  it('filters by prefix, ignoring case and a slash in the agent name', () => {
    expect(filterCommands(cmds, '/').map((c) => c.name)).toEqual(['compact', '/review', 'rename']);
    expect(filterCommands(cmds, '/RE').map((c) => c.name)).toEqual(['/review', 'rename']);
    expect(filterCommands(cmds, '/rev').map((c) => c.name)).toEqual(['/review']);
    expect(filterCommands(cmds, '/zz')).toEqual([]);
    expect(filterCommands(cmds, 'plain text')).toEqual([]);
  });

  it('inserts "/name " and gives the hint once exactly a command and a space are typed', () => {
    expect(commandText(cmds[0])).toBe('/compact ');
    expect(commandText(cmds[1])).toBe('/review ');
    expect(commandHint(cmds, '/review ')).toBe('what to review');
    expect(commandHint(cmds, '/compact ')).toBeNull();
    expect(commandHint(cmds, '/review x')).toBeNull();
    expect(commandHint(cmds, '/nope ')).toBeNull();
  });
});

describe('errors and the start fallback', () => {
  it('drops the code in front of the sentence', () => {
    expect(errorSentence('BUSY: The agent is working.')).toBe('The agent is working.');
    expect(errorSentence(new Error('ACP_NO_ADAPTER: Not installed.'))).toBe('Not installed.');
    expect(errorSentence('plain words')).toBe('plain words');
  });

  it('offers the terminal only for an ACP_ error that says the terminal can run the agent', () => {
    const yes = startFailure('ACP_ADAPTER_MISSING: The chat could not start. The terminal can run this agent instead.');
    expect(yes.terminalFallback).toBe(true);
    expect(yes.sentence).toBe('The chat could not start. The terminal can run this agent instead.');
    expect(startFailure('ACP_LABS_OFF: Chat sessions are off.').terminalFallback).toBe(false);
    expect(startFailure('NOPE: The terminal can run this agent instead.').terminalFallback).toBe(false);
  });
});

describe('hand-over', () => {
  it('reads the ask of a "not yet" answer and nothing from a done one', () => {
    expect(handOverAsk({ ok: true, result: { conversation: 'c', session_id: 's', how: 'x' } })).toBeNull();
    expect(handOverAsk({ ok: false, ask: ' The agent is working in the chat ', reason: 'turn_running' })).toEqual({
      ask: 'The agent is working in the chat',
      reason: 'turn_running',
    });
    expect(handOverAsk({ ok: false, ask: '', reason: 'r' })?.ask).toBeTruthy();
  });

  it('has the two plain buttons', () => {
    expect(HANDOVER_WORDS.toTerminal.yes).toBe('Stop it and open in terminal');
    expect(HANDOVER_WORDS.toChat.yes).toBe('Stop the terminal and go back to chat');
  });
});

describe('the lane of a new session', () => {
  const agent = (over = {}) => ({ id: 'a', label: 'A', lane: 'terminal' as const, effective_lane: 'terminal' as const, disabled: false, disabled_note: null, ...over });

  it('defaults to what the agent really uses', () => {
    expect(laneChoiceFor(undefined)).toBe('terminal');
    expect(laneChoiceFor(agent())).toBe('terminal');
    expect(laneChoiceFor(agent({ lane: 'acp', effective_lane: 'acp' }))).toBe('acp');
    expect(laneChoiceFor(agent({ lane: 'acp', effective_lane: 'terminal' }))).toBe('terminal');
  });

  it('offers Chat only with Labs on, and only for agents that are not disabled', () => {
    expect(chatCapable(null)).toEqual([]);
    expect(chatCapable({ labs_enabled: false, agents: [agent()] })).toEqual([]);
    expect(chatCapable({ labs_enabled: true, agents: [agent(), agent({ id: 'b', disabled: true })] }).map((a) => a.id)).toEqual(['a']);
  });
});

describe('startFailure for a missing add-on', () => {
  it('points at Settings and still offers the terminal', () => {
    const r = startFailure('ACP_ADDON_MISSING: Chat needs a small add-on for Claude (about 40 MB). Install it first.');
    expect(r.sentence).toContain('Install it first.');
    expect(r.sentence).toContain('Settings, under Agents › Chat');
    expect(r.terminalFallback).toBe(true);
  });
});
