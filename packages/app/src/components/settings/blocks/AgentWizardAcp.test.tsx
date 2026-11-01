// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FinishResult, OpenRouterSupport, PlannedRun, ProbeInfo, StartInfo, VerifyResult, WizardApi, LiveResult } from '../../../lib/agentWizard';
import type { AcpDetectResult } from '../../../lib/agentWizardAcp';
import { AgentWizard } from './AgentWizard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const F = '`'.repeat(3);
const GOOD = 'schema_version = 1\n# GOOD\n';

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'fake-agent',
  id_suggestion: 'fake_agent',
  program: { path: 'C:\\tools\\fake-agent.exe', display: '~\\fake-agent.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'fake-agent 1.4.2', number: '1.4.2' },
  native_candidates: [],
  cost_note: 'The live test runs fake-agent 3 times.',
  limits: {},
};

const probe: ProbeInfo = {
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text: 'Usage: fake-agent <COMMAND>\n  run  Run one task non-interactively\n  acp  Start the <b>chat</b> server', exit: 0, timed_out: false, truncated: false, bytes: 27, duplicate_of: null }],
  subcommands: [],
  skipped: [],
};

const planned: PlannedRun = {
  program: { path: 'C:\\tools\\fake-agent.exe', display: '~\\fake-agent.exe' },
  stages: [{ name: 'pong', label: 'Say pong', args: ['run', '--', '<test message>'] }],
  stdin: null,
  env: [],
  env_unset: [],
  fingerprint: 'plan-A',
};

const verifyOk = (): VerifyResult =>
  ({
    ok: true,
    state: 'verified',
    errors: [],
    text: '',
    warnings: [],
    rendered: { headless_argv: ['run', '--', '<task>'], resume_argv: null },
    checked: { fixtures: 1, rules: 8 },
    samples_replayed: 0,
    sha256: 'abc',
    planned,
  }) as VerifyResult;

const liveOk = (): LiveResult =>
  ({
    ok: true,
    state: 'tested',
    stages: [{ name: 'pong', label: 'Say pong', status: 'passed', args: [], exit: 0, duration_ms: 1, timed_out: false, result: 'pong', session: 's', error_code: null, problems: [], note: null, stdout: '', stderr: '' }],
    errors: [],
    text: '',
    tested_version: '1.4.2',
    fixtures: ['a.jsonl'],
  }) as LiveResult;

const finishOk = (): FinishResult => ({
  ok: true,
  id: 'fake_agent',
  file: 'C:/data/agents/fake_agent.toml',
  fixtures_dir: 'C:/data/agents/fake_agent',
  provenance_file: 'C:/data/agents/fake_agent.wizard.json',
  state: 'needs_approval',
  fixtures: { passed: 0, total: 0 },
});

const detected: AcpDetectResult = {
  ok: true,
  command: 'fake-agent',
  candidates: [
    { command: 'fake-agent', args: ['acp'], evidence: 'acp  Start the <b>chat</b> server', confidence: 'high' },
    { command: 'fake-acp-bridge', args: ['--stdio', '--quiet'], evidence: 'fake-acp-bridge  ACP over stdio', confidence: 'medium' },
  ],
};

type MockApi = WizardApi & { [K in keyof WizardApi]: ReturnType<typeof vi.fn> };

function mockApi(over: Record<string, unknown> = {}): MockApi {
  return {
    start: vi.fn(async () => start),
    probe: vi.fn(async () => probe),
    verify: vi.fn(async () => verifyOk()),
    livetest: vi.fn(async () => liveOk()),
    finish: vi.fn(async () => finishOk()),
    acpDetect: vi.fn(async () => detected),
    acpFinish: vi.fn(async () => finishOk()),
    cancel: vi.fn(async () => ({ ok: true })),
    ask: vi.fn(async () => `${F}toml\n${GOOD}${F}`),
    drafters: vi.fn(async () => [{ id: 'claude', label: 'Claude' }]),
    visibletest: vi.fn(async () => {
      throw new Error('no visible test here');
    }),
    testStatus: vi.fn(async () => ({ running: false })),
    capturePlan: vi.fn(),
    captureStart: vi.fn(),
    captureStatus: vi.fn(),
    captureAct: vi.fn(),
    captureShow: vi.fn(),
    captureFinish: vi.fn(),
    captureDiscard: vi.fn(async () => undefined),
    ...over,
  } as unknown as MockApi;
}

const openRouter = {
  models: vi.fn(async () => []),
  hasKey: vi.fn(async () => false),
  ask: vi.fn(),
} as unknown as OpenRouterSupport;

let host: HTMLDivElement;
let root: Root;

async function render(api: WizardApi, onApprove: (d: FinishResult) => void = () => undefined) {
  await act(async () => {
    root.render(<AgentWizard api={api} openRouter={openRouter} onClose={() => undefined} onApprove={onApprove} />);
  });
}

async function click(el: Element | null | undefined) {
  expect(el, 'element to click').toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function setValue(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const button = (label: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);
const text = () => host.textContent ?? '';
const field = (id: string) => host.querySelector(`#${id}`) as HTMLInputElement & HTMLTextAreaElement;
const kindRadio = (label: string) =>
  Array.from(host.querySelectorAll('input[name="wizard-kind"]')).find((r) => r.closest('label')?.textContent?.startsWith(label)) as HTMLInputElement;

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

