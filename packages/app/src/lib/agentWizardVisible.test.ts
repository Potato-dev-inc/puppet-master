// The visible path of the wizard's front end (SPEC 11): reading a reply with expectation blocks, the draft loop
// with a captured session (verify -> confirm -> visible test -> optional headless test), and Retry.
import { describe, expect, it, vi } from 'vitest';
import {
  extractDraft,
  factsFrom,
  offersOneTaskMode,
  retryLiveTest,
  runDraftLoop,
  type LiveResult,
  type LoopOptions,
  type PlannedVisible,
  type ProbeInfo,
  type ProgressEvent,
  type StartInfo,
  type VerifyResult,
  type VisibleTestResult,
} from './agentWizard';
import type { CaptureFacts } from './agentWizardPrompt';

const F = '`'.repeat(3);
const reply = (toml: string, expects: Record<string, string> = {}) =>
  [`Here is the file.\n${F}toml\n${toml}\n${F}`, ...Object.entries(expects).map(([n, t]) => `${F}toml\n# expect: ${n}\n${t}\n${F}`), 'Done.'].join('\n');

const GOOD = 'schema_version = 1\n# GOOD\n[agent]\nid = "nimbus"\n[tui]\nargv = []\n';
const WRONG = 'schema_version = 1\n# WRONG\n[agent]\nid = "nimbus"\n[tui]\nargv = []\n';
const CHANGED = 'schema_version = 1\n# CHANGED\n[agent]\nid = "nimbus"\n[tui]\nargv = ["--x"]\n';

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'nimbus',
  id_suggestion: 'nimbus',
  program: { path: 'C:\\x\\nimbus.exe', display: '~\\nimbus.exe', kind: 'native' },
  version: { ok: false, args: ['--version'], text: '', number: null },
  native_candidates: [],
  cost_note: 'The live test runs nimbus 4 times.',
  limits: {},
};

const probe: ProbeInfo = {
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text: 'Usage: nimbus [--script FILE]', exit: 0, timed_out: false, truncated: false, bytes: 30, duplicate_of: null }],
  subcommands: [],
  skipped: [],
};

const captures: CaptureFacts = {
  grid: { cols: 120, rows: 40 },
  files: [
    { name: 'tui_idle', text: '| > Type a task...', variants: [] },
    { name: 'tui_working', text: '\u280b Thinking... ctrl+c to stop', variants: [] },
    { name: 'tui_reply', text: 'reply: <word-1>', variants: [] },
    { name: 'tui_permission', text: 'Allow nimbus to create hello.txt? [y/n]', variants: [] },
  ],
  facts: { exit_method: 'slash_exit', partial: [], skipped: [], hello_written: false, startup_dialogs: ['trust'], exit_screen: null, notes: [] },
};

const plannedVisible = (toml: string): PlannedVisible => ({
  program: { path: 'C:\\x\\nimbus.exe', display: '~\\nimbus.exe' },
  args: toml.includes('CHANGED') ? ['--x'] : [],
  env: [],
  env_unset: [],
  cwd: '<isolated folder>',
  grid: { cols: 120, rows: 40 },
  fingerprint: toml.includes('CHANGED') ? 'vis-B' : 'vis-A',
});

const verifyOk = (toml: string, headless = false): VerifyResult => ({
  ok: true,
  state: 'verified',
  errors: [],
  text: '',
  warnings: [],
  rendered: { headless_argv: headless ? ['run'] : null, resume_argv: null, tui_argv: [] },
  checked: { fixtures: 5, rules: 8 },
  samples_replayed: 0,
  sha256: 'x',
  planned_visible: plannedVisible(toml),
  can_visibletest: true,
  ...(headless
    ? {
        planned: { program: { path: 'p', display: 'p' }, stages: [{ name: 'pong', label: 'Say pong', args: ['run'] }], stdin: null, env: [], env_unset: [], fingerprint: 'live-A' },
        can_livetest: true,
      }
    : {}),
});

