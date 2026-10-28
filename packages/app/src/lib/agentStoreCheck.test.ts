// "Check it works" glue (AS-20): the stand-in writer, the catalog reading, and the whole run over a fake wizard api.
import { describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { expectsFrom, firstBinaryName, makeStoreAsk, NO_DRAFTER_MESSAGE, plain, realDeps, runStoreCheck, type StoreCheckDeps, type StoreCheckSource } from './agentStoreCheck';
import { buildClassifierPrompt, buildMenuPrompt, type AutoTimings, type Clock } from './agentWizardAuto';
import {
  extractDraft,
  type CaptureStatus,
  type ProbeInfo,
  type ProgressEvent,
  type StartInfo,
  type VerifyResult,
  type VisibleTestResult,
  type WizardApi,
} from './agentWizard';
import type { CaptureFacts } from './agentWizardPrompt';

// The real dependencies call Tauri; the tests in the last block check the command names and their arguments only.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const F = '`'.repeat(3);
const TOML = 'schema_version = 1\n\n[agent]\nid = "nimbus"\nlabel = "Nimbus"\n\n[binary]\nnames = ["nimbus", "nimbus-cli"]\nversion_args = ["--version"]\n\n[tui]\nargv = []\n';
const EXPECT_IDLE = '[expect]\ncontains = ["Type a task"]';
const SOURCE: StoreCheckSource = {
  definition: TOML,
  fixtures: [
    { name: 'tui_idle.txt', text: '| > Type a task...' },
    { name: 'tui_idle.expect.toml', text: EXPECT_IDLE },
  ],
};

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'nimbus',
  id_suggestion: 'nimbus',
  program: { path: 'C:\\x\\nimbus.exe', display: '~\\nimbus.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'nimbus 0.4.1', number: '0.4.1' },
  native_candidates: [],
  cost_note: '',
  limits: {},
};
const probe: ProbeInfo = {
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text: 'Usage: nimbus', exit: 0, timed_out: false, truncated: false, bytes: 10, duplicate_of: null }],
  subcommands: [],
  skipped: [],
};
const captures: CaptureFacts = {
  grid: { cols: 120, rows: 40 },
  files: [{ name: 'tui_idle', text: '| > Type a task...', variants: [] }],
  facts: { exit_method: 'slash_exit', partial: [], skipped: [], hello_written: false, startup_dialogs: [], exit_screen: null, notes: [] },
};

const steps = (): CaptureStatus['steps'] =>
  ['start', 'ready', 'message', 'permission', 'exit'].map((id) => ({ id: id as 'start', label: id, status: 'running', note: '', frames: 0 }));
const SETTLED: CaptureStatus = { state: 'running', captured: false, steps: steps(), step: 'exit', stable_ms: 3000, settled: true, fingerprint: 'fp', screen: 'NIMBUS', needs_you: null, can: ['retry'] };
const unsure = (kind = 'unsure'): CaptureStatus => ({
  ...SETTLED,
  settled: false,
  step: 'ready',
  needs_you: { step: 'ready', kind, message: 'I cannot tell whether the agent is waiting for a task.' },
  screen: 'NIMBUS 0.4\n\nTip: ask me to approve a plan.\n\n| > Type a task...',
});

const verifyOk = (): VerifyResult => ({
  ok: true,
  state: 'verified',
  errors: [],
  text: '',
  warnings: [],
  checked: { fixtures: 5, rules: 8 },
  samples_replayed: 0,
  planned_visible: {
    program: { path: 'C:\\x\\nimbus.exe', display: '~\\nimbus.exe' },
    args: [],
    env: [],
    env_unset: [],
    cwd: '<isolated folder>',
    grid: { cols: 120, rows: 40 },
    fingerprint: 'vis-A',
  },
  can_visibletest: true,
});
const verifyBad = (): VerifyResult => ({
  ...verifyOk(),
  ok: false,
  planned_visible: undefined,
  can_visibletest: undefined,
  errors: [{ stage: 'fixtures', code: 'FIXTURE_X', path: '', line: null, column: null, message: 'x', fix: '' }],
  text: 'The checker found 1 problem. SCREEN-TEXT-C:\\Users\\you\\secret',
});
const visibleOk = (): VisibleTestResult => ({
  ok: true,
  state: 'tested',
  checks: [{ name: 'starts', label: 'The agent starts in a pane', status: 'passed', note: '', ms: 5 }],
  errors: [],
  text: '',
  version: '0.4.1',
  passed: 7,
  total: 7,
});

