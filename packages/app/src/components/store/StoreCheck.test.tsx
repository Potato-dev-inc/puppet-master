// @vitest-environment jsdom
// The Check sheet: says what runs and what it costs, runs nothing until Start, shows the wizard's progress and a
// result line in plain words. Tauri `invoke` is mocked and must never be called from this sheet.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ProgressEvent } from '../../lib/agentWizard';
import { StoreCheck, type RunStoreCheck, type StoreCheckResult } from './StoreCheck';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  invoke.mockReset();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderSheet(runCheck: RunStoreCheck, over: { installedVersion?: string | null } = {}) {
  const onOpenAgent = vi.fn();
  const onClose = vi.fn();
  act(() =>
    root.render(
      <StoreCheck
        id="qwen"
        label="Qwen Code"
        installedVersion={over.installedVersion === undefined ? '0.26.2' : over.installedVersion}
        runCheck={runCheck}
        onOpenAgent={onOpenAgent}
        onClose={onClose}
      />,
    ),
  );
  return { onOpenAgent, onClose };
}

const button = (text: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text);

function click(text: string) {
  const el = button(text);
  if (!el) throw new Error(`no button "${text}"`);
  act(() => el.click());
}

async function settle(resolve: () => void) {
  await act(async () => {
    resolve();
    await Promise.resolve();
  });
}

