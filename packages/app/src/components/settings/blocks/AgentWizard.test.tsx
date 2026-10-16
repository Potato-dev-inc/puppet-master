// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  FinishResult,
  LiveResult,
  OpenRouterSupport,
  ProbeInfo,
  StartInfo,
  VerifyResult,
  WizardApi,
} from '../../../lib/agentWizard';
import { AgentWizard } from './AgentWizard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const F = '`'.repeat(3);
const GOOD = 'schema_version = 1\n# GOOD\n';
const BAD = 'schema_version = 1\n# BAD\n';
const CHECKER = 'The checker found 1 problem.\n\n1. [validate] AGENT_DEF_INCONSISTENT at `headless.output.result.path`\n';

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'fake-agent',
  id_suggestion: 'fake_agent',
  program: { path: 'C:\\Users\\me\\fake-agent.exe', display: '~\\fake-agent.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'fake-agent 1.4.2', number: '1.4.2' },
  native_candidates: [],
  cost_note: 'The live test runs fake-agent 3 times with tiny messages (about 3 small messages per try). It uses your own fake-agent account or plan.',
  limits: {},
};

const probe: ProbeInfo = {
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text: 'Usage: fake-agent <COMMAND>', exit: 0, timed_out: false, truncated: false, bytes: 27, duplicate_of: null }],
  subcommands: [],
  skipped: [],
};

const verifyOk = (): VerifyResult => ({
  ok: true,
  state: 'verified',
  errors: [],
  text: '',
  warnings: ['The command passes `--yolo`, which usually skips every confirmation.'],
  rendered: { headless_argv: ['run', '--format', 'json', '--', '<task>'], resume_argv: null },
  checked: { fixtures: 1, rules: 8 },
  samples_replayed: 0,
  sha256: 'abc',
});

const verifyBad = (): VerifyResult => ({ ...verifyOk(), ok: false, errors: [{ stage: 'validate', code: 'AGENT_DEF_INCONSISTENT', path: 'x', line: 1, column: 1, message: 'm', fix: 'f' }], text: CHECKER });

const liveOk = (): LiveResult => ({
  ok: true,
  state: 'tested',
  stages: [
    { name: 'pong', label: 'Say pong', status: 'passed', args: [], exit: 0, duration_ms: 1, timed_out: false, result: 'pong', session: 's', error_code: null, problems: [], note: null, stdout: '', stderr: '' },
    { name: 'resume', label: 'Continue the same conversation', status: 'passed', args: [], exit: 0, duration_ms: 1, timed_out: false, result: 'pong2', session: 's', error_code: null, problems: [], note: null, stdout: '', stderr: '' },
    { name: 'bad_model', label: 'Wrong model name is an error', status: 'skipped', args: [], exit: null, duration_ms: 0, timed_out: false, result: null, session: null, error_code: null, problems: [], note: 'the draft has no model option', stdout: '', stderr: '' },
  ],
  errors: [],
  text: '',
  tested_version: '1.4.2',
  fixtures: ['headless_success.jsonl', 'headless_resume.jsonl', 'headless_garbage.txt'],
});

const finishOk = (): FinishResult => ({
  ok: true,
  id: 'fake_agent',
  file: 'C:/data/agents/fake_agent.toml',
  fixtures_dir: 'C:/data/agents/fake_agent',
  provenance_file: 'C:/data/agents/fake_agent.wizard.json',
  state: 'needs_approval',
  fixtures: { passed: 3, total: 3 },
});

type MockApi = WizardApi & { [K in keyof WizardApi]: ReturnType<typeof vi.fn> };

function mockApi(replies: string[] = [`${F}toml\n${GOOD}${F}`]): MockApi {
  let n = 0;
  return {
    start: vi.fn(async () => start),
    probe: vi.fn(async () => probe),
    verify: vi.fn(async (_s: string, toml: string) => (toml.includes('GOOD') ? verifyOk() : verifyBad())),
    livetest: vi.fn(async () => liveOk()),
    finish: vi.fn(async () => finishOk()),
    cancel: vi.fn(async () => ({ ok: true })),
    ask: vi.fn(async () => replies[Math.min(n++, replies.length - 1)]),
    drafters: vi.fn(async () => [
      { id: 'codex', label: 'Codex' },
      { id: 'claude', label: 'Claude Code' },
    ]),
  } as unknown as MockApi;
}

function mockOpenRouter(over: Partial<OpenRouterSupport> = {}): OpenRouterSupport & { ask: ReturnType<typeof vi.fn> } {
  return {
    models: vi.fn(async () => [{ id: 'google/gemini-2.5-flash', label: 'Gemini 2.5 Flash' }]),
    hasKey: vi.fn(async () => true),
    ask: vi.fn(async () => `${F}toml\n${GOOD}${F}`),
    ...over,
  } as never;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: WizardApi, openRouter: OpenRouterSupport, props: { onClose?: () => void; onApprove?: (d: FinishResult) => void } = {}) {
  await act(async () => {
    root.render(<AgentWizard api={api} openRouter={openRouter} onClose={props.onClose ?? (() => undefined)} onApprove={props.onApprove ?? (() => undefined)} />);
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

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

async function findIt(command = 'fake-agent') {
  await type(host.querySelector('#wizard-command') as HTMLInputElement, command);
  await click(button('Find it'));
  await flush();
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

describe('AgentWizard', () => {
  it('asks which command starts the agent, in plain words', async () => {
    await render(mockApi(), mockOpenRouter());
    expect(text()).toContain('Which command starts it?');
    expect(text()).toContain('isolated temporary folder');
    expect(text()).not.toMatch(/sandbox/i);
    expect(button('Find it')?.disabled).toBe(true);
  });

  it('shows where the program was found and its version, then asks who writes the profile', async () => {
    const api = mockApi();
    await render(api, mockOpenRouter());
    await findIt();
    expect(api.start).toHaveBeenCalledWith('fake-agent');
    expect(api.probe).toHaveBeenCalledWith('wz_1');
    const found = host.querySelector('[data-testid="wizard-found"]');
    expect(found?.textContent).toContain('Found fake-agent');
    expect(found?.textContent).toContain('fake-agent 1.4.2');
    expect(found?.textContent).toContain('~\\fake-agent.exe');
    expect(text()).toContain('Who writes the profile?');
    // Claude Code comes first and is the default even though the list had Codex first
    const radios = Array.from(host.querySelectorAll('input[type="radio"]')) as HTMLInputElement[];
    expect(radios).toHaveLength(3);
    expect(radios[0].checked).toBe(true);
    expect(host.querySelector('label')?.textContent).not.toBeNull();
    expect(text().indexOf('Claude Code')).toBeLessThan(text().indexOf('Codex'));
  });

  it('shows the error when the program is not found and keeps the command box', async () => {
    const api = mockApi();
    api.start.mockRejectedValueOnce('AGENT_NOT_INSTALLED: `nope` was not found on PATH.');
    await render(api, mockOpenRouter());
    await findIt('nope');
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('`nope` was not found on PATH.');
    expect(host.querySelector('#wizard-command')).toBeTruthy();
    expect(api.probe).not.toHaveBeenCalled();
  });

  it('drives the whole flow with a stand-in drafter: fix round, confirm, review, approve', async () => {
    const api = mockApi([`${F}toml\n${BAD}${F}`, `${F}toml\n${GOOD}${F}`]);
    const approved: FinishResult[] = [];
    await render(api, mockOpenRouter(), { onApprove: (d) => approved.push(d) });
    await findIt();
    await click(button('Start'));
    await flush();

    // the first draft failed the check and was fixed; the live test waits for a confirm
    expect(api.ask).toHaveBeenCalledTimes(2);
    expect(api.ask.mock.calls[0].slice(0, 2)).toEqual(['wz_1', 'claude']);
    expect(String(api.ask.mock.calls[1][2])).toContain(CHECKER.trim());
    const confirm = host.querySelector('[data-testid="live-confirm"]');
    expect(confirm?.textContent).toContain('3 small messages per try');
    expect(confirm?.textContent).toContain('up to 5 tries');
    expect(api.livetest).not.toHaveBeenCalled();
    const rows = Array.from(host.querySelectorAll('[data-testid="wizard-progress"] li')).map((li) => `${li.getAttribute('data-step')}:${li.getAttribute('data-status')}`);
    expect(rows).toEqual(['help:done', 'draft:done', 'check:done', 'fix:done', 'live:waiting']);
    expect(text()).toContain('Fix (try 2 of 5)');

    await click(button('Run the live test'));
    await flush();
    expect(api.livetest).toHaveBeenCalledTimes(1);

    const review = host.querySelector('[data-testid="wizard-review"]');
    expect(review?.textContent).toContain('Ready for your approval');
    expect(review?.textContent).toContain('fake-agent run --format json -- <task>');
    expect(review?.textContent).toContain('Say pong');
    expect(review?.textContent).toContain('skipped - the draft has no model option');
    expect(review?.textContent).toContain('2 live checks passed on version 1.4.2');
    expect(review?.textContent).toContain('3 saved samples');
    expect(review?.textContent).toContain('--yolo');
    expect(review?.textContent).toContain('Written after 2 tries');

    await click(button('Show TOML'));
    expect(host.querySelector('[data-testid="wizard-toml"]')?.textContent).toBe(GOOD.trim());

    await click(button('Approve'));
    expect(api.finish).toHaveBeenCalledTimes(1);
    const [session, provenance] = api.finish.mock.calls[0];
    expect(session).toBe('wz_1');
    expect(provenance).toMatchObject({ drafter: 'Claude Code (local)', drafter_kind: 'local_agent', rounds: 2, prompt_version: '1' });
    expect(approved).toHaveLength(1);
    expect(approved[0].id).toBe('fake_agent');
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it('does not run the live test when the person says not now', async () => {
    const api = mockApi();
    await render(api, mockOpenRouter());
    await findIt();
    await click(button('Start'));
    await flush();
    await click(button('Not now'));
    await flush();
    expect(api.livetest).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="wizard-stopped"]')?.textContent).toContain('Stopped before the live test.');
    expect(button('Try again')).toBeTruthy();
  });

  it('says so after five tries and shows what the checker last said', async () => {
    const api = mockApi([`${F}toml\n${BAD}${F}`]);
    await render(api, mockOpenRouter());
    await findIt();
    await click(button('Start'));
    await flush();
    expect(api.ask).toHaveBeenCalledTimes(5);
    const stopped = host.querySelector('[data-testid="wizard-stopped"]');
    expect(stopped?.textContent).toContain('No working profile after 5 tries.');
    expect(stopped?.textContent).toContain('AGENT_DEF_INCONSISTENT');
  });

  it('Discard cancels the session and closes', async () => {
    const api = mockApi();
    const closed = vi.fn();
    await render(api, mockOpenRouter(), { onClose: closed });
    await findIt();
    await click(button('Start'));
    await flush();
    await click(button('Run the live test'));
    await flush();
    await click(button('Discard'));
    expect(api.cancel).toHaveBeenCalledWith('wz_1');
    expect(closed).toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
  });

  it('Retry goes back to the writer choice and can draft again', async () => {
    const api = mockApi();
    await render(api, mockOpenRouter());
    await findIt();
    await click(button('Start'));
    await flush();
    await click(button('Run the live test'));
    await flush();
    await click(button('Retry'));
    expect(text()).toContain('Who writes the profile?');
    await click(button('Start'));
    await flush();
    expect(api.ask).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="live-confirm"]')).toBeTruthy();
  });

  it('an OpenRouter model needs a preview and an explicit opt-in before anything is sent', async () => {
    const api = mockApi();
    const or = mockOpenRouter();
    await render(api, or);
    await findIt();
    const radios = Array.from(host.querySelectorAll('input[type="radio"]')) as HTMLInputElement[];
    await click(radios[2]);
    const preview = host.querySelector('[data-testid="openrouter-preview"]');
    expect(preview?.textContent).toContain('Command typed by the person: fake-agent');
    expect(preview?.textContent).toContain('Usage: fake-agent <COMMAND>');
    expect(text()).toContain('This is exactly what will be sent to OpenRouter');
    expect(button('Start')?.disabled).toBe(true);
    await click(button('Start'));
    expect(or.ask).not.toHaveBeenCalled();
    const optIn = host.querySelector('input[type="checkbox"]') as HTMLInputElement;
    await click(optIn);
    expect(button('Start')?.disabled).toBe(false);
    await click(button('Start'));
    await flush();
    expect(or.ask).toHaveBeenCalledTimes(1);
    expect(or.ask.mock.calls[0][0]).toBe('google/gemini-2.5-flash');
    expect(api.ask).not.toHaveBeenCalled();
    // the larger remote budget: the first prompt equals the previewed text
    expect(or.ask.mock.calls[0][1]).toBe(preview?.textContent);
    await click(button('Run the live test'));
    await flush();
    await click(button('Approve'));
    expect(api.finish.mock.calls[0][1]).toMatchObject({ drafter: 'OpenRouter google/gemini-2.5-flash', drafter_kind: 'openrouter', model: 'google/gemini-2.5-flash' });
  });

  it('without an OpenRouter key the opt-in is not enough', async () => {
    const or = mockOpenRouter({ hasKey: vi.fn(async () => false) });
    await render(mockApi(), or);
    await findIt();
    await click((Array.from(host.querySelectorAll('input[type="radio"]')) as HTMLInputElement[])[2]);
    expect(text()).toContain('Add your OpenRouter key in Settings > Keys first.');
    await click(host.querySelector('input[type="checkbox"]'));
    expect(button('Start')?.disabled).toBe(true);
  });

  it('offers only OpenRouter when no local agent is installed, and says what to install when nothing is', async () => {
    const api = mockApi();
    api.drafters.mockResolvedValue([]);
    await render(api, mockOpenRouter());
    await findIt();
    expect(host.querySelectorAll('input[type="radio"]')).toHaveLength(1);
    expect((host.querySelector('input[type="radio"]') as HTMLInputElement).checked).toBe(true);
    await act(async () => root.unmount());
    root = createRoot(host);
    const empty = mockApi();
    empty.drafters.mockResolvedValue([]);
    await render(empty, mockOpenRouter({ models: vi.fn(async () => []) }));
    await findIt();
    expect(text()).toContain('No writer is available.');
    expect(button('Start')?.disabled).toBe(true);
  });

  it('shows what the program printed as text, never as HTML', async () => {
    const api = mockApi();
    api.start.mockResolvedValue({ ...start, program: { ...start.program, display: '<img src=x onerror="window.__pwned=1">' } });
    await render(api, mockOpenRouter());
    await findIt();
    expect(host.querySelector('img')).toBeNull();
    expect(text()).toContain('<img src=x onerror="window.__pwned=1">');
  });

  it('cancels the session when the dialog goes away without finishing', async () => {
    const api = mockApi();
    await render(api, mockOpenRouter());
    await findIt();
    await act(async () => root.unmount());
    expect(api.cancel).toHaveBeenCalledWith('wz_1');
    root = createRoot(host);
  });

  it('says the launcher is a script when the program is an npm wrapper', async () => {
    const api = mockApi();
    api.start.mockResolvedValue({ ...start, program: { ...start.program, kind: 'shim' } });
    await render(api, mockOpenRouter());
    await findIt();
    expect(text()).toContain('npm launcher script');
  });
});