const CHECKER = 'The checker found 1 problem. Fix every one and reply with the complete corrected file in one ```toml block.\n\n1. [fixture] FIXTURE_FAILED at `tui_idle.txt`\n   Problem: ready: expected true, got false\n';
const verifyBad = (): VerifyResult => ({ ...verifyOk(WRONG), ok: false, state: 'probed', errors: [], text: CHECKER, planned_visible: undefined, can_visibletest: undefined });

const VISIBLE_TEXT =
  'The visible test started your profile in a real pane and found 1 problem. Fix every one and reply with the complete corrected file in one ```toml block.\n\n1. [visibletest] VISIBLETEST_READY_FAILED at `ready`\n   Problem: never ready\n';

const visibleOk = (): VisibleTestResult => ({
  ok: true,
  state: 'tested',
  checks: [{ name: 'starts', label: 'The agent starts in a pane', status: 'passed', note: '', ms: 5 }],
  errors: [],
  text: '',
  version: null,
  passed: 7,
  total: 7,
  agent_error: null,
});

const visibleBad = (extra: Partial<VisibleTestResult> = {}): VisibleTestResult => ({
  ...visibleOk(),
  ok: false,
  state: 'probed',
  text: VISIBLE_TEXT,
  errors: [{ stage: 'visibletest', code: 'VISIBLETEST_READY_FAILED', path: 'ready', line: null, column: null, message: 'never ready', fix: '' }],
  passed: 1,
  ...extra,
});

const liveOk = (): LiveResult => ({ ok: true, state: 'tested', stages: [], errors: [], text: '', tested_version: null, fixtures: [] });

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
      verify: vi.fn(async (_s: string, toml: string) => (toml.includes('WRONG') ? verifyBad() : verifyOk(toml))),
      livetest: vi.fn(async () => liveOk()),
      visibletest: vi.fn(async () => visibleOk()),
    },
    session: 'wz_1',
    facts: factsFrom(start, probe, captures),
    maxChars: 120_000,
    confirmLive: vi.fn(async () => true),
    confirmVisible: vi.fn(async () => true),
    onEvent: (e) => events.push(e),
    events,
    ...over,
  };
}

describe('extractDraft', () => {
  it('takes the definition block and the expectation blocks by capture name', () => {
    const out = extractDraft(reply(GOOD, { tui_idle: 'chrome_lines = ["NIMBUS"]', tui_reply: 'not_chrome_lines = ["reply: x"]' }));
    expect(out).toEqual({ ok: true, toml: GOOD.trim(), expects: { tui_idle: 'chrome_lines = ["NIMBUS"]', tui_reply: 'not_chrome_lines = ["reply: x"]' } });
  });

  it('works like the old reader for a reply with one block', () => {
    expect(extractDraft(reply(GOOD))).toEqual({ ok: true, toml: GOOD.trim(), expects: {} });
    expect(extractDraft('schema_version = 1\n')).toMatchObject({ ok: false });
  });

  it('says what is wrong with a reply that has no definition, two definitions, or two blocks for one capture', () => {
    const only = extractDraft(`${F}toml\n# expect: tui_idle\nmenu = false\n${F}`);
    expect(only).toMatchObject({ ok: false });
    expect((only as { problem: string }).problem).toContain('only expectation blocks');
    const two = extractDraft(`${reply(GOOD)}\n${F}toml\n${WRONG}\n${F}`);
    expect((two as { problem: string }).problem).toContain('2 ```toml blocks that are not expectation blocks');
    const dup = extractDraft(reply(GOOD, {}) + `\n${F}toml\n# expect: tui_idle\na = 1\n${F}\n${F}toml\n# expect: tui_idle\nb = 2\n${F}`);
    expect((dup as { problem: string }).problem).toContain('two expectation blocks for tui_idle');
  });

  it('an expectation header is read in any spacing and the body is kept as written', () => {
    const out = extractDraft(`${F}toml\n${GOOD}\n${F}\n${F}toml\n#expect:tui_working\nbusy = true\n${F}`);
    expect(out).toMatchObject({ ok: true, expects: { tui_working: 'busy = true' } });
  });
});