/** A clock that moves only when you sleeps. */
function fakeClock(): Clock {
  const c = {
    t: 0,
    now: () => c.t,
    sleep: async (ms: number) => {
      await new Promise((r) => setTimeout(r, 0));
      c.t += ms;
    },
  };
  return c;
}
const TIMINGS: AutoTimings = { pollMs: 500, stableMs: 1500, patienceMs: 20_000, escalateMs: 8000, captureMs: 600_000 };

/** The fake wizard: a capture that settles at once (or shows the given status), a checker, a visible test, and spies. */
function fakeApi(over: Partial<WizardApi> = {}, capture: CaptureStatus = SETTLED): WizardApi {
  return {
    start: vi.fn(async () => start),
    probe: vi.fn(async () => probe),
    verify: vi.fn(async () => verifyOk()),
    livetest: vi.fn(),
    visibletest: vi.fn(async () => visibleOk()),
    testStatus: vi.fn(async () => ({ running: false })),
    capturePlan: vi.fn(async () => ({ program: { path: 'C:\\x\\nimbus.exe', display: 'x' }, args: [], cwd: '<isolated folder>', grid: { cols: 120, rows: 40 }, fingerprint: 'cap-A' })),
    captureStart: vi.fn(async () => capture),
    captureStatus: vi.fn(async () => capture),
    captureAct: vi.fn(async () => capture),
    captureShow: vi.fn(),
    captureFinish: vi.fn(async () => captures),
    captureDiscard: vi.fn(),
    captureDump: vi.fn(async () => ({ path: 'C:\\Temp\\wizard-debug.json' })),
    verdict: vi.fn(async (_s: string, verdict) => ({ decision: 'wait_then_skip', verdict, status: capture })),
    testVerdict: vi.fn(),
    finish: vi.fn(),
    cancel: vi.fn(async () => undefined),
    ask: vi.fn(async () => 'unknown'),
    drafters: vi.fn(async () => []),
    ...over,
  } as unknown as WizardApi;
}

function setup(api: WizardApi, source: StoreCheckSource = SOURCE) {
  const recordCheck = vi.fn(async () => undefined);
  const deps: StoreCheckDeps = { wizard: api, source: vi.fn(async () => source), recordCheck, timings: TIMINGS, clock: fakeClock() };
  const events: ProgressEvent[] = [];
  const run = (signal: AbortSignal = new AbortController().signal) => runStoreCheck('nimbus', signal, (e) => events.push(e), deps);
  return { deps, recordCheck, events, run };
}

// ---- the catalog, read -------------------------------------------------------------------------------------------

describe('reading the catalog entry', () => {
  it('takes the expectation files by capture name and ignores the rest', () => {
    expect(expectsFrom(SOURCE.fixtures)).toEqual({ tui_idle: EXPECT_IDLE });
    expect(expectsFrom([{ name: 'x.expect.toml', text: '  ' }])).toEqual({});
  });

  it('finds the first program name in the [binary] table only', () => {
    expect(firstBinaryName(TOML)).toBe('nimbus');
    expect(firstBinaryName('[agent]\nnames = ["wrong"]\n[binary]\nnames = ["qwen"]\n[tui]\nargv = []')).toBe('qwen');
    expect(firstBinaryName('[agent]\nid = "x"')).toBeNull();
    expect(firstBinaryName('[binary]\nnames = ["a b; rm"]')).toBeNull();
    expect(firstBinaryName('[binary]\nnames = ["..\\\\evil"]')).toBeNull();
  });

  it('keeps paths and long text out of a message', () => {
    expect(plain('It failed in C:\\Users\\you\\x\\a.json and ~/b/c.txt')).toBe('It failed in <path> and <path>');
    expect(plain('x'.repeat(500), 50)).toHaveLength(50);
  });
});

