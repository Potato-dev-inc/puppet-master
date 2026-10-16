// @vitest-environment jsdom
// The visible path of the wizard dialog (SPEC 11.11): the capture panel, the confirmation of the pane, the
// review with "Visible pane: n of n", and the optional one-task step.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  CapturePlan,
  CaptureStatus,
  CaptureStepId,
  FinishResult,
  LiveResult,
  OpenRouterSupport,
  PlannedRun,
  PlannedVisible,
  ProbeInfo,
  StartInfo,
  VerifyResult,
  VisibleTestResult,
  WizardApi,
} from '../../../lib/agentWizard';
import type { CaptureFacts } from '../../../lib/agentWizardPrompt';
import { AgentWizard } from './AgentWizard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const F = '`'.repeat(3);
const GOOD = 'schema_version = 1\n# GOOD\n[tui]\nargv = []\n';
const BOTH = 'schema_version = 1\n# BOTH\n[tui]\nargv = []\n[headless]\nargv = ["run"]\n';

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'nimbus',
  id_suggestion: 'nimbus',
  program: { path: 'C:\\Users\\me\\nimbus.exe', display: '~\\nimbus.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'nimbus 0.4.2', number: '0.4.2' },
  native_candidates: [],
  cost_note: 'The live test runs nimbus 4 times.',
  limits: {},
};

const probeFor = (text: string): ProbeInfo => ({
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text, exit: 0, timed_out: false, truncated: false, bytes: text.length, duplicate_of: null }],
  subcommands: [],
  skipped: [],
});
const PLAIN_HELP = 'Usage: nimbus [--script FILE]\nStarts the interactive assistant.';
const ONE_TASK_HELP = `${PLAIN_HELP}\n  -p, --print   run one task non-interactively`;

const plan = (args: string[]): CapturePlan => ({
  program: { path: 'C:\\Users\\me\\nimbus.exe', display: '~\\nimbus.exe' },
  args,
  cwd: '<isolated folder>',
  grid: { cols: 120, rows: 40 },
  fingerprint: `cap-${args.join('-')}`,
});

const STEPS = (over: Partial<Record<CaptureStepId, { status: CaptureStatus['steps'] extends Array<infer S> | undefined ? S extends { status: infer T } ? T : never : never; note?: string }>> = {}): NonNullable<CaptureStatus['steps']> =>
  (
    [
      ['start', 'Starting'],
      ['ready', 'Ready'],
      ['message', 'Test message'],
      ['permission', 'Permission'],
      ['exit', 'Exit'],
    ] as Array<[CaptureStepId, string]>
  ).map(([id, label]) => ({ id, label, status: over[id]?.status ?? 'pending', note: over[id]?.note ?? '', frames: 0 }));

const running = (over: Partial<CaptureStatus> = {}): CaptureStatus => ({
  state: 'running',
  captured: false,
  pane: 'pane-1',
  grid: { cols: 120, rows: 40 },
  alive: true,
  step: 'start',
  steps: STEPS({ start: { status: 'running' } }),
  screen_kind: 'unknown',
  stable_ms: 100,
  settled: false,
  needs_you: null,
  screen: 'NIMBUS 0.4 ready',
  can: ['skip', 'retry', 'stuck'],
  ...over,
});

const captured = (): CaptureFacts => ({
  grid: { cols: 120, rows: 40 },
  files: [
    { name: 'tui_idle', text: '| > Type a task...', variants: [] },
    { name: 'tui_reply', text: 'reply: <word-1>', variants: [] },
  ],
  facts: { exit_method: 'slash_exit', partial: [], skipped: [], hello_written: false, startup_dialogs: [], exit_screen: null, notes: [] },
});

const plannedVisible = (): PlannedVisible => ({
  program: { path: 'C:\\Users\\me\\nimbus.exe', display: '~\\nimbus.exe' },
  args: [],
  env: [],
  env_unset: [],
  cwd: '<isolated folder>',
  grid: { cols: 120, rows: 40 },
  fingerprint: 'vis-A',
});

const plannedLive = (): PlannedRun => ({
  program: { path: 'C:\\Users\\me\\nimbus.exe', display: '~\\nimbus.exe' },
  stages: [{ name: 'pong', label: 'Say pong', args: ['run', '--', '<test message>'] }],
  stdin: null,
  env: [],
  env_unset: [],
  fingerprint: 'live-A',
});

const verifyOk = (toml: string): VerifyResult => ({
  ok: true,
  state: 'verified',
  errors: [],
  text: '',
  warnings: [],
  rendered: { headless_argv: toml.includes('BOTH') ? ['run', '--', '<task>'] : null, resume_argv: null, tui_argv: [] },
  checked: { fixtures: 5, rules: 8 },
  samples_replayed: 0,
  sha256: 'abc',
  planned_visible: plannedVisible(),
  can_visibletest: true,
  ...(toml.includes('BOTH') ? { planned: plannedLive(), can_livetest: true } : {}),
});

const visibleOk = (): VisibleTestResult => ({
  ok: true,
  state: 'tested',
  checks: [
    { name: 'starts', label: 'The agent starts in a pane', status: 'passed', note: '', ms: 1 },
    { name: 'ready', label: 'It reaches its empty input box', status: 'passed', note: '', ms: 1 },
    { name: 'permission', label: 'A permission question becomes a card', status: 'skipped', note: 'No permission question appeared.', ms: 0 },
  ],
  errors: [],
  text: '',
  version: '0.4.2',
  passed: 2,
  total: 2,
  agent_error: null,
});

const liveOk = (): LiveResult => ({
  ok: true,
  state: 'tested',
  stages: [
    { name: 'pong', label: 'Say pong', status: 'passed', args: [], exit: 0, duration_ms: 1, timed_out: false, result: 'pong', session: 's', error_code: null, problems: [], note: null, stdout: '', stderr: '' },
  ],
  errors: [],
  text: '',
  tested_version: '0.4.2',
  fixtures: ['headless_success.jsonl', 'headless_garbage.txt'],
});

const finishOk = (): FinishResult => ({
  ok: true,
  id: 'nimbus',
  file: 'C:/data/agents/nimbus.toml',
  fixtures_dir: 'C:/data/agents/nimbus',
  provenance_file: 'C:/data/agents/nimbus.wizard.json',
  state: 'needs_approval',
  fixtures: { passed: 5, total: 5 },
});

type MockApi = WizardApi & { [K in keyof WizardApi]: ReturnType<typeof vi.fn> };

function mockApi(help: string, replies: string[] = [`${F}toml\n${GOOD}${F}`]): MockApi {
  let n = 0;
  return {
    start: vi.fn(async () => start),
    probe: vi.fn(async () => probeFor(help)),
    verify: vi.fn(async (_s: string, toml: string) => verifyOk(toml)),
    livetest: vi.fn(async () => liveOk()),
    visibletest: vi.fn(async () => visibleOk()),
    testStatus: vi.fn(async () => ({ running: false })),
    finish: vi.fn(async () => finishOk()),
    cancel: vi.fn(async () => ({ ok: true })),
    ask: vi.fn(async () => replies[Math.min(n++, replies.length - 1)]),
    drafters: vi.fn(async () => [{ id: 'claude', label: 'Claude Code' }]),
    capturePlan: vi.fn(async (_s: string, args: string[]) => plan(args)),
    captureStart: vi.fn(async () => running()),
    captureStatus: vi.fn(async () => running()),
    captureAct: vi.fn(async () => running()),
    captureShow: vi.fn(async () => undefined),
    captureFinish: vi.fn(async () => captured()),
    captureDiscard: vi.fn(async () => undefined),
  } as unknown as MockApi;
}

const openRouter = (): OpenRouterSupport => ({ models: vi.fn(async () => []), hasKey: vi.fn(async () => false), ask: vi.fn(async () => '') }) as never;

let host: HTMLDivElement;
let root: Root;

async function render(api: WizardApi, props: { onClose?: () => void; onApprove?: (d: FinishResult) => void } = {}) {
  await act(async () => {
    root.render(<AgentWizard api={api} openRouter={openRouter()} onClose={props.onClose ?? (() => undefined)} onApprove={props.onApprove ?? (() => undefined)} />);
  });
}

async function click(el: Element | null | undefined) {
  expect(el, 'element to click').toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function type(el: HTMLInputElement, value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
    setter?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const button = (label: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);
const text = () => host.textContent ?? '';

async function flush(ms = 0) {
  await act(async () => {
    await new Promise((r) => setTimeout(r, ms));
  });
}

async function findIt() {
  await type(host.querySelector('#wizard-command') as HTMLInputElement, 'nimbus');
  await click(button('Find it'));
  await flush();
}

async function startCapture(api: MockApi) {
  await findIt();
  await click(button('Start'));
  await flush();
  expect(api.captureStart).toHaveBeenCalled();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('the capture panel', () => {
  it('shows the exact command, the isolated folder and the grid before anything starts', async () => {
    const api = mockApi(PLAIN_HELP);
    await render(api);
    await findIt();
    expect(api.capturePlan).toHaveBeenCalledWith('wz_1', []);
    expect(host.querySelector('[data-testid="capture-command"]')?.textContent).toBe('C:\\Users\\me\\nimbus.exe');
    expect(text()).toContain('isolated temporary folder');
    expect(text()).toContain('120 wide and 40 tall');
    expect(text()).toContain('You answer its questions in the pane yourself');
    expect(text()).not.toMatch(/sandbox/i);
    expect(api.captureStart).not.toHaveBeenCalled();
  });

  it('offers "Skip the visible part" only when the help shows a one-task mode', async () => {
    const without = mockApi(PLAIN_HELP);
    await render(without);
    await findIt();
    expect(button('Skip the visible part')).toBeUndefined();
    expect(text()).toContain('It shows no one-task mode, so the pane is the way in.');
    await act(async () => root.unmount());
    root = createRoot(host);
    const withOne = mockApi(ONE_TASK_HELP);
    await render(withOne);
    await findIt();
    expect(button('Skip the visible part')).toBeTruthy();
  });

  it('extra arguments change the command that is shown, and Start sends the fingerprint of what was shown', async () => {
    const api = mockApi(PLAIN_HELP);
    await render(api);
    await findIt();
    await type(host.querySelector('#capture-args') as HTMLInputElement, '--model "small one"');
    await flush();
    expect(api.capturePlan).toHaveBeenLastCalledWith('wz_1', ['--model', 'small one']);
    expect(host.querySelector('[data-testid="capture-command"]')?.textContent).toBe('C:\\Users\\me\\nimbus.exe --model "small one"');
    await click(button('Start'));
    await flush();
    expect(api.captureStart).toHaveBeenCalledWith('wz_1', ['--model', 'small one'], 'cap---model-small one');
    expect(host.querySelector('[data-testid="capture-running"]')).toBeTruthy();
  });

  it('shows the steps, says when it is the person\'s turn, and shows the pane as text only', async () => {
    const api = mockApi(PLAIN_HELP);
    api.captureStart.mockResolvedValue(running());
    api.captureStatus.mockResolvedValue(
      running({
        steps: STEPS({ start: { status: 'needs_you', note: 'asking' } }),
        needs_you: { step: 'start', kind: 'trust', message: 'The agent is asking whether you trust this folder. Answer it in the pane yourself. The wizard presses nothing.' },
        screen: 'Do you trust the files in this folder? <b>bold</b>',
        can: ['skip', 'retry', 'stuck'],
      }),
    );
    await render(api);
    await startCapture(api);
    await flush(900);
    const needs = host.querySelector('[data-testid="capture-needs-you"]');
    expect(needs?.textContent).toContain('Answer it in the pane yourself');
    expect(needs?.textContent).toContain('presses nothing');
    const steps = Array.from(host.querySelectorAll('[data-testid="capture-steps"] li')).map((li) => `${li.getAttribute('data-step')}:${li.getAttribute('data-status')}`);
    expect(steps).toEqual(['start:needs_you', 'ready:pending', 'message:pending', 'permission:pending', 'exit:pending']);
    const screen = host.querySelector('[data-testid="capture-screen"]');
    expect(screen?.textContent).toContain('Do you trust the files in this folder? <b>bold</b>');
    expect(screen?.querySelector('b')).toBeNull();
    // while a question is up there is no "It is ready" button
    expect(button('It is ready')).toBeUndefined();
  });

  it('the buttons send the person\'s choice for the current step', async () => {
    const api = mockApi(PLAIN_HELP);
    api.captureStatus.mockResolvedValue(running({ step: 'ready', steps: STEPS({ start: { status: 'done' }, ready: { status: 'running' } }), can: ['skip', 'retry', 'stuck', 'ready'] }));
    await render(api);
    await startCapture(api);
    await flush(900);
    await click(button('It is ready'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'ready', undefined);
    await click(button('It looks stuck'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'stuck', undefined);
    await click(button('Skip'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'skip', undefined);
    await click(button('Retry'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'retry', undefined);
    await click(button('Show the pane'));
    expect(api.captureShow).toHaveBeenCalledWith('wz_1');
  });

  it('a question the wizard does not recognise offers "It is done" and "It is a question"', async () => {
    const api = mockApi(PLAIN_HELP);
    const odd = running({
      step: 'permission',
      steps: STEPS({ start: { status: 'done' }, ready: { status: 'done' }, message: { status: 'done' }, permission: { status: 'needs_you' } }),
      needs_you: { step: 'permission', kind: 'question', message: 'The screen has stopped changing and it is not the agent\'s empty input box. If the agent has finished, press "It is done".' },
      can: ['skip', 'retry', 'stuck', 'ready', 'done', 'question'],
    });
    api.captureStatus.mockResolvedValue(odd);
    api.captureAct.mockResolvedValue(odd);
    await render(api);
    await startCapture(api);
    await flush(900);
    expect(host.querySelector('[data-testid="capture-needs-you"]')?.textContent).toContain('It is done');
    await click(button('It is done'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'done', undefined);
    await click(button('It is a question'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'question', undefined);
  });

  it('a step that took too long says so and offers Skip and Retry; "What the wizard saw" shows the last looks', async () => {
    const api = mockApi(PLAIN_HELP);
    const stuck = running({
      step: 'permission',
      steps: STEPS({ permission: { status: 'needs_you' } }),
      needs_you: { step: 'permission', kind: 'stuck', message: 'This step has taken a long time and is not finished. Press Skip to go on without it, or Retry to try it again.' },
      can: ['skip', 'retry', 'stuck'],
    });
    api.captureStatus.mockResolvedValue(stuck);
    api.captureDebug = vi.fn(async () => ({
      at_ms: 130000,
      frames: [
        { ms: 120000, step: 'permission', step_status: 'running', classifier: 'working', reading: 'frozen', stable_ms: 30000, quiet_ms: 30000, needs_you: null, screen: 'Working...\n| > Type a task...' },
      ],
    })) as never;
    await render(api);
    await startCapture(api);
    await flush(900);
    expect(host.querySelector('[data-testid="capture-needs-you"]')?.textContent).toContain('Press Skip');
    expect(host.querySelector('[data-testid="capture-saw-screen"]')).toBeNull();
    await click(button('What the wizard saw'));
    await flush();
    const lines = host.querySelector('[aria-label="The last looks"]')?.textContent ?? '';
    expect(lines).toContain('read as working, then frozen');
    expect(lines).toContain('nothing moved for 30 s');
    expect(host.querySelector('[data-testid="capture-saw-screen"]')?.textContent).toContain('Working...');
  });

  it('the exit step offers /exit and "I closed it myself"', async () => {
    const api = mockApi(PLAIN_HELP);
    const atExit = running({ step: 'exit', steps: STEPS({ permission: { status: 'done' }, exit: { status: 'running' } }), can: ['skip', 'retry', 'stuck', 'try_exit', 'closed'] });
    api.captureStatus.mockResolvedValue(atExit);
    api.captureAct.mockResolvedValue(atExit);
    await render(api);
    await startCapture(api);
    await flush(1500);
    await click(button('Try /exit'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'try_exit', undefined);
    await click(button('I closed it myself'));
    expect(api.captureAct).toHaveBeenLastCalledWith('wz_1', 'closed', undefined);
  });

  it('finishing hands the screens to the writer step; an empty capture stays on the panel', async () => {
    const api = mockApi(PLAIN_HELP);
    api.captureFinish.mockResolvedValueOnce({ ...captured(), files: [] }).mockResolvedValueOnce(captured());
    await render(api);
    await startCapture(api);
    await click(button('Use what was captured so far'));
    await flush();
    expect(text()).toContain('Nothing was captured');
    expect(host.querySelector('[data-testid="capture-plan"]')).toBeTruthy();
    await click(button('Start'));
    await flush();
    await click(button('Use what was captured so far'));
    await flush();
    expect(host.querySelector('[data-testid="captured-note"]')?.textContent).toContain('Captured 2 screens of nimbus');
    expect(text()).toContain('Who writes the profile?');
  });

  it('cancelling closes the pane and the session', async () => {
    const api = mockApi(PLAIN_HELP);
    const onClose = vi.fn();
    await render(api, { onClose });
    await startCapture(api);
    await click(button('Cancel'));
    await flush();
    expect(api.captureDiscard).toHaveBeenCalledWith('wz_1');
    expect(api.cancel).toHaveBeenCalledWith('wz_1');
    expect(onClose).toHaveBeenCalled();
  });
});

describe('the visible flow', () => {
  async function throughCapture(api: MockApi) {
    await startCapture(api);
    await click(button('Use what was captured so far'));
    await flush();
    await click(button('Start'));
    await flush(10);
  }

  it('drafts from the screens, asks before it starts the pane, runs the visible test and shows "n of n"', async () => {
    const api = mockApi(PLAIN_HELP);
    await render(api);
    await throughCapture(api);
    // the writer was given the captures (masked) and the checker got the reply's expectation blocks
    expect(String(api.ask.mock.calls[0][2])).toContain('<<<CAPTURE tui_idle');
    expect(api.verify).toHaveBeenCalled();
    const confirm = host.querySelector('[data-testid="visible-confirm"]');
    expect(confirm?.textContent).toContain('Ready for the visible test');
    expect(confirm?.textContent).toContain('the wizard answers nothing');
    expect(host.querySelector('[data-testid="planned-pane"]')?.textContent).toContain('C:\\Users\\me\\nimbus.exe');
    expect(host.querySelector('[data-testid="planned-pane"]')?.textContent).toContain('120 wide and 40 tall');
    expect(api.visibletest).not.toHaveBeenCalled();
    await click(button('Run the visible test'));
    await flush(10);
    expect(api.visibletest).toHaveBeenCalledWith('wz_1', 'vis-A');
    const review = host.querySelector('[data-testid="wizard-review"]');
    expect(review?.textContent).toContain('Visible pane: 2 of 2 checks passed');
    expect(review?.textContent).toContain('skipped - No permission question appeared.');
    expect(review?.textContent).toContain('One-task mode: not added');
    expect(review?.textContent).toContain('The captured screens will be kept next to the definition');
    expect(host.querySelector('[data-testid="one-task-offer"]')).toBeNull();
    let provenance: unknown;
    api.finish.mockImplementationOnce(async (_s: string, p: unknown) => {
      provenance = p;
      return finishOk();
    });
    await click(button('Approve'));
    await flush();
    expect(provenance).toMatchObject({ drafter: 'Claude Code (local)', prompt_version: '5', rounds: 1 });
  });

  it('the Review says when the agent writes files without asking', async () => {
    const api = mockApi(PLAIN_HELP);
    api.captureFinish.mockResolvedValue({ ...captured(), facts: { ...captured().facts, hello_written: true, writes_without_asking: true } });
    await render(api);
    await throughCapture(api);
    await click(button('Run the visible test'));
    await flush(10);
    expect(host.querySelector('[data-testid="review-writes-without-asking"]')?.textContent).toContain('writes files without asking');
    // the writer was told too
    expect(String(api.ask.mock.calls[0][2])).toContain('This agent writes files without asking');
  });

  it('"Not now" stops before the pane is started', async () => {
    const api = mockApi(PLAIN_HELP);
    await render(api);
    await throughCapture(api);
    await click(button('Not now'));
    await flush(10);
    expect(api.visibletest).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="wizard-stopped"]')?.textContent).toContain('Stopped before the test.');
  });

  it('offers the one-task mode as an extra when the help shows one, and adds it on top of the visible file', async () => {
    const api = mockApi(ONE_TASK_HELP, [`${F}toml\n${GOOD}${F}`, `${F}toml\n${BOTH}${F}`]);
    await render(api);
    await throughCapture(api);
    await click(button('Run the visible test'));
    await flush(10);
    const offer = host.querySelector('[data-testid="one-task-offer"]');
    expect(offer?.textContent).toContain('Also add a one-task mode (optional)');
    await click(button('Add the one-task mode'));
    await flush(10);
    // the second prompt keeps the visible file and asks only for [headless]
    expect(String(api.ask.mock.calls[1][2])).toContain('The file to keep');
    expect(String(api.ask.mock.calls[1][2])).toContain('Now add the [headless] tables');
    // both plans are shown to the person before anything runs; the pane plan was confirmed already
    expect(host.querySelector('[data-testid="live-confirm"]')).toBeTruthy();
    expect(host.querySelector('[data-testid="visible-confirm"]')).toBeNull();
    await click(button('Run the live test'));
    await flush(10);
    const review = host.querySelector('[data-testid="wizard-review"]');
    expect(review?.textContent).toContain('Visible pane: 2 of 2 checks passed');
    expect(review?.textContent).toContain('One-task mode: 1 of 1 checks passed');
    expect(host.querySelector('[data-testid="one-task-offer"]')).toBeNull();
  });

  it('a failed one-task step keeps the visible profile', async () => {
    const api = mockApi(ONE_TASK_HELP, [`${F}toml\n${GOOD}${F}`, `${F}toml\n${BOTH}${F}`]);
    await render(api);
    await throughCapture(api);
    await click(button('Run the visible test'));
    await flush(10);
    api.livetest.mockResolvedValue({ ...liveOk(), ok: false, text: 'It failed.', errors: [{ stage: 'livetest', code: 'X', path: '', line: null, column: null, message: 'm', fix: '' }] });
    await click(button('Add the one-task mode'));
    await flush(10);
    await click(button('Run the live test'));
    await flush(10);
    // five tries run out: the writer keeps answering with the same file
    await flush(50);
    expect(host.querySelector('[data-testid="wizard-stopped"]')).toBeTruthy();
    await click(button('Keep it without a one-task mode'));
    await flush();
    expect(host.querySelector('[data-testid="wizard-review"]')?.textContent).toContain('One-task mode: not added');
  });

  it('skipping the visible part is the old headless flow and shows no visible lines', async () => {
    const api = mockApi(ONE_TASK_HELP);
    api.verify.mockImplementation(async () => ({ ...verifyOk(BOTH), planned_visible: undefined, can_visibletest: undefined }));
    await render(api);
    await findIt();
    await click(button('Skip the visible part'));
    await flush();
    expect(host.querySelector('[data-testid="headless-only-note"]')).toBeTruthy();
    expect(api.captureStart).not.toHaveBeenCalled();
    await click(button('Start'));
    await flush(10);
    await click(button('Run the live test'));
    await flush(10);
    const review = host.querySelector('[data-testid="wizard-review"]');
    expect(review?.textContent).not.toContain('Visible pane');
    expect(review?.textContent).toContain('Tested live on nimbus 0.4.2: 1 of 1 checks passed.');
    expect(api.visibletest).not.toHaveBeenCalled();
  });
});
