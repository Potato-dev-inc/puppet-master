// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentRunModeApi, RunMode, RunModeAgent, RunModeView } from '../../../lib/agentRunMode';
import { lapsedNote, notVerifiedText, ONE_TASK_LABEL, VISIBLE_LABEL } from '../../../lib/agentRunMode';
import { RunModeControl } from './AgentRunMode';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function agent(over: Partial<RunModeAgent> = {}): RunModeAgent {
  return {
    id: 'fake_custom',
    label: 'Fake Custom',
    headless_supported: true,
    headless_verified: true,
    headless_verified_reason: null,
    run_mode: 'visible',
    ...over,
  };
}

/** A backend that remembers the choice, and refuses One-task mode for an unverified agent like the real one. */
function mockApi(initial: RunModeAgent) {
  let current = initial;
  const view = (): RunModeView => ({ agents: [current] });
  return {
    get: vi.fn(async () => view()),
    set: vi.fn(async (_id: string, mode: RunMode) => {
      if (mode === 'one_task' && !current.headless_verified) throw new Error('RUN_MODE_NOT_VERIFIED: nope');
      current = { ...current, run_mode: mode };
      return view();
    }),
  } satisfies AgentRunModeApi;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: AgentRunModeApi, agentId = 'fake_custom') {
  await act(async () => {
    root.render(<RunModeControl agentId={agentId} api={api} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const select = () => host.querySelector<HTMLSelectElement>('select');
const option = (value: string) => host.querySelector<HTMLOptionElement>(`option[value="${value}"]`);

async function choose(value: string) {
  await act(async () => {
    const el = select()!;
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
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

describe('RunModeControl', () => {
  it('defaults to the visible pane and offers both modes in plain words', async () => {
    await render(mockApi(agent()));
    expect(select()?.value).toBe('visible');
    expect(host.textContent).toContain('Agent runs');
    expect(option('visible')?.textContent).toBe(VISIBLE_LABEL);
    expect(option('one_task')?.textContent).toBe(ONE_TASK_LABEL);
    expect(option('one_task')?.disabled).toBe(false);
    expect(host.querySelector('[data-testid="run-mode-reason"]')).toBeNull();
  });

  it('disables One-task mode and says why when the agent is not verified', async () => {
    await render(mockApi(agent({ headless_verified: false, headless_verified_reason: 'not_tested' })));
    expect(option('one_task')?.disabled).toBe(true);
    expect(option('visible')?.disabled).toBe(false);
    expect(host.querySelector('[data-testid="run-mode-reason"]')?.textContent).toBe(notVerifiedText('not_tested'));
    expect(notVerifiedText('not_tested')).toMatch(/wizard test/);
  });

  it('says a changed definition needs the wizard test again', async () => {
    await render(mockApi(agent({ headless_verified: false, headless_verified_reason: 'changed_since_test' })));
    expect(host.querySelector('[data-testid="run-mode-reason"]')?.textContent).toMatch(/changed after the wizard tested it/);
  });

  it('saves the choice the moment it is picked, and back again', async () => {
    const api = mockApi(agent());
    await render(api);
    await choose('one_task');
    expect(api.set).toHaveBeenCalledWith('fake_custom', 'one_task');
    expect(select()?.value).toBe('one_task');
    await choose('visible');
    expect(api.set).toHaveBeenLastCalledWith('fake_custom', 'visible');
    expect(select()?.value).toBe('visible');
  });

  it('shows the backend refusal in plain words and keeps the visible pane', async () => {
    const api = mockApi(agent({ headless_verified: false, headless_verified_reason: 'not_tested' }));
    await render(api);
    await choose('one_task');
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('nope');
    expect(select()?.value).toBe('visible');
  });

  it('tells you when a saved One-task choice no longer holds', async () => {
    const lapsed = agent({ run_mode: 'one_task', headless_verified: false, headless_verified_reason: 'changed_since_test' });
    await render(mockApi(lapsed));
    expect(host.querySelector('[data-testid="run-mode-lapsed"]')?.textContent).toBe(lapsedNote(lapsed));
    expect(lapsedNote(lapsed)).toMatch(/visible pane/);
    expect(lapsedNote(agent({ run_mode: 'one_task' }))).toBe('');
  });

  it('shows nothing for an agent it does not know, or when there is no desktop shell', async () => {
    await render(mockApi(agent()), 'someone_else');
    expect(host.querySelector('[data-testid="run-mode"]')).toBeNull();
    const offline: AgentRunModeApi = { get: vi.fn(async () => Promise.reject(new Error('Desktop shell required'))), set: vi.fn() };
    await render(offline);
    expect(host.querySelector('[data-testid="run-mode"]')).toBeNull();
  });
});
