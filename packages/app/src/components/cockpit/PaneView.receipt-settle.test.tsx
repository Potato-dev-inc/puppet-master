// @vitest-environment jsdom
// Live r13 (tour chapter 05): the Receipt tab said "Result: Still working" while the agent sat idle.
// The pane reads idle a moment before its run settles; the tab fetched the run at "idle" and never
// looked again, because the pane's status did not change after that.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RUN_SETTLE_POLL_MS, useRunData } from './PaneView';

let runInfo: { handle: string; status: string; receipt: unknown } | null = null;
const invoke = vi.fn(async (cmd: string) => {
  if (cmd === 'pane_run_info') return runInfo;
  if (cmd === 'pane_transcript') return { chunks: [] };
  return null;
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let seen: { status?: string; outcome?: string } = {};
function Probe({ status }: { status: string }) {
  const run = useRunData('p1', true, status);
  seen = { status: run.info?.status, outcome: (run.info?.receipt as { outcome?: string } | null)?.outcome };
  return null;
}

let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  vi.useFakeTimers();
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('the Receipt tab of a run that settles after its pane went idle', () => {
  it('shows the settled result without the pane status changing again', async () => {
    runInfo = { handle: 'h1', status: 'running', receipt: { outcome: 'in_progress' } };
    act(() => root.render(<Probe status="idle" />));
    await flush();
    expect(seen.outcome).toBe('in_progress');
    // The run completes; the pane stays idle (no status change to trigger a fetch).
    runInfo = { handle: 'h1', status: 'completed', receipt: { outcome: 'completed_unchecked' } };
    await act(async () => {
      vi.advanceTimersByTime(RUN_SETTLE_POLL_MS + 10);
    });
    await flush();
    expect(seen.outcome).toBe('completed_unchecked');
    // Settled: no more polling.
    const calls = invoke.mock.calls.length;
    await act(async () => {
      vi.advanceTimersByTime(RUN_SETTLE_POLL_MS * 3);
    });
    await flush();
    expect(invoke.mock.calls.length).toBe(calls);
  });
});
