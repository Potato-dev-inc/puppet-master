// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { JobsApi, JobsMap, PaneJob } from '../../../lib/pane-jobs';
import { jobCount, jobsRunning, runTime } from '../../../lib/pane-jobs';
import { BackgroundCard } from './BackgroundCard';
import { JobsChip } from './JobsChip';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const opened = vi.hoisted(() => [] as string[]);
vi.mock('../../../lib/pane-preview', () => ({ openPreview: (url: string) => (opened.push(url), Promise.resolve()) }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  opened.length = 0;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const job = (pid: number, over: Partial<PaneJob> = {}): PaneJob => ({
  pid,
  parentPid: 1,
  name: 'python',
  command: 'python -m http.server 8765',
  startedAtMs: Date.now() - 125_000,
  ports: [8765],
  ...over,
});
const api = (all: JobsMap = {}): JobsApi & { stop: ReturnType<typeof vi.fn> } => ({ all: vi.fn(async () => all), stop: vi.fn(async () => {}) });
const click = (el: Element | null) => act(() => void (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true })));
const button = (label: string) => [...document.querySelectorAll('button')].find((b) => b.textContent === label) ?? null;

describe('pane-jobs helpers', () => {
  it('formats the time, the count and the sentence', () => {
    expect(runTime(0, 125_000)).toBe('2m');
    expect(jobCount({ a: [job(1), job(2)], b: [job(3)] })).toBe(3);
    expect(jobsRunning(1)).toBe('1 job running');
    expect(jobsRunning(3)).toBe('3 jobs running');
  });
});

describe('BackgroundCard', () => {
  it('is hidden with no jobs', () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[]} api={api()} />));
    expect(host.innerHTML).toBe('');
  });

  it('shows the name, the port chip, the run time and the short command', () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    const text = host.textContent ?? '';
    expect(text).toContain('Background');
    expect(text).toContain('python');
    expect(text).toContain(':8765');
    expect(text).toContain('2m');
    expect(host.querySelector('code')?.textContent).toBe('python -m http.server 8765');
  });

  it('opens the port in your browser on Shift+click (a plain click opens the app preview)', () => {
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={api()} />));
    act(() => void (button(':8765') as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true })));
    expect(opened).toEqual(['http://localhost:8765/']);
  });

  it('asks before it stops and No changes nothing', () => {
    const a = api();
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={a} />));
    click(button('Stop'));
    expect(host.textContent).toContain('Stop this job?');
    click(button('No'));
    expect(host.textContent).not.toContain('Stop this job?');
    expect(a.stop).not.toHaveBeenCalled();
  });

  it('stops exactly that pid of that pane, with the start time the job was listed with, on Yes', async () => {
    const a = api();
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7, { startedAtMs: 1_700_000_000_123 }), job(8)]} api={a} />));
    click(host.querySelectorAll('.ck-job-stop')[0]);
    await act(async () => click(button('Yes')));
    expect(a.stop).toHaveBeenCalledTimes(1);
    // The start time travels with the pid: a pid the system reused for a new process has another one, and is refused.
    expect(a.stop).toHaveBeenCalledWith('p1', 7, 1_700_000_000_123);
  });

  it('says why when the stop is refused', async () => {
    const a = api();
    a.stop.mockRejectedValueOnce('That job is already gone.');
    act(() => root.render(<BackgroundCard paneId="p1" jobs={[job(7)]} api={a} />));
    click(button('Stop'));
    await act(async () => click(button('Yes')));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('That job is already gone.');
    expect(button('Stop')).not.toBeNull();
  });
});

describe('JobsChip', () => {
  const names = new Map([
    ['p1', 'Claude Code'],
    ['p2', 'Codex'],
  ]);
  const flush = () => act(async () => void (await Promise.resolve()));

  it('is hidden when nothing runs', async () => {
    act(() => root.render(<JobsChip names={names} onOpenPane={() => {}} api={api({})} />));
    await flush();
    expect(host.querySelector('[data-testid="jobs-chip"]')).toBeNull();
  });

  it('counts the jobs of every pane and lists them grouped by pane in a sheet', async () => {
    const onOpenPane = vi.fn();
    const all = { p1: [job(1), job(2, { name: 'node', command: 'node vite.js', ports: [5173] })], p2: [job(3)] };
    act(() => root.render(<JobsChip names={names} onOpenPane={onOpenPane} api={api(all)} />));
    await flush();
    expect(host.querySelector('[data-testid="jobs-chip"]')?.textContent).toBe('3 jobs running');
    click(host.querySelector('[data-testid="jobs-chip"]'));
    const sheet = document.querySelector('.ck-sheet--jobs');
    expect(sheet).not.toBeNull();
    const groups = [...sheet!.querySelectorAll('section')].map((s) => s.getAttribute('aria-label'));
    expect(groups).toEqual(['Claude Code', 'Codex']);
    expect(sheet!.querySelectorAll('[data-testid="background-job"]').length).toBe(3);
    click(button('Open'));
    expect(onOpenPane).toHaveBeenCalledWith('p1');
  });
});
