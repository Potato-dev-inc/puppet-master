import { describe, expect, it, vi } from 'vitest';
import {
  askOpenRouter,
  chooseDefaultDrafter,
  commandLine,
  extractTomlBlock,
  factsFrom,
  MAX_ROUNDS,
  orderDrafters,
  promptBudget,
  provenanceOf,
  runDraftLoop,
  samplesFrom,
  splitError,
  type LiveResult,
  type LoopOptions,
  type ProbeInfo,
  type ProgressEvent,
  type StartInfo,
  type VerifyResult,
} from './agentWizard';
import { LOCAL_PROMPT_BUDGET, PROMPT_VERSION, REMOTE_PROMPT_BUDGET } from './agentWizardPrompt';

const F = '`'.repeat(3);
const block = (toml: string) => `Here is the file.\n${F}toml\n${toml}\n${F}\nDone.`;

// Recorded replies of a stand-in drafter. GOOD is what a careful drafter writes for the fake agent;
// BROKEN has the mistake the checker finds in round 1.
const GOOD = 'schema_version = 1\n# GOOD\n[agent]\nid = "fake_agent"\n';
const BROKEN = 'schema_version = 1\n# BROKEN\n[agent]\nid = "fake_agent"\n';
const WRONG_FIELD = 'schema_version = 1\n# WRONG_FIELD\n[agent]\nid = "fake_agent"\n';

const CHECKER_TEXT =
  'The checker found 1 problem. Fix every one and reply with the complete corrected file in one ```toml block.\n\n1. [validate] AGENT_DEF_INCONSISTENT at `headless.output.result.path`\n   Problem: this selector needs a `path`.\n   Fix: add `path = "..."`.\n';
const LIVE_TEXT =
  'The live test ran your file against the real program and found 1 problem. Fix every one and reply with the complete corrected file in one ```toml block.\n\n1. [livetest] LIVETEST_PONG_FAILED at `pong`\n   Problem: no result was found in the output; expected "pong".\n';

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'fake-agent',
  id_suggestion: 'fake_agent',
  program: { path: 'C:\\x\\fake-agent.exe', display: '~\\fake-agent.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'fake-agent 1.4.2', number: '1.4.2' },
  native_candidates: [],
  cost_note: 'The live test runs fake-agent 3 times.',
  limits: {},
};

const probe: ProbeInfo = {
  state: 'probed',
  runs: [
    { label: '--help', args: ['--help'], text: 'Usage: fake-agent <COMMAND>\nCommands:\n  run  Run', exit: 0, timed_out: false, truncated: false, bytes: 40, duplicate_of: null },
    { label: '-h', args: ['-h'], text: '', exit: 0, timed_out: false, truncated: false, bytes: 40, duplicate_of: '--help' },
  ],
  subcommands: ['run'],
  skipped: [],
};

const verifyOk = (toml: string): VerifyResult => ({
  ok: true,
  state: 'verified',
  errors: [],
  text: '',
  warnings: [],
  rendered: { headless_argv: ['run', '--', '<task>'], resume_argv: null },
  checked: { fixtures: 1, rules: 8 },
  samples_replayed: 0,
  sha256: toml.length.toString(),
});

const verifyBad = (): VerifyResult => ({
  ...verifyOk(''),
  ok: false,
  state: 'probed',
  errors: [{ stage: 'validate', code: 'AGENT_DEF_INCONSISTENT', path: 'headless.output.result.path', line: 5, column: 1, message: 'needs a path', fix: 'add path' }],
  text: CHECKER_TEXT,
  sha256: undefined,
});

const liveOk = (): LiveResult => ({
  ok: true,
  state: 'tested',
  stages: [
    { name: 'pong', label: 'Say pong', status: 'passed', args: ['run'], exit: 0, duration_ms: 10, timed_out: false, result: 'pong', session: 's1', error_code: null, problems: [], note: null, stdout: '{"text":"pong"}', stderr: '' },
  ],
  errors: [],
  text: '',
  tested_version: '1.4.2',
  fixtures: ['headless_success.jsonl', 'headless_garbage.txt'],
});

