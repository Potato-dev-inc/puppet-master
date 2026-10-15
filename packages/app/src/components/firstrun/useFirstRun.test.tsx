// @vitest-environment jsdom
// When the welcome shows: once on a fresh profile, never for someone who has used this install, and again
// when asked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FirstRunApi } from './first-run-api';
import type { FirstRunState } from './model';
import { useFirstRun, type FirstRunHost } from './useFirstRun';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let last: FirstRunHost;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  window.localStorage.clear();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

function Probe(props: { panes: number; ready: boolean; api: FirstRunApi; patch: never; load: never }) {
  last = useFirstRun(props.panes, props.ready, { api: props.api, patch: props.patch, load: props.load });
  return null;
}

const settle = () => act(async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); });

async function mount(state: FirstRunState | Error, opts: { panes?: number; ready?: boolean; settings?: Record<string, unknown> } = {}) {
  const patch = vi.fn(async () => ({}));
  const api: FirstRunApi = {
    state: vi.fn(async () => {
      if (state instanceof Error) throw state;
      return state;
    }),
    startTask: vi.fn(),
    taskStatus: vi.fn(),
  };
  const load = vi.fn(async () => opts.settings ?? {});
  await act(async () => {
    root.render(<Probe panes={opts.panes ?? 0} ready={opts.ready ?? true} api={api} patch={patch as never} load={load as never} />);
  });
  await settle();
  return { patch, api };
}

describe('useFirstRun', () => {
  it('shows once on a fresh profile with no panes', async () => {
    await mount({ show: true, auto: true, step: null });
    expect(last.open).toBe(true);
    expect(last.seed.start).toBe('welcome');
  });

  it('does not show for an install that has had a pane', async () => {
    window.localStorage.setItem('pm.cockpit.everSpawned', '1');
    await mount({ show: true, auto: true, step: null });
    expect(last.open).toBe(false);
  });

  it('does not show while panes exist, and waits until the pane list is known', async () => {
    await mount({ show: true, auto: true, step: null }, { panes: 2 });
    expect(last.open).toBe(false);
    window.localStorage.clear();
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount({ show: true, auto: true, step: null }, { ready: false });
    expect(last.open).toBe(false);
  });

  it('does not show once finished or skipped', async () => {
    await mount({ show: false, auto: false, step: null });
    expect(last.open).toBe(false);
  });

  it('shows without a desktop shell? no: an unreadable state means no welcome', async () => {
    await mount(new Error('Desktop shell required'));
    expect(last.open).toBe(false);
  });

  it('resumes at the saved step with the saved folder and look', async () => {
    await mount(
      { show: true, auto: true, step: 'task' },
      { settings: { onboarding_project: 'D:\\code\\shop', onboarding_agent: 'claude', simple_mode: true } },
    );
    expect(last.open).toBe(true);
    expect(last.seed.start).toBe('task');
    expect(last.seed.project?.path).toBe('D:\\code\\shop');
    expect(last.seed.project?.name).toBe('shop');
    expect(last.seed.agentId).toBe('claude');
    expect(last.seed.look).toBe('ai');
  });

  it('a saved task step without a folder lands on the folder step', async () => {
    await mount({ show: true, auto: true, step: 'task' }, { settings: {} });
    expect(last.seed.start).toBe('project');
  });

  it('Finish writes onboarding_done and closes; reopen writes false and starts at the top', async () => {
    const { patch } = await mount({ show: true, auto: true, step: null });
    await act(async () => last.finish());
    expect(last.open).toBe(false);
    expect(patch).toHaveBeenCalledWith({ onboarding_done: true, onboarding_step: null, onboarding_project: null, onboarding_agent: null });
    const key = last.openKey;
    await act(async () => last.reopen());
    await settle();
    expect(patch).toHaveBeenCalledWith({ onboarding_done: false, onboarding_step: 'welcome' });
    expect(last.open).toBe(true);
    expect(last.seed.start).toBe('welcome');
    expect(last.openKey).toBeGreaterThan(key);
  });
});