// ---- the stand-in writer ---------------------------------------------------------------------------------------------

describe('makeStoreAsk', () => {
  const draftPrompt = 'You write agent definitions. Facts: ...';

  it('round 1 of the draft is the catalog definition and one expect block per capture, in the wizard reply format', async () => {
    const realAsk = vi.fn(async () => 'never');
    const ask = makeStoreAsk({ catalogToml: TOML, expects: { tui_idle: EXPECT_IDLE, tui_reply: '[expect]\nx = 1' }, realAsk });
    const reply = await ask(draftPrompt, 1);
    expect(realAsk).not.toHaveBeenCalled();
    expect(reply).toContain(`${F}toml\n${TOML.trim()}\n${F}`);
    const read = extractDraft(reply);
    expect(read).toEqual({ ok: true, toml: TOML.trim(), expects: { tui_idle: EXPECT_IDLE, tui_reply: '[expect]\nx = 1' } });
  });

  it('a starter has no expect blocks', async () => {
    const read = extractDraft(await makeStoreAsk({ catalogToml: TOML, expects: {} })(draftPrompt, 1));
    expect(read).toEqual({ ok: true, toml: TOML.trim(), expects: {} });
  });

  it('a screen look goes to the real writer, or is unknown when there is none', async () => {
    const look = buildClassifierPrompt('nimbus', 'NIMBUS');
    const menu = buildMenuPrompt('nimbus', 'NIMBUS', [], null);
    const realAsk = vi.fn(async () => 'working');
    const withWriter = makeStoreAsk({ catalogToml: TOML, expects: {}, realAsk });
    expect(await withWriter(look, 0)).toBe('working');
    expect(await withWriter(menu, 0)).toBe('working');
    expect(realAsk).toHaveBeenCalledTimes(2);
    const alone = makeStoreAsk({ catalogToml: TOML, expects: {} });
    expect(await alone(look, 0)).toBe('unknown');
    expect(await alone(menu, 0)).toBe('unknown');
  });

  it('a repair round goes to the real writer, else it throws NO_DRAFTER', async () => {
    const realAsk = vi.fn(async () => 'fixed');
    expect(await makeStoreAsk({ catalogToml: TOML, expects: {}, realAsk })(draftPrompt, 2)).toBe('fixed');
    expect(realAsk).toHaveBeenCalledWith(draftPrompt, 2);
    await expect(makeStoreAsk({ catalogToml: TOML, expects: {} })(draftPrompt, 2)).rejects.toThrow(/^NO_DRAFTER: /);
  });
});

// ---- the whole run -----------------------------------------------------------------------------------------------------