const liveBad = (): LiveResult => ({
  ...liveOk(),
  ok: false,
  state: 'verified',
  stages: [
    { name: 'pong', label: 'Say pong', status: 'failed', args: ['run', '--', 'Reply with exactly the word: pong'], exit: 0, duration_ms: 10, timed_out: false, result: null, session: null, error_code: 'AGENT_OUTPUT_UNPARSEABLE', problems: ['no result'], note: null, stdout: '{"event":"end","text":"pong"}', stderr: '' },
    { name: 'resume', label: 'Continue', status: 'skipped', args: [], exit: null, duration_ms: 0, timed_out: false, result: null, session: null, error_code: null, problems: [], note: 'not run', stdout: '', stderr: '' },
  ],
  errors: [{ stage: 'livetest', code: 'LIVETEST_PONG_FAILED', path: 'pong', line: null, column: null, message: 'no result was found', fix: '' }],
  text: LIVE_TEXT,
});

/** A stand-in drafter: returns the recorded replies in order and remembers every prompt it was sent. */
function standIn(replies: string[]) {
  const prompts: string[] = [];
  const ask = vi.fn(async (prompt: string) => {
    prompts.push(prompt);
    return replies[Math.min(prompts.length - 1, replies.length - 1)];
  });
  return { ask, prompts };
}

function options(over: Partial<LoopOptions> & Pick<LoopOptions, 'ask'>): LoopOptions & { events: ProgressEvent[] } {
  const events: ProgressEvent[] = [];
  return {
    api: {
      verify: vi.fn(async (_s: string, toml: string) => (toml.includes('GOOD') || toml.includes('WRONG_FIELD') ? verifyOk(toml) : verifyBad())),
      livetest: vi.fn(async () => liveOk()),
    },
    session: 'wz_1',
    facts: factsFrom(start, probe),
    maxChars: LOCAL_PROMPT_BUDGET,
    confirmLive: vi.fn(async () => true),
    onEvent: (e) => events.push(e),
    events,
    ...over,
  };
}

describe('extractTomlBlock', () => {
  it('takes the one toml block and nothing around it', () => {
    expect(extractTomlBlock(block('a = 1\nb = 2'))).toEqual({ ok: true, toml: 'a = 1\nb = 2' });
    expect(extractTomlBlock(`${F}TOML\r\na = 1\r\n${F}`)).toEqual({ ok: true, toml: 'a = 1' });
  });

  it('tells the drafter when there is no block or more than one', () => {
    const none = extractTomlBlock('I think you should use --json.');
    expect(none.ok).toBe(false);
    expect(!none.ok && none.problem).toContain('no ```toml block');
    const two = extractTomlBlock(`${block('a = 1')}\n${block('b = 2')}`);
    expect(!two.ok && two.problem).toContain('2 ```toml blocks');
    const empty = extractTomlBlock(`${F}toml\n\n${F}`);
    expect(!empty.ok && empty.problem).toContain('empty');
  });

  it('accepts one unlabelled fence that starts like a definition, and no other', () => {
    expect(extractTomlBlock(`${F}\nschema_version = 1\n[agent]\n${F}`)).toEqual({ ok: true, toml: 'schema_version = 1\n[agent]' });
    expect(extractTomlBlock(`${F}\nls -la\n${F}`).ok).toBe(false);
    expect(extractTomlBlock(`${F}json\n{"schema_version": 1}\n${F}`).ok).toBe(false);
  });
});

