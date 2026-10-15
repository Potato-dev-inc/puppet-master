// @vitest-environment jsdom
// The pane's chrome says plain things: no screen size or process id in the header, the project (not the
// copy's generated folder) in the subtitle, one question card inside the Terminal tab, and a slim bar on
// the other tabs.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const COPY_CWD = 'C:\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c';
const prompt = {
  promptId: 'q1',
  kind: 'permission',
  text: 'Run npm test?',
  summary: 'Run `npm test`',
  choices: [
    { id: 'yes', label: 'Yes' },
    { id: 'no', label: 'No' },
  ],
  risk: { level: 'low', headline: 'Run tests', reasons: ['It runs your tests.'] },
};

function cockpit(cwd: string, withPrompt: boolean, status = withPrompt ? 'waiting_input' : 'idle'): CockpitApi {
  const p = { status, info: { id: 'p1', agent_type: 'claude', status, pid: 4120, created_at: 1, last_output_at: null, cwd, cols: 158, rows: 56 } };
  return {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Claude Code']]),
    activities: withPrompt ? { p1: { kind: 'waiting', text: 'Waiting: allow `npm test`?' } } : {},
    previews: {},
    asked: { questions: [] },
    prompts: withPrompt ? { p1: prompt } : {},
    statusSince: new Map(withPrompt ? [['p1', 2000]] : []),
    now: 12_000,
    selectedId: 'p1',
    mode: 'orchestration',
    select: vi.fn(),
    notify: vi.fn(),
    answer: vi.fn(),
    closePane: vi.fn(),
  } as unknown as CockpitApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockImplementation(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null));
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent?.startsWith(name))!;
const header = () => host.querySelector('.ck-pane-head')!.textContent ?? '';
const openMenuItem = async (label: string) => {
  await act(async () => host.querySelector<HTMLElement>('[aria-label="Pane actions"]')!.click());
  const item = [...host.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((m) => m.textContent === label)!;
  await act(async () => item.click());
  await flush();
};

describe('pane header', () => {
  it('shows no screen size and no process id', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', false)} paneId="p1" />));
    await flush();
    expect(header()).not.toMatch(/158|56|×|4120|pid/);
    expect(host.querySelector('.ck-grid-size')).toBeNull();
  });

  it('names the project a separate copy was made from, never its folder id', async () => {
    await act(async () => root.render(<PaneView c={cockpit(COPY_CWD, false)} paneId="p1" />));
    await flush();
    expect(header()).toContain('shop');
    expect(header()).toContain('separate copy');
    expect(header()).not.toMatch(/7d4cb2b3|worktrees|pm\//);
  });

  it('keeps the size and process id one click away under Details', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', false)} paneId="p1" />));
    await flush();
    await openMenuItem('Details');
    const sheet = document.querySelector('.ck-sheet')!;
    expect(sheet.textContent).toContain('158×56');
    expect(sheet.textContent).toContain('4120');
  });

  it('says what kind of thing it asks, not the raw command, while waiting', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', true)} paneId="p1" />));
    await flush();
    const line = host.querySelector('.ck-pane-doing')!.textContent!;
    expect(line).toBe('Needs you · Run tests · 10s');
    expect(line).not.toContain('`');
  });

  it('strips backticks from an activity line that has no risk headline', async () => {
    const c = cockpit('C:\\work\\shop', true);
    (c.prompts as Record<string, unknown>).p1 = { ...prompt, risk: undefined };
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    expect(host.querySelector('.ck-pane-doing')!.textContent).toBe('Waiting: allow npm test? · 10s');
  });
});

describe('the dock', () => {
  it('puts the question inside the Terminal tab, below the tab strip', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', true)} paneId="p1" />));
    await flush();
    const tabs = host.querySelector('.ck-tabs-bar')!;
    const body = host.querySelector('.ck-pane-body')!;
    const ask = host.querySelector('.ck-dock [data-ib-card]')!;
    expect(body.contains(ask)).toBe(true);
    // the tab strip comes before the card, so it never moves when a card arrives
    expect(tabs.compareDocumentPosition(ask) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(host.querySelector('.ck-dock-bar')).toBeNull();
  });

  it('shows a slim "Needs you" bar on the other tabs that goes back to the Terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', true)} paneId="p1" />));
    await flush();
    await act(async () => tab('History').click());
    await flush();
    expect(host.querySelector('.ck-dock [data-ib-card]')).toBeNull();
    const bar = host.querySelector<HTMLElement>('.ck-dock-bar')!;
    expect(bar.textContent).toContain('Needs you · Run tests');
    await act(async () => bar.click());
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('.ck-dock [data-ib-card]')).not.toBeNull();
  });

  it('docks the Inbox card itself: its words, its age, no "Open terminal" (the terminal is right here)', async () => {
    const c = cockpit('C:\\work\\shop', true);
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    const card = host.querySelector<HTMLElement>('.ck-dock .ib-dock [data-ib-card]')!;
    expect(card.dataset.kind).toBe('approval');
    const labels = Array.from(card.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels).toContain('Allow once');
    expect(labels).toContain('Deny');
    expect(labels).not.toContain('Open terminal');
    expect(card.textContent).toContain('10s');
    const allow = Array.from(card.querySelectorAll('button')).find((b) => b.textContent === 'Allow once')!;
    await act(async () => allow.click());
    expect(c.answer).toHaveBeenCalledWith('p1', prompt.promptId, 'yes', 'Yes', expect.objectContaining({ kind: 'click' }));
  });

  it('has no dock and no bar when nothing waits', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', false)} paneId="p1" />));
    await flush();
    expect(host.querySelector('.ck-dock')).toBeNull();
    await act(async () => tab('Receipt').click());
    expect(host.querySelector('.ck-dock-bar')).toBeNull();
  });
});

describe('closing a running agent', () => {
  it('says the agent stops and its conversation ends, and the button says Close agent', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', false, 'running')} paneId="p1" />));
    await flush();
    await openMenuItem('Close');
    await flush();
    const text = document.body.textContent ?? '';
    expect(text).toContain('Claude Code stops, and its conversation ends.');
    expect(text).toContain('Close agent');
    expect(text).not.toContain('Close pane');
  });
});

describe('the Receipt tab before the first turn', () => {
  it('has one empty state, no Second opinion and no Refresh button', async () => {
    await act(async () => root.render(<PaneView c={cockpit('C:\\work\\shop', false)} paneId="p1" />));
    await flush();
    await act(async () => tab('Receipt').click());
    await flush();
    const text = host.querySelector('.ck-pane-body')!.textContent!;
    expect(text).toContain('No turns yet');
    expect(text).toContain('The receipt appears when Claude Code finishes its first turn.');
    expect(text).not.toContain('Second opinion');
    expect(text).not.toContain('Refresh');
    expect(text).not.toContain('Nothing yet');
  });
});