describe('offersOneTaskMode', () => {
  const run = (text: string): ProbeInfo => ({ ...probe, runs: [{ ...probe.runs[0], text }] });
  it('is true when the help mentions a non-interactive, print, exec or JSON mode', () => {
    for (const text of ['Options:\n  -p, --print  run non-interactively', 'Commands:\n  exec  run one task', 'use --output-format json', 'headless mode']) {
      expect(offersOneTaskMode(run(text)), text).toBe(true);
    }
  });
  it('is false for a help that only describes the interactive program', () => {
    expect(offersOneTaskMode(run('Usage: nimbus [--script FILE]\nStarts the interactive assistant.'))).toBe(false);
  });
});

describe('runDraftLoop with a captured session', () => {
  it('verifies with the expectation blocks, confirms the pane once, runs the visible test and finishes', async () => {
    const drafter = standIn([reply(GOOD, { tui_idle: 'chrome_lines = ["NIMBUS"]' })]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    if (result.status !== 'tested') return;
    expect(result.rounds).toBe(1);
    expect(result.visible?.ok).toBe(true);
    expect(result.live).toBeNull();
    expect(result.expects).toEqual({ tui_idle: 'chrome_lines = ["NIMBUS"]' });
    expect(o.api.verify).toHaveBeenCalledWith('wz_1', GOOD.trim(), { tui_idle: 'chrome_lines = ["NIMBUS"]' });
    expect(o.confirmVisible).toHaveBeenCalledWith(plannedVisible(GOOD));
    expect(o.api.visibletest).toHaveBeenCalledWith('wz_1', 'vis-A');
    expect(o.api.livetest).not.toHaveBeenCalled();
    expect(drafter.prompts[0]).toContain('<<<CAPTURE tui_idle');
    expect(o.events.map((e) => `${e.step}:${e.status}`)).toEqual(['draft:running', 'draft:done', 'check:running', 'check:done', 'visible:running', 'visible:done']);
  });

  it('feeds a failed visible test back verbatim with the previous file and its expectation blocks, without asking again', async () => {
    const drafter = standIn([reply(WRONG, { tui_idle: 'menu = false' }), reply(GOOD)]);
    const visibletest = vi.fn<(s: string, p: string) => Promise<VisibleTestResult>>().mockResolvedValueOnce(visibleBad()).mockResolvedValueOnce(visibleOk());
    const o = options({ ask: drafter.ask });
    o.api = { ...o.api, visibletest, verify: vi.fn(async (_s: string, toml: string) => verifyOk(toml)) };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(result.status === 'tested' && result.rounds).toBe(2);
    expect(o.confirmVisible).toHaveBeenCalledTimes(1);
    expect(drafter.prompts[1]).toContain(VISIBLE_TEXT.trim());
    expect(drafter.prompts[1]).toContain('# WRONG');
    expect(drafter.prompts[1]).toContain('# expect: tui_idle');
    expect(drafter.prompts[1]).toContain('menu = false');
  });

  it('a draft the checker rejects goes straight back without a test', async () => {
    const drafter = standIn([reply(WRONG), reply(GOOD)]);
    const o = options({ ask: drafter.ask });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(drafter.prompts[1]).toContain(CHECKER.trim());
    expect(o.api.visibletest).toHaveBeenCalledTimes(1);
  });

  it('asks again only when a fix changes what the pane would start', async () => {
    const drafter = standIn([reply(WRONG), reply(CHANGED), reply(GOOD), reply(GOOD)]);
    const verify = vi.fn(async (_s: string, toml: string) => verifyOk(toml));
    const visibletest = vi
      .fn<(s: string, p: string) => Promise<VisibleTestResult>>()
      .mockResolvedValueOnce(visibleBad())
      .mockResolvedValueOnce(visibleBad())
      .mockResolvedValueOnce(visibleBad())
      .mockResolvedValueOnce(visibleOk());
    const o = options({ ask: drafter.ask });
    o.api = { ...o.api, verify, visibletest };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect((o.confirmVisible as ReturnType<typeof vi.fn>).mock.calls.map((c) => (c[0] as PlannedVisible).fingerprint)).toEqual(['vis-A', 'vis-B', 'vis-A']);
    expect(visibletest.mock.calls.map((c) => c[1])).toEqual(['vis-A', 'vis-B', 'vis-A', 'vis-A']);
  });

  it('declines when the person does not agree to the visible test', async () => {
    const o = options({ ask: standIn([reply(GOOD)]).ask, confirmVisible: vi.fn(async () => false) });
    const result = await runDraftLoop(o);
    expect(result.status).toBe('declined');
    expect(o.api.visibletest).not.toHaveBeenCalled();
  });

  it('stops with the program\'s own error instead of asking for another draft', async () => {
    const drafter = standIn([reply(GOOD)]);
    const o = options({ ask: drafter.ask });
    o.api = { ...o.api, visibletest: vi.fn(async () => visibleBad({ agent_error: { check: 'ready', message: 'Please log in', external: true } })) };
    const result = await runDraftLoop(o);
    expect(result).toMatchObject({ status: 'agent_error', error: { stage: 'ready', message: 'Please log in', external: true } });
    expect(drafter.ask).toHaveBeenCalledTimes(1);
  });

  it('asks the drafter for another round when the visible test failed on the draft and the only "error" is the banner', async () => {
    // A prompt regex that is too broad, on a TUI whose first row is the banner: the draft is at fault, so a fix round.
    const banner = { check: 'done_reply', message: '\u2584\u2580 Nimbus CLI 1.4.2.', external: true };
    const drafter = standIn([reply(GOOD), reply(GOOD)]);
    const o = options({ ask: drafter.ask });
    const visibletest = vi
      .fn<(s: string, plan: string) => Promise<VisibleTestResult>>()
      .mockResolvedValueOnce(visibleBad({ agent_error: banner }))
      .mockResolvedValueOnce(visibleOk());
    o.api = { ...o.api, visibletest };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(drafter.ask).toHaveBeenCalledTimes(2);
    expect(drafter.prompts[1]).toContain('VISIBLETEST_READY_FAILED');
  });

  it('does not stop on the same banner twice in a row', async () => {
    const banner = { check: 'done_reply', message: '\u2584\u2580 Nimbus CLI 1.4.2.', external: false };
    const drafter = standIn([reply(GOOD), reply(GOOD), reply(GOOD)]);
    const o = options({ ask: drafter.ask });
    const visibletest = vi
      .fn<(s: string, plan: string) => Promise<VisibleTestResult>>()
      .mockResolvedValueOnce(visibleBad({ agent_error: banner }))
      .mockResolvedValueOnce(visibleBad({ agent_error: banner }))
      .mockResolvedValueOnce(visibleOk());
    o.api = { ...o.api, visibletest };
    expect((await runDraftLoop({ ...o, maxRounds: 4 })).status).toBe('tested');
    expect(drafter.ask).toHaveBeenCalledTimes(3);
  });

  it('still stops on a real provider error row, with that row as the message', async () => {
    const row = { check: 'done_reply', message: 'Error: No endpoints found that support tool use.', external: true };
    const drafter = standIn([reply(GOOD)]);
    const o = options({ ask: drafter.ask });
    o.api = { ...o.api, visibletest: vi.fn(async () => visibleBad({ agent_error: row })) };
    const result = await runDraftLoop(o);
    expect(result).toMatchObject({ status: 'agent_error', error: { stage: 'done_reply', message: row.message, external: true } });
    expect(drafter.ask).toHaveBeenCalledTimes(1);
  });

  it('Retry reports a banner "error" as a failed test, not as the programs own error', async () => {
    const verify = verifyOk(GOOD);
    const visibletest = vi.fn().mockResolvedValue(visibleBad({ agent_error: { check: 'done_reply', message: '\u2584\u2580 Nimbus CLI 1.4.2.', external: true } }));
    const out = await retryLiveTest({ api: { livetest: vi.fn(), visibletest }, session: 's', toml: GOOD, rounds: 1, verify });
    expect(out.status).toBe('failed');
  });

  it('with the one-task mode added, both tests must pass and both plans are confirmed', async () => {
    const drafter = standIn([reply(GOOD)]);
    const o = options({
      ask: drafter.ask,
      promptOptions: { addOneTaskMode: { toml: 'schema_version = 1\n[tui]\nargv = []\n' } },
    });
    o.api = { ...o.api, verify: vi.fn(async (_s: string, toml: string) => verifyOk(toml, true)) };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    if (result.status !== 'tested') return;
    expect(result.visible?.ok && result.live?.ok).toBe(true);
    expect(o.confirmLive).toHaveBeenCalledTimes(1);
    expect(o.confirmVisible).toHaveBeenCalledTimes(1);
    expect(drafter.prompts[0]).toContain('The file to keep');
    expect(drafter.prompts[0]).toContain('Now add the [headless] tables');
  });

  it('a headless-only session (no captures) never runs the visible test', async () => {
    const o = options({ ask: standIn([reply(GOOD)]).ask, facts: factsFrom(start, probe) });
    o.api = { ...o.api, verify: vi.fn(async (_s: string, toml: string) => ({ ...verifyOk(toml, true), planned_visible: undefined })) };
    const result = await runDraftLoop(o);
    expect(result.status).toBe('tested');
    expect(o.api.visibletest).not.toHaveBeenCalled();
    expect(o.confirmVisible).not.toHaveBeenCalled();
    expect(o.api.livetest).toHaveBeenCalledTimes(1);
  });
});

describe('retryLiveTest for the visible path', () => {
  const verify = verifyOk(GOOD);
  it('runs only the visible test of the kept draft with the confirmed fingerprint', async () => {
    const visibletest = vi.fn<(s: string, p: string) => Promise<VisibleTestResult>>().mockResolvedValueOnce(visibleOk());
    const events: ProgressEvent[] = [];
    const out = await retryLiveTest({ api: { livetest: vi.fn(), visibletest }, session: 'wz_1', toml: GOOD, rounds: 2, verify, onEvent: (e) => events.push(e) });
    expect(out.status).toBe('tested');
    expect(visibletest).toHaveBeenCalledWith('wz_1', 'vis-A');
    expect(events.map((e) => `${e.step}:${e.status}`)).toEqual(['visible:running', 'visible:done']);
  });

  it('reports the program\'s own error again, or a normal failure with its text', async () => {
    const visibletest = vi
      .fn<(s: string, p: string) => Promise<VisibleTestResult>>()
      .mockResolvedValueOnce(visibleBad({ agent_error: { check: 'ready', message: 'not logged in', external: true } }))
      .mockResolvedValueOnce(visibleBad());
    expect(await retryLiveTest({ api: { livetest: vi.fn(), visibletest }, session: 's', toml: GOOD, rounds: 1, verify })).toMatchObject({ status: 'agent_error' });
    expect(await retryLiveTest({ api: { livetest: vi.fn(), visibletest }, session: 's', toml: GOOD, rounds: 1, verify })).toMatchObject({ status: 'failed', text: VISIBLE_TEXT });
  });

  it('runs both tests when the draft has both halves', async () => {
    const both = verifyOk(GOOD, true);
    const visibletest = vi.fn<(s: string, p: string) => Promise<VisibleTestResult>>().mockResolvedValue(visibleOk());
    const livetest = vi.fn<(s: string, p: string) => Promise<LiveResult>>().mockResolvedValue(liveOk());
    const out = await retryLiveTest({ api: { livetest, visibletest }, session: 's', toml: GOOD, rounds: 1, verify: both });
    expect(out.status).toBe('tested');
    expect(visibletest).toHaveBeenCalledWith('s', 'vis-A');
    expect(livetest).toHaveBeenCalledWith('s', 'live-A');
  });
});