describe('small helpers', () => {
  it('splits CODE: message errors', () => {
    expect(splitError('WIZARD_BUSY: wait for it')).toEqual({ code: 'WIZARD_BUSY', message: 'wait for it' });
    expect(splitError(new Error('AGENT_NOT_INSTALLED: `x` was not found'))).toEqual({ code: 'AGENT_NOT_INSTALLED', message: '`x` was not found' });
    expect(splitError('plain failure')).toEqual({ code: '', message: 'plain failure' });
  });

  it('defaults to Claude Code, then Codex, then anything', () => {
    const all = [
      { id: 'kilo', label: 'Kilo' },
      { id: 'codex', label: 'Codex' },
      { id: 'claude', label: 'Claude Code' },
    ];
    expect(chooseDefaultDrafter(all)?.id).toBe('claude');
    expect(chooseDefaultDrafter(all.slice(0, 2))?.id).toBe('codex');
    expect(chooseDefaultDrafter(all.slice(0, 1))?.id).toBe('kilo');
    expect(chooseDefaultDrafter([])).toBeNull();
    expect(orderDrafters(all).map((d) => d.id)).toEqual(['claude', 'codex', 'kilo']);
  });

  it('records who wrote the draft without any key or path', () => {
    expect(provenanceOf({ kind: 'local_agent', id: 'claude', label: 'Claude Code' }, 2)).toEqual({
      drafter: 'Claude Code (local)',
      drafter_kind: 'local_agent',
      model: null,
      prompt_version: PROMPT_VERSION,
      rounds: 2,
    });
    const or = provenanceOf({ kind: 'openrouter', model: 'google/gemini-2.5-flash', label: 'Gemini' }, 1);
    expect(or).toMatchObject({ drafter: 'OpenRouter google/gemini-2.5-flash', drafter_kind: 'openrouter', model: 'google/gemini-2.5-flash' });
    expect(promptBudget({ kind: 'local_agent', id: 'claude', label: 'C' })).toBe(LOCAL_PROMPT_BUDGET);
    expect(promptBudget({ kind: 'openrouter', model: 'm', label: 'M' })).toBe(REMOTE_PROMPT_BUDGET);
  });

  it('turns the start and probe answers into prompt facts', () => {
    const facts = factsFrom(start, probe);
    expect(facts).toMatchObject({ command: 'fake-agent', idSuggestion: 'fake_agent', version: '1.4.2', programKind: 'native' });
    expect(facts.probes[1].duplicate_of).toBe('--help');
    expect(factsFrom({ ...start, version: { ok: false, args: [], text: '', number: null } }, probe).version).toBeNull();
  });

  it('shows the program name first in the command line', () => {
    expect(commandLine('fake-agent', ['run', '--format', 'json', '--', '<task>'])).toBe('fake-agent run --format json -- <task>');
    expect(commandLine('x', ['-m', 'a b'])).toBe('x -m "a b"');
    expect(commandLine('x', null)).toBe('x');
  });

  it('keeps the real output of each stage that ran for the next prompt', () => {
    const samples = samplesFrom(liveBad());
    expect(samples).toHaveLength(1);
    expect(samples[0]).toMatchObject({ stage: 'pong', exit: 0, error_code: 'AGENT_OUTPUT_UNPARSEABLE' });
  });
});