describe('runStoreCheck', () => {
  it('a catalog draft that passes gives the report, records the numbers once, and writes and approves nothing', async () => {
    const api = fakeApi();
    const t = setup(api);
    const out = await t.run();
    expect(out).toEqual({ status: 'done', passed: 7, total: 7, version: '0.4.1' });
    expect(t.recordCheck).toHaveBeenCalledTimes(1);
    expect(t.recordCheck).toHaveBeenCalledWith('nimbus', 7, 7, '0.4.1');
    // the wizard session is for the first program name, with "answer setup questions" and "Finish without me"
    expect(api.start).toHaveBeenCalledWith('nimbus', true, true);
    // the draft the checker saw is the catalog definition with its expectations
    expect(api.verify).toHaveBeenCalledWith('wz_1', TOML.trim(), { tui_idle: EXPECT_IDLE });
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.captureDump).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(api.cancel).toHaveBeenCalledWith('wz_1');
    expect(t.events.map((e) => `${e.step}:${e.status}`)).toEqual(['capture:running', 'capture:done', 'draft:running', 'draft:done', 'check:running', 'check:done', 'visible:running', 'visible:done']);
  });

  it('a sign-in screen stops with the message, answers nothing, and records nothing', async () => {
    const api = fakeApi({}, unsure('login'));
    api.verdict = vi.fn(async (_s: string, verdict) => ({
      decision: 'stop' as const,
      verdict,
      stop: { code: 'WIZARD_SIGN_IN_NEEDED', message: 'Sign in to nimbus (or finish its setup), then press Retry.' },
    }));
    const t = setup(api);
    const out = await t.run();
    expect(out).toEqual({ status: 'sign_in', message: 'Sign in to nimbus (or finish its setup), then press Retry.' });
    expect(api.verdict).toHaveBeenCalledWith('wz_1', 'sign_in_or_payment');
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.verify).not.toHaveBeenCalled();
    expect(api.captureAct).not.toHaveBeenCalled();
    expect(api.captureDump).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('a sign-in verdict from the writer stops the run the same way', async () => {
    const api = fakeApi({ drafters: vi.fn(async () => [{ id: 'claude', label: 'Claude Code' }]), ask: vi.fn(async () => 'sign_in_or_payment') }, unsure());
    api.verdict = vi.fn(async (_s: string, verdict) => ({
      decision: 'stop' as const,
      verdict,
      stop: { code: 'WIZARD_SIGN_IN_NEEDED', message: 'Sign in to nimbus (or finish its setup), then press Retry.' },
    }));
    const t = setup(api);
    const out = await t.run();
    expect(out.status).toBe('sign_in');
    expect(api.ask).toHaveBeenCalledWith('wz_1', 'claude', expect.stringContaining('Below is the terminal screen'));
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('with no writer installed, a screen nobody can read stops with the message for the person, and records nothing', async () => {
    const api = fakeApi({}, unsure());
    const t = setup(api);
    const out = await t.run();
    expect(out.status).toBe('stopped');
    const message = out.status === 'stopped' ? out.message : '';
    expect(message).toContain('only you may answer');
    expect(message).toContain('answer it there, then press Retry');
    expect(message).not.toContain('Type a task');
    expect(api.verdict).toHaveBeenCalledWith('wz_1', 'unknown');
    expect(api.ask).not.toHaveBeenCalled();
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('a draft that fails the checker is repaired by a real writer when there is one', async () => {
    const verify = vi.fn().mockResolvedValueOnce(verifyBad()).mockResolvedValue(verifyOk());
    const api = fakeApi({ verify, drafters: vi.fn(async () => [{ id: 'codex', label: 'Codex' }]), ask: vi.fn(async () => `${F}toml\n${TOML}\n${F}`) });
    const t = setup(api);
    const out = await t.run();
    expect(out).toMatchObject({ status: 'done', passed: 7, total: 7 });
    expect(api.ask).toHaveBeenCalledTimes(1);
    expect(api.ask).toHaveBeenCalledWith('wz_1', 'codex', expect.stringContaining('SCREEN-TEXT'));
    expect(t.recordCheck).toHaveBeenCalledTimes(1);
  });

  it('without a writer, a draft that fails the checker stops with NO_DRAFTER wording and none of the checker text', async () => {
    const api = fakeApi({ verify: vi.fn(async () => verifyBad()) });
    const t = setup(api);
    const out = await t.run();
    expect(out).toEqual({ status: 'stopped', message: NO_DRAFTER_MESSAGE });
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('a failed visible test gives a short message without the failing screen or paths', async () => {
    const bad: VisibleTestResult = { ...visibleOk(), ok: false, text: 'The visible test found 1 problem. SCREEN C:\\Users\\you\\secret.txt', errors: [] };
    const api = fakeApi({ visibletest: vi.fn(async () => bad), drafters: vi.fn(async () => [{ id: 'claude', label: 'Claude Code' }]), ask: vi.fn(async () => `${F}toml\n${TOML}\n${F}`) });
    const t = setup(api);
    const out = await t.run();
    expect(out.status).toBe('stopped');
    const message = out.status === 'stopped' ? out.message : '';
    expect(message).toContain('No working profile');
    expect(message).not.toContain('SCREEN');
    expect(message).not.toContain('Users');
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
  });

  it('a built-in agent has no definition: nothing starts', async () => {
    const api = fakeApi();
    const t = setup(api, { definition: null, fixtures: [] });
    const out = await t.run();
    expect(out.status).toBe('stopped');
    expect(api.start).not.toHaveBeenCalled();
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it('an error while starting is a plain stopped result, and the session (if any) is cancelled', async () => {
    const api = fakeApi({ probe: vi.fn(async () => { throw new Error('WIZARD_NOT_FOUND: nimbus was not found in C:\\Users\\you\\bin'); }) });
    const t = setup(api);
    const out = await t.run();
    expect(out).toEqual({ status: 'stopped', message: 'nimbus was not found in <path>' });
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(t.recordCheck).not.toHaveBeenCalled();
  });

  it('a finished check whose numbers cannot be saved still shows the report, with a note', async () => {
    const api = fakeApi();
    const t = setup(api);
    t.recordCheck.mockRejectedValueOnce(new Error('disk full'));
    const out = await t.run();
    expect(out).toMatchObject({ status: 'done', passed: 7, total: 7, note: 'The result could not be saved on the card.' });
  });

  it('settles quickly when the signal aborts while the visible test is still running, cancels the session, and records nothing', async () => {
    const abort = new AbortController();
    const visibletest = vi.fn(() => new Promise<VisibleTestResult>(() => undefined));
    const api = fakeApi({ visibletest });
    const t = setup(api);
    const pending = t.run(abort.signal);
    await vi.waitFor(() => expect(visibletest).toHaveBeenCalled());
    const began = Date.now();
    abort.abort();
    const out = await pending;
    expect(Date.now() - began).toBeLessThan(1000);
    expect(out.status).toBe('stopped');
    expect(api.cancel).toHaveBeenCalledWith('wz_1');
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(t.recordCheck).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
  });

  it('closes a session whose start finished after the abort, and starts nothing in it', async () => {
    const abort = new AbortController();
    let release: (s: StartInfo) => void = () => undefined;
    const api = fakeApi({ start: vi.fn(() => new Promise<StartInfo>((r) => { release = r; })) });
    const t = setup(api);
    const pending = t.run(abort.signal);
    await vi.waitFor(() => expect(api.start).toHaveBeenCalled());
    abort.abort();
    expect((await pending).status).toBe('stopped');
    release(start);
    await vi.waitFor(() => expect(api.cancel).toHaveBeenCalledWith('wz_1'));
    expect(api.probe).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('an already aborted signal starts nothing', async () => {
    const abort = new AbortController();
    abort.abort();
    const api = fakeApi();
    const t = setup(api);
    expect((await t.run(abort.signal)).status).toBe('stopped');
    expect(api.start).not.toHaveBeenCalled();
    expect(t.recordCheck).not.toHaveBeenCalled();
  });
});

// ---- the real dependencies: the commands the app calls ------------------------------------------------------------

describe('the app wiring (real dependencies)', () => {
  it('reads the entry with agent_store_check_source and { id }, and returns what it sends', async () => {
    const payload = { definition: null, fixtures: [{ name: 'tui_idle.txt', text: 'x' }] };
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockResolvedValueOnce(payload);
    expect(await realDeps.source('qwen')).toEqual(payload);
    expect(invoke).toHaveBeenCalledWith('agent_store_check_source', { id: 'qwen' });
  });

  it('a run with the default source reads the entry first, and a built-in entry stops before any start', async () => {
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockResolvedValueOnce({ definition: null, fixtures: [] });
    const result = await runStoreCheck('qwen', new AbortController().signal, () => undefined);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('agent_store_check_source', { id: 'qwen' });
    expect(result).toEqual({ status: 'stopped', message: 'This agent has no definition that can be checked.' });
  });

  it('records the numbers with agent_store_record_check and the argument names of agent-store.ts', async () => {
    vi.mocked(invoke).mockClear();
    await realDeps.recordCheck('qwen', 7, 7, '0.26.2');
    expect(invoke).toHaveBeenCalledWith('agent_store_record_check', { id: 'qwen', passed: 7, total: 7, version: '0.26.2' });
  });
});