describe('StoreCheck', () => {
  it('says what will run and what it costs, and runs nothing until Start', () => {
    const runCheck = vi.fn<RunStoreCheck>(() => new Promise(() => {}));
    renderSheet(runCheck);

    expect(host.textContent).toContain('Check Qwen Code');
    expect(host.textContent).toContain('version 0.26.2');
    expect(host.textContent).toContain('temporary folder');
    expect(host.querySelector('[data-testid="store-check-cost"]')?.textContent).toContain(
      'A check uses real agent turns from your own plan.',
    );
    expect(host.querySelector('[data-testid="store-check-cost"]')?.textContent).toContain('about 4 small messages');
    expect(host.textContent).not.toMatch(/sandbox/i);
    expect(runCheck).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();

    click('Start');
    expect(runCheck).toHaveBeenCalledTimes(1);
    expect(runCheck.mock.calls[0][0]).toBe('qwen');
    expect(runCheck.mock.calls[0][1]).toBeInstanceOf(AbortSignal);
    expect(typeof runCheck.mock.calls[0][2]).toBe('function');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('says the program is not found when no version is installed', () => {
    renderSheet(vi.fn<RunStoreCheck>(() => new Promise(() => {})), { installedVersion: null });
    expect(host.textContent).toContain('the program found on this PC');
  });

  it('shows the wizard progress from its events, then the result line', async () => {
    const d = deferred<StoreCheckResult>();
    const runCheck: RunStoreCheck = (_id, _signal, onEvent) => {
      onEvent({ step: 'help', status: 'done', round: 0 } satisfies ProgressEvent);
      onEvent({ step: 'draft', status: 'running', round: 1 });
      return d.promise;
    };
    renderSheet(runCheck);
    click('Start');

    const help = host.querySelector('[data-step="help"]');
    const draft = host.querySelector('[data-step="draft"]');
    expect(help?.getAttribute('data-status')).toBe('done');
    expect(draft?.getAttribute('data-status')).toBe('running');
    expect(host.querySelector('[data-step="check"]')).toBeNull();

    await settle(() => d.resolve({ status: 'done', passed: 7, total: 7, version: '0.26.2' }));
    expect(host.querySelector('[data-testid="store-check-result"]')?.textContent).toBe(
      '7 of 7 checks passed on version 0.26.2.',
    );
    expect(host.textContent).toContain('Nothing was added or approved.');
    expect(button('Check again')).toBeTruthy();
    expect(button('Start')).toBeUndefined();
  });

  it('shows a partial result with its note', async () => {
    const d = deferred<StoreCheckResult>();
    renderSheet(() => d.promise);
    click('Start');
    await settle(() =>
      d.resolve({ status: 'done', passed: 5, total: 7, version: '0.26.2', note: 'Two checks did not pass on this PC.' }),
    );
    expect(host.querySelector('[data-testid="store-check-result"]')?.textContent).toBe(
      '5 of 7 checks passed on version 0.26.2.',
    );
    expect(host.textContent).toContain('Two checks did not pass on this PC.');
  });

  it('a sign-in stop says so, Open agent opens the agent, and Retry runs again', async () => {
    const d = deferred<StoreCheckResult>();
    const runCheck = vi.fn<RunStoreCheck>(() => d.promise);
    const { onOpenAgent } = renderSheet(runCheck);
    click('Start');
    await settle(() => d.resolve({ status: 'sign_in', message: 'Sign in to Qwen Code, then press Retry.' }));

    expect(host.querySelector('[data-testid="store-check-sign-in"]')?.textContent).toBe(
      'Sign in to Qwen Code, then press Retry.',
    );
    expect(host.querySelector('[data-testid="store-check-result"]')).toBeNull();

    click('Open agent');
    expect(onOpenAgent).toHaveBeenCalledWith('qwen');
    expect(runCheck).toHaveBeenCalledTimes(1);

    runCheck.mockImplementation(() => new Promise(() => {}));
    click('Retry');
    expect(runCheck).toHaveBeenCalledTimes(2);
  });

  it('Stop aborts the run and ends in a stopped state with nothing added', async () => {
    let captured: AbortSignal | null = null;
    const runCheck: RunStoreCheck = (_id, signal) => {
      captured = signal;
      return new Promise((resolve) => {
        signal.addEventListener('abort', () => resolve({ status: 'stopped', message: 'Stopped by you.' }));
      });
    };
    const { onClose } = renderSheet(runCheck);
    click('Start');
    expect(button('Cancel')).toBeUndefined();
    expect(button('Close')).toBeUndefined();

    // Escape does not close the sheet while a run is going.
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(onClose).not.toHaveBeenCalled();

    click('Stop');
    expect(captured!.aborted).toBe(true);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(host.textContent).toContain('Check stopped. Nothing was added or approved.');
    expect(button('Retry')).toBeTruthy();
    expect(host.querySelector('[data-testid="store-check-result"]')).toBeNull();
  });

  it('a stopped run ignores a late result', async () => {
    const d = deferred<StoreCheckResult>();
    renderSheet(() => d.promise);
    click('Start');
    click('Stop');
    expect(button('Stopping...')?.disabled).toBe(true);
    await settle(() => d.resolve({ status: 'done', passed: 7, total: 7, version: '0.26.2' }));
    expect(host.querySelector('[data-testid="store-check-result"]')).toBeNull();
    expect(host.textContent).toContain('Check stopped.');
  });

  it('shows a failed check in plain words', async () => {
    const d = deferred<StoreCheckResult>();
    renderSheet(() => d.promise);
    click('Start');
    await settle(() => d.reject('WIZARD_NOT_AVAILABLE: this wizard cannot run the check.'));
    expect(host.querySelector('.ck-doctor-error')?.textContent).toBe(
      'WIZARD_NOT_AVAILABLE: this wizard cannot run the check.',
    );
    expect(button('Retry')).toBeTruthy();
  });

  it('a runner that throws at once is shown as an error', async () => {
    renderSheet(() => {
      throw new Error('boom');
    });
    click('Start');
    await act(async () => {
      await Promise.resolve();
    });
    expect(host.querySelector('.ck-doctor-error')?.textContent).toBe('boom');
  });

  it('Cancel closes before Start and does not run anything', () => {
    const runCheck = vi.fn<RunStoreCheck>(() => new Promise(() => {}));
    const { onClose } = renderSheet(runCheck);
    click('Cancel');
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(runCheck).not.toHaveBeenCalled();
  });
});
