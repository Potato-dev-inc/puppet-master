// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StopReport } from '../../lib/loop-guard';
import { PanicSheet } from './PanicSheet';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const HELD = { held: true, sinceMs: 1, botsPaused: 2, routinesHeld: 4, computerAsleep: true };
const names = { get: () => undefined } as never;

function report(agents: number): StopReport {
  return {
    stopped: Array.from({ length: agents }, (_, i) => ({ pane_id: `p${i}`, agent_type: 'claude', cwd: '/p' })),
    failed: [],
  } as unknown as StopReport;
}

let host: HTMLDivElement;
let root: Root;

async function show(agents: number, hold: unknown) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === 'potato_hold_state') return hold;
    if (cmd === 'potato_resume_all') return { held: false, sinceMs: null, botsPaused: 0, routinesHeld: 0, computerAsleep: false };
    throw new Error(`unexpected ${cmd}`);
  });
  await act(async () => root.render(<PanicSheet report={report(agents)} names={names} onClose={() => {}} />));
  await act(async () => {});
}

const rowTexts = () => Array.from(document.querySelectorAll('.ck-panic-list .ck-panic-name')).map((n) => n.textContent);

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(invoke).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('PanicSheet: Everything is stopped', () => {
  it('lists only what was running, in plain words', async () => {
    await show(3, HELD);
    expect(document.querySelector('#ck-panic-t')?.textContent).toBe('Everything is stopped');
    expect(rowTexts()).toEqual(['3 agents stopped', '2 bots paused', '4 routines on hold', "Fish's computer is asleep"]);
  });

  it('leaves out rows for things that were not running', async () => {
    await show(1, { held: true, sinceMs: 1, botsPaused: 0, routinesHeld: 1, computerAsleep: false });
    expect(rowTexts()).toEqual(['1 agent stopped', '1 routine on hold']);
  });

  it('"Start bots again" calls potato_resume_all and the hold goes away', async () => {
    await show(0, HELD);
    const button = Array.from(document.querySelectorAll('button')).find((b) => b.textContent === 'Start bots again');
    expect(button).toBeTruthy();
    await act(async () => button!.click());
    expect(invoke).toHaveBeenCalledWith('potato_resume_all');
    expect(Array.from(document.querySelectorAll('button')).some((b) => b.textContent === 'Start bots again')).toBe(false);
  });

  it('shows no "Start bots again" when nothing is held', async () => {
    await show(2, { held: false, sinceMs: null, botsPaused: 0, routinesHeld: 0, computerAsleep: false });
    expect(Array.from(document.querySelectorAll('button')).some((b) => b.textContent === 'Start bots again')).toBe(false);
  });
});