describe('runDraftLoop with a stand-in drafter', () => {
  it('finishes in one round for a good first draft, and asks about the live test once', async () => {
    const drafter = standIn([block(GOOD)]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    if (result.status === 'tested') {
      expect(result.rounds).toBe(1);
      expect(result.toml).toBe(GOOD.trim());
    }
    expect(drafter.ask).toHaveBeenCalledTimes(1);
    expect(o.confirmLive).toHaveBeenCalledTimes(1);
    expect(o.events.map((e) => `${e.step}:${e.status}`)).toEqual([
      'draft:running', 'draft:done', 'check:running', 'check:done', 'live:running', 'live:done',
    ]);
    // round 1 asks for the file; it carries the facts and no feedback
    expect(drafter.prompts[0]).toContain('Command typed by the person: fake-agent');
    expect(drafter.prompts[0]).toContain('Now write the file.');
  });

  it('feeds the checker text back verbatim and fixes the file in round 2', async () => {
    const drafter = standIn([block(BROKEN), block(GOOD)]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(result.status === 'tested' && result.rounds).toBe(2);
    expect(drafter.prompts[1]).toContain(CHECKER_TEXT.trim());
    expect(drafter.prompts[1]).toContain('# BROKEN');
    expect(drafter.prompts[1]).toContain('Fix every problem.');
    expect(drafter.prompts[1]).not.toContain('Now write the file.');
    const steps = o.events.map((e) => `${e.step}:${e.status}`);
    expect(steps).toContain('check:failed');
    expect(steps).toContain('fix:running');
    expect(o.events.find((e) => e.step === 'check' && e.status === 'failed')?.detail).toBe('1 problem');
  });

  it('tells a drafter that forgot the toml block and tries again', async () => {
    const drafter = standIn(['Sure! You should add a headless table.', block(GOOD)]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(drafter.prompts[1]).toContain('Your reply had no ```toml block');
    expect(o.api.verify).toHaveBeenCalledTimes(1);
  });

  it('feeds a failed live test back with the real output, without asking to confirm again', async () => {
    const drafter = standIn([block(WRONG_FIELD), block(GOOD)]);
    const livetest = vi
      .fn<() => Promise<LiveResult>>()
      .mockResolvedValueOnce(liveBad())
      .mockResolvedValueOnce(liveOk());
    const o = options({ ask: drafter.ask });
    o.api = { verify: o.api.verify, livetest };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(result.status === 'tested' && result.rounds).toBe(2);
    expect(o.confirmLive).toHaveBeenCalledTimes(1);
    expect(drafter.prompts[1]).toContain(LIVE_TEXT.trim());
    expect(drafter.prompts[1]).toContain('<<<SAMPLE pong');
    expect(drafter.prompts[1]).toContain('{"event":"end","text":"pong"}');
    expect(drafter.prompts[1]).toContain('# WRONG_FIELD');
  });

  it('stops without a live test when the person says not now', async () => {
    const drafter = standIn([block(GOOD)]);
    const o = options({ ask: drafter.ask, confirmLive: vi.fn(async () => false) });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('declined');
    expect(o.api.livetest).not.toHaveBeenCalled();
  });

  it('gives up after five rounds with the last checker text', async () => {
    const drafter = standIn([block(BROKEN)]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result).toMatchObject({ status: 'gave_up', rounds: MAX_ROUNDS });
    expect(result.status === 'gave_up' && result.lastText).toBe(CHECKER_TEXT);
    expect(drafter.ask).toHaveBeenCalledTimes(MAX_ROUNDS);
    expect(o.confirmLive).not.toHaveBeenCalled();
    expect(o.api.livetest).not.toHaveBeenCalled();
  });

  it('stops at once when it is cancelled, even while waiting for the drafter', async () => {
    const abort = new AbortController();
    const ask = vi.fn(async () => {
      abort.abort();
      return block(GOOD);
    });
    const o = options({ ask, signal: abort.signal });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('cancelled');
    expect(o.api.verify).not.toHaveBeenCalled();
  });

  it('passes a drafter error up after marking the step failed', async () => {
    const ask = vi.fn(async () => {
      throw new Error('DRAFTER_TIMEOUT: too slow');
    });
    const o = options({ ask });
    await expect(runDraftLoop(o)).rejects.toThrow('DRAFTER_TIMEOUT');
    expect(o.events.at(-1)).toMatchObject({ step: 'draft', status: 'failed', detail: 'too slow' });
  });

  it('uses the budget it was given', async () => {
    const big: ProbeInfo = { ...probe, runs: [{ ...probe.runs[0], text: 'x'.repeat(65_000) }] };
    const drafter = standIn([block(GOOD)]);
    await runDraftLoop(options({ ask: drafter.ask, facts: factsFrom(start, big), maxChars: LOCAL_PROMPT_BUDGET }));
    expect(drafter.prompts[0].length).toBeLessThanOrEqual(LOCAL_PROMPT_BUDGET);
    const remote = standIn([block(GOOD)]);
    await runDraftLoop(options({ ask: remote.ask, facts: factsFrom(start, big), maxChars: REMOTE_PROMPT_BUDGET }));
    expect(remote.prompts[0].length).toBeGreaterThan(LOCAL_PROMPT_BUDGET);
  });
});

describe('askOpenRouter', () => {
  it('sends the prompt through the existing client and returns the text', async () => {
    const stream = vi.fn(async () => ({ content: [{ type: 'text' as const, text: 'hello' }, { type: 'text' as const, text: 'world' }], stop_reason: 'end_turn' as const }));
    const text = await askOpenRouter({ apiKey: 'k', model: 'google/gemini', prompt: 'the prompt', stream });
    expect(text).toBe('hello\nworld');
    expect(stream).toHaveBeenCalledTimes(1);
    const [provider, key, req] = stream.mock.calls[0] as unknown as [string, string, { model: string; messages: Array<{ content: string }>; tools: unknown[] }];
    expect(provider).toBe('openrouter');
    expect(key).toBe('k');
    expect(req.model).toBe('google/gemini');
    expect(req.messages[0].content).toBe('the prompt');
    expect(req.tools).toEqual([]);
  });

  it('refuses an empty answer', async () => {
    const stream = vi.fn(async () => ({ content: [], stop_reason: 'end_turn' as const }));
    await expect(askOpenRouter({ apiKey: 'k', model: 'm', prompt: 'p', stream })).rejects.toThrow('empty');
  });
});
