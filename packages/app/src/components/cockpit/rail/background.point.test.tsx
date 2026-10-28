// @vitest-environment jsdom
// The job port chips in the Background card and the jobs sheet. A click opens the app in the in-app
// preview, Shift+click opens your browser, and "Point" opens the preview with the picker on (Point and Fix).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { JobsApi, JobsMap, PaneJob } from '../../../lib/pane-jobs';
import { BackgroundCard } from './BackgroundCard';
import { JobsChip } from './JobsChip';

const invoke = vi.fn(async () => null as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...a) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  invoke.mockImplementation(async () => null);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

const job = (pid: number, over: Partial<PaneJob> = {}): PaneJob => ({
  pid,
  parentPid: 1,
  name: 'node',
  command: 'node vite.js',
  startedAtMs: Date.now() - 125_000,
  ports: [8765],
  ...over,
});
const api = (all: JobsMap = {}): JobsApi => ({ all: vi.fn(async () => all), stop: vi.fn(async () => {}) });
const click = (el: Element | null, init: MouseEventInit = {}) => act(() => void (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true, ...init })));
const button = (label: string) => [...document.querySelectorAll('button')].find((b) => b.textContent === label) ?? null;
const flush = () => act(async () => {
  await new Promise((r) => setTimeout(r, 0));
});
/** Command names sent to the desktop shell, in order. */
const commands = () => (invoke.mock.calls as unknown[][]).map((c) => c[0]);

describe('job port chips in the Background card', () => {
  it('a plain click opens the app in the in-app preview for this pane', async () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    click(button(':8765'));
    await flush();
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'p1', port: 8765 });
    expect(commands()).not.toContain('point_arm');
    expect(commands()).not.toContain('open_external_url');
  });

  it('Shift+click opens your browser and never the app preview', async () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    click(button(':8765'), { shiftKey: true });
    await flush();
    expect(invoke).toHaveBeenCalledWith('open_external_url', { url: 'http://localhost:8765/' });
    expect(commands()).not.toContain('point_open_preview');
  });

  it('the Point button opens the preview and arms the picker, in that order', async () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    click(host.querySelector('[aria-label="Point and fix on localhost:8765"]'));
    await flush();
    expect(commands()).toEqual(['point_open_preview', 'point_arm']);
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'p1', port: 8765 });
  });

  it('says what a click does in the chip title', () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    expect(button(':8765')?.getAttribute('title')).toBe('Preview :8765 (Shift+click: your browser)');
  });

  it('says why when the preview cannot open, under the row', async () => {
    invoke.mockImplementation(async () => {
      throw 'That preview is not running any more.';
    });
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    click(button(':8765'));
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('That preview is not running any more.');
  });
});

describe('job port chips in the jobs sheet', () => {
  const names = new Map([['p9', 'Agent']]);

  it('Point sends each job to its own pane id', async () => {
    const all = { p9: [job(4, { ports: [5173] })] };
    act(() => root.render(<JobsChip names={names} onOpenPane={() => {}} api={api(all)} />));
    await flush();
    click(host.querySelector('[data-testid="jobs-chip"]'));
    click(button(':5173'));
    await flush();
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'p9', port: 5173 });
    expect(commands()).not.toContain('open_external_url');
    invoke.mockClear();
    click(button('Point'));
    await flush();
    expect(commands()).toEqual(['point_open_preview', 'point_arm']);
  });
});
