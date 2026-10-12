// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_STANDBY_CONFIG } from '../../../lib/standby';
import type { StandbyConfig, StandbyStatus } from '../../../lib/tauri-types';
import { PreloadTab, type PreloadApi } from './PreloadTab';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function statusFor(config: StandbyConfig): StandbyStatus {
  return {
    enabled: config.preload.enabled,
    config,
    counts: { total: 0, ready: 0, starting: 0 },
    standbys: [],
    blocked: [],
    recent: [],
    stats: { spawned: 0, spawn_failed: 0, claimed: 0, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
    supported: [],
  };
}

function mockApi(initial: StandbyConfig = DEFAULT_STANDBY_CONFIG) {
  let current = initial;
  const api = {
    getStatus: vi.fn(async () => statusFor(current)),
    setConfig: vi.fn(async (next: StandbyConfig) => {
      current = next;
      return statusFor(current);
    }),
  };
  return api satisfies PreloadApi;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: PreloadApi) {
  await act(async () => {
    root.render(<PreloadTab api={api} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const checkbox = (label: string) => host.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
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

describe('PreloadTab', () => {
  it('is off by default, explains the cost, and lists visible-capable agents', async () => {
    await render(mockApi());
    const master = host.querySelector('[role="switch"]');
    expect(master?.getAttribute('aria-checked')).toBe('false');
    expect(host.textContent).toContain('running CLI process that uses memory');
    expect(checkbox('Keep Claude Code warm')?.checked).toBe(false);
    expect(checkbox('Keep Claude Code warm')?.disabled).toBe(false);
    // Codex can open a visible pane but has no warm mode yet.
    expect(checkbox('Keep Codex CLI warm')?.disabled).toBe(true);
    expect(host.textContent).toContain('no warm mode yet');
  });

  it('the master switch and a provider tick are saved through the api', async () => {
    const api = mockApi();
    await render(api);
    await click(host.querySelector('[role="switch"]'));
    expect(api.setConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ preload: { enabled: true, providers: {} } }),
    );
    await click(checkbox('Keep Claude Code warm'));
    expect(api.setConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ preload: { enabled: true, providers: { claude: 1 } } }),
    );
    expect(checkbox('Keep Claude Code warm')?.checked).toBe(true);
    await click(checkbox('Keep Claude Code warm'));
    expect(api.setConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ preload: { enabled: true, providers: {} } }),
    );
  });

  it('a ticked provider can keep two warm and the idle timeout is edited in minutes', async () => {
    const api = mockApi({ ...DEFAULT_STANDBY_CONFIG, preload: { enabled: true, providers: { claude: 1 } } });
    await render(api);
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Warm Claude Code count"]');
    expect(select?.value).toBe('1');
    expect(select?.disabled).toBe(false);
    expect(Array.from(select?.options ?? []).map((o) => o.value)).toEqual(['1', '2']);
    expect(host.querySelector<HTMLSelectElement>('select[aria-label="Warm Cursor Agent CLI count"]')?.disabled).toBe(true);
    const idle = host.querySelector<HTMLInputElement>('input[aria-label="Preload idle timeout (minutes)"]');
    expect(idle?.value).toBe('20');
  });
});