async function findIt(kind?: 'Chat (ACP)' | 'Both') {
  if (kind) await click(kindRadio(kind));
  await setValue(field('wizard-command'), 'fake-agent');
  await click(button('Find it'));
  await flush();
}

beforeEach(() => {
  globalThis.localStorage.setItem('pm.agentWizard.finishWithoutMe', 'off');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  globalThis.localStorage.removeItem('pm.agentWizard.finishWithoutMe');
  await act(async () => root.unmount());
  host.remove();
});

describe('AgentWizard: chat (ACP)', () => {
  it('defaults to terminal only and the terminal flow is unchanged', async () => {
    const api = mockApi();
    await render(api);
    expect(kindRadio('Terminal').checked).toBe(true);
    expect(kindRadio('Chat (ACP)').checked).toBe(false);
    await findIt();
    expect(api.start).toHaveBeenCalledWith('fake-agent', true);
    expect(api.acpDetect).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="acp-step"]')).toBeNull();
    await click(button('Skip the visible part'));
    await flush();
    await click(button('Start'));
    await flush();
    await click(button('Run the live test'));
    await flush();
    // no [acp] block anywhere in what was checked, and no chat table in the review
    expect(String(api.verify.mock.calls[0][1])).toBe(GOOD.trimEnd());
    expect(host.querySelector('[data-testid="review-acp"]')).toBeNull();
    await click(button('Approve'));
    expect(api.finish).toHaveBeenCalledTimes(1);
    expect(api.acpFinish).not.toHaveBeenCalled();
  });

  it('chat only: detects, shows the table, finishes with acp_finish and never captures, drafts or verifies', async () => {
    const api = mockApi();
    const approved: FinishResult[] = [];
    await render(api, (d) => approved.push(d));
    await findIt('Chat (ACP)');
    expect(api.start).toHaveBeenCalledWith('fake-agent', false);
    expect(api.probe).toHaveBeenCalledWith('wz_1');
    expect(api.acpDetect).toHaveBeenCalledWith('wz_1');
    expect(host.querySelector('[data-testid="acp-step"]')).toBeTruthy();
    // the first (high confidence) candidate is picked and fills the fields
    expect(field('acp-command').value).toBe('fake-agent');
    expect(field('acp-args').value).toBe('acp');

    await setValue(field('acp-env'), 'HOME\nFAKE_KEY');
    await setValue(field('acp-resume'), '--resume\n{session}');
    await click(button('Review'));
    const table = host.querySelector('[data-testid="acp-table"]')?.textContent ?? '';
    expect(table).toContain('Command:        fake-agent');
    expect(table).toContain('Arguments:      acp');
    expect(table).toContain('HOME  FAKE_KEY');
    expect(table).toContain('--resume {session}');

    await click(button('Approve'));
    expect(api.acpFinish).toHaveBeenCalledTimes(1);
    expect((api.acpFinish as MockApi['acpFinish'] & ReturnType<typeof vi.fn>).mock.calls[0][0]).toEqual({
      session: 'wz_1',
      label: 'Fake-agent',
      acp: { command: 'fake-agent', args: ['acp'], env_allow: ['HOME', 'FAKE_KEY'], resume_command: ['--resume', '{session}'] },
    });
    expect(approved).toHaveLength(1);
    for (const name of ['captureStart', 'capturePlan', 'ask', 'verify', 'livetest', 'visibletest', 'finish', 'drafters'] as const) {
      expect(api[name], name).not.toHaveBeenCalled();
    }
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it('shows what the program printed as text, not as HTML, and lets the person pick a candidate or enter it', async () => {
    const api = mockApi();
    await render(api);
    await findIt('Chat (ACP)');
    expect(host.querySelector('[data-testid="acp-step"] b')).toBeNull();
    expect(text()).toContain('From the help: acp  Start the <b>chat</b> server');
    expect(text()).toContain('Looks like a good match.');
    expect(text()).toContain('A possible match. Check it before you go on.');
    const radios = Array.from(host.querySelectorAll('input[name="wizard-acp-pick"]')) as HTMLInputElement[];
    expect(radios).toHaveLength(3);
    expect(radios.map((r) => r.checked)).toEqual([true, false, false]);

    await click(radios[1]);
    expect(radios[1].checked).toBe(true);
    expect(field('acp-command').value).toBe('fake-acp-bridge');
    expect(field('acp-args').value).toBe('--stdio\n--quiet');

    await click(radios[2]);
    expect(radios[2].checked).toBe(true);
    // the fields stay editable and keep what is there
    await setValue(field('acp-command'), 'my-own');
    await click(button('Review'));
    expect(host.querySelector('[data-testid="acp-table"]')?.textContent).toContain('Command:        my-own');
  });

  it('with nothing detected or a failed look-up, the person enters it themselves', async () => {
    const api = mockApi({ acpDetect: vi.fn(async () => ({ ok: true, command: 'fake-agent', candidates: [] })) });
    await render(api);
    await findIt('Chat (ACP)');
    expect(host.querySelector('[data-testid="acp-no-candidates"]')).toBeTruthy();
    expect((host.querySelector('input[name="wizard-acp-pick"]') as HTMLInputElement).checked).toBe(true);
    expect(field('acp-command').value).toBe('fake-agent');
  });

  it('says so when detection fails and still allows typing', async () => {
    const api = mockApi({ acpDetect: vi.fn(async () => Promise.reject('WIZARD_NOT_AVAILABLE: no help text.')) });
    await render(api);
    await findIt('Chat (ACP)');
    expect(host.querySelector('[data-testid="acp-detect-error"]')?.textContent).toContain('no help text.');
    expect(button('Review')).toBeTruthy();
  });

  it('shows plain local messages and does not go on until they are fixed', async () => {
    const api = mockApi({ acpDetect: vi.fn(async () => ({ ok: true, command: 'fake-agent', candidates: [] })) });
    await render(api);
    await findIt('Chat (ACP)');
    await setValue(field('acp-command'), '   ');
    await setValue(field('acp-env'), 'GOOD\n1bad');
    await setValue(field('acp-resume'), '--resume');
    // nothing is shown before the first try
    expect(host.querySelectorAll('[role="alert"]')).toHaveLength(0);
    await click(button('Review'));
    const alerts = Array.from(host.querySelectorAll('[role="alert"]')).map((a) => a.textContent);
    expect(alerts).toEqual([
      'Enter the program that speaks the chat protocol.',
      '"1bad" is not a valid name. Use letters, digits and underscores, and do not start with a digit.',
      'The resume command must contain {session}, the place where the session id goes.',
    ]);
    expect(host.querySelector('[data-testid="acp-review"]')).toBeNull();
    expect(text()).toContain('Everything else from the app');

    await setValue(field('acp-command'), 'ok');
    await setValue(field('acp-env'), 'GOOD');
    await setValue(field('acp-resume'), '--resume\n{session}');
    await click(button('Review'));
    expect(host.querySelector('[data-testid="acp-review"]')).toBeTruthy();
  });

  it("shows the backend's refusal text and stays on the review step", async () => {
    const api = mockApi({ acpFinish: vi.fn(async () => Promise.reject('AGENT_DEF_ACP_INVALID: acp.args is out of range.')) });
    await render(api);
    await findIt('Chat (ACP)');
    await click(button('Review'));
    await click(button('Approve'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('acp.args is out of range.');
    expect(host.querySelector('[data-testid="acp-review"]')).toBeTruthy();
    expect(button('Approve')?.disabled).toBe(false);
  });

  it('both: adds the [acp] block to the draft before it is checked, and shows the table in the review', async () => {
    const api = mockApi();
    await render(api);
    await findIt('Both');
    expect(api.start).toHaveBeenCalledWith('fake-agent', true);
    expect(api.acpDetect).toHaveBeenCalledWith('wz_1');
    await setValue(field('acp-env'), 'FAKE_KEY');
    await click(button('Next'));
    await flush();
    // the normal flow goes on: the capture step comes next
    await click(button('Skip the visible part'));
    await flush();
    await click(button('Start'));
    await flush();
    const checked = String(api.verify.mock.calls[0][1]);
    expect(checked.startsWith(GOOD.trimEnd())).toBe(true);
    expect(checked).toContain('[acp]\ncommand = "fake-agent"\nargs = ["acp"]\nenv_allow = ["FAKE_KEY"]\n');
    // the writer was not asked about the chat part
    expect(String(api.ask.mock.calls[0][2])).not.toContain('FAKE_KEY');

    await click(button('Run the live test'));
    await flush();
    expect(host.querySelector('[data-testid="review-acp"]')?.textContent).toContain('Allowed names:  FAKE_KEY');
    await click(button('Show TOML'));
    expect(host.querySelector('[data-testid="wizard-toml"]')?.textContent).toContain('[acp]');
    await click(button('Approve'));
    expect(api.finish).toHaveBeenCalledTimes(1);
    expect(api.acpFinish).not.toHaveBeenCalled();
  });
});
