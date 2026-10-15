// @vitest-environment jsdom
// The Inbox is ONE list of ONE kind of card: same header, same single action row, same age format,
// high risk first, Deny before Allow, and a keyboard that works without the mouse.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { PanePrompt } from '../../lib/cockpit-api';
import type { Question } from '../../lib/questions';
import { Inbox } from './Inbox';

const settings = vi.hoisted(() => ({ patchSettings: vi.fn(() => Promise.resolve({})) }));
vi.mock('../../lib/settings', async (importOriginal) => ({ ...(await importOriginal<typeof import('../../lib/settings')>()), patchSettings: settings.patchSettings }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (Element.prototype as { scrollIntoView?: () => void }).scrollIntoView = () => {};
  window.localStorage.setItem('pm.cockpit.everSpawned', '1');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  settings.patchSettings.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  window.localStorage.clear();
});

const NOW = 600_000;
const info = (id: string, agent: string, status: string, cwd: string): PaneInfo =>
  ({ id, agent_type: agent, pid: 1, status, created_at: 1, last_output_at: null, cwd, cols: 80, rows: 24, title: null }) as unknown as PaneInfo;

const prompt = (level: 'low' | 'high', headline: string): PanePrompt => ({
  promptId: `p-${level}`,
  kind: 'command_approval',
  text: 'raw',
  summary: 'Run command: x',
  choices: [
    { id: 'yes', label: 'Yes' },
    { id: 'always', label: 'Yes, and do not ask again for npm test', broad: true },
    { id: 'no', label: 'No, tell Claude what to do differently' },
  ],
  risk: { level, headline, reasons: ['It does a thing.', 'It cannot be undone.'] },
});

const question = (over: Partial<Question> = {}): Question => ({
  id: 'q1',
  agent: 'Planner',
  question: 'Which database?',
  choices: [{ id: 'pg', label: 'Postgres' }, { id: 'sq', label: 'SQLite' }],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: NOW - 240_000,
  expires_at_ms: NOW + 600_000,
  state: 'pending',
  ...over,
});

const consent = question({
  id: 'c1',
  agent: 'Claude Code',
  question: 'Allow Claude Code to control Codex?',
  context: 'It did not start this pane.',
  choices: [{ id: 'allow_once', label: 'Allow once' }, { id: 'deny', label: 'Deny' }],
  urgency: 'high',
  created_at_ms: NOW - 40_000,
  consent: { action: 'take_over', requester: 'Claude Code', target: 'Codex', pane_id: 'hi' },
});
const budget = question({
  id: 'b1',
  agent: 'Claude Code',
  pane_id: 'lo',
  question: 'Claude Code used its 20 turns.',
  context: 'Nothing was lost.',
  choices: [{ id: 'add_turns', label: 'Add 10 turns' }, { id: 'stop', label: 'Stop here' }],
  kind: 'budget',
  created_at_ms: NOW - 20_000,
});

function fixture(over: { questions?: Question[]; panes?: { id: string; agent: string; status: string; cwd: string }[]; since?: Record<string, number> } = {}) {
  const spec = over.panes ?? [
    { id: 'lo', agent: 'claude', status: 'waiting_input', cwd: 'C:\\work\\shop' },
    { id: 'hi', agent: 'codex', status: 'waiting_input', cwd: 'C:\\work\\billing' },
    { id: 'ps', agent: 'powershell', status: 'error', cwd: 'C:\\work\\shop' },
  ];
  const list = spec.map((p) => ({ info: info(p.id, p.agent, p.status, p.cwd), status: p.status }));
  const answer = vi.fn();
  const select = vi.fn();
  const answerQuestion = vi.fn();
  const dismissQuestion = vi.fn();
  const since: Record<string, number> = over.since ?? { lo: NOW - 300_000, hi: NOW - 5_000, ps: NOW - 60_000 };
  const api = {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([['lo', 'Claude Code'], ['hi', 'Codex'], ['ps', 'PowerShell']]),
    asked: { questions: over.questions ?? [], answerQuestion, dismissQuestion },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select,
    prompts: { lo: prompt('low', 'Run tests'), hi: prompt('high', 'Delete a folder outside this project') } as Record<string, PanePrompt>,
    statusSince: new Map(Object.entries(since)),
    now: NOW,
    spawn: null,
    answer,
    interrupt: vi.fn(),
    newAgent: vi.fn(),
  } as unknown as CockpitApi;
  return { api, answer, select, answerQuestion, dismissQuestion };
}

const mount = (api: CockpitApi, onOpenDoctor?: () => void) => act(() => root.render(<Inbox c={api} onOpenDoctor={onOpenDoctor} />));
const cards = () => Array.from(host.querySelectorAll<HTMLElement>('[data-ib-card]'));
const kinds = () => cards().map((c) => c.dataset.kind);
const press = (key: string, target: Element = document.activeElement ?? document.body) =>
  act(() => {
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });

describe('one list, one card', () => {
  it('puts every card that needs you in one list, high risk and consent first, then the oldest', () => {
    const { api } = fixture({ questions: [question(), consent, budget] });
    mount(api);
    expect(host.querySelectorAll('ul[aria-label="Needs you"]')).toHaveLength(1);
    // rank 0: consent (40s old), high-risk approval (5s) · rank 2, oldest first: low-risk approval (5m), question (4m)
    // · rank 3: budget · rank 4: error
    expect(kinds()).toEqual(['consent', 'approval', 'approval', 'question', 'budget', 'error']);
    const approvals = cards().filter((c) => c.dataset.kind === 'approval');
    expect(approvals.map((c) => c.dataset.risk)).toEqual(['high', 'low']);
    // the cards are 12px apart by one rule (no per-kind box): a single list, no nested boxes
    expect(host.querySelectorAll('.ck-list')).toHaveLength(0);
  });

  it('gives every card the same shape: a title, a quiet chip, at most one action row, one age format', () => {
    const { api } = fixture({ questions: [question(), consent, budget] });
    mount(api);
    for (const card of cards()) {
      expect(card.querySelector('.ib-title')?.textContent?.length).toBeGreaterThan(3);
      expect(card.querySelectorAll('.ib-actions').length).toBeLessThanOrEqual(1);
      expect(card.querySelectorAll('.ib-glyph')).toHaveLength(1);
    }
    const ages = Array.from(host.querySelectorAll('.ib-age')).map((a) => a.textContent);
    expect(ages.length).toBeGreaterThanOrEqual(5);
    for (const age of ages) expect(age).toMatch(/^(now|\d+[smhd])$/);
    // no prefix and no "ago" on any card
    expect(host.textContent).not.toMatch(/\basked\b|\bwaiting \d|ran out \d|\d[smhd] ago/);
  });

  it('puts Deny before Allow on the high-risk card and never lets a number key answer it', () => {
    const { api, answer } = fixture();
    mount(api);
    const high = cards().find((c) => c.dataset.risk === 'high')!;
    expect(Array.from(high.querySelectorAll('[data-ib="answer"]')).map((b) => b.textContent)).toEqual(['Deny', 'Allow once']);
    expect(high.hasAttribute('data-guard')).toBe(true);
    press('1', high.querySelector('button')!);
    expect(answer).not.toHaveBeenCalled();
  });

  it('uses the same neutral words on the low-risk card, whatever the agent menu says', () => {
    const { api } = fixture();
    mount(api);
    const low = cards().find((c) => c.dataset.risk === 'low')!;
    expect(Array.from(low.querySelectorAll('[data-ib="answer"]')).map((b) => b.textContent)).toEqual(['Allow once', 'Always allow npm test', 'Deny']);
    expect(host.textContent).not.toMatch(/tell Claude what to do/);
    // and the project shows, never an id
    expect(low.querySelector('.ib-meta')?.textContent).toBe('Claude Code · shop');
  });

  it('names a separate copy by its project, never by the id of its folder', () => {
    const { api } = fixture({
      panes: [
        { id: 'lo', agent: 'claude', status: 'waiting_input', cwd: 'C:/work/shop/.puppet-master/worktrees/7d4cb2b3-2-209c' },
        { id: 'ps', agent: 'powershell', status: 'error', cwd: 'C:/work/shop/.puppet-master/worktrees/7d4cb2b3-2-209c' },
      ],
    });
    mount(api);
    expect(host.querySelector('[data-kind="approval"] .ib-meta')?.textContent).toBe('Claude Code · shop · separate copy');
    expect(host.querySelector('[data-kind="error"] .ib-meta')?.textContent).toBe('shop · separate copy');
    expect(host.textContent).not.toContain('7d4cb2b3');
  });

  it('is the same list on a mixed inbox with an error: what happened, what is safe, one step', () => {
    const { api } = fixture();
    mount(api);
    const error = cards().find((c) => c.dataset.kind === 'error')!;
    expect(error.querySelector('.ib-title')?.textContent).toBe('PowerShell stopped');
    expect(error.querySelector('.ib-reason')?.textContent).toContain('Your files are as it left them.');
    expect(error.textContent).not.toMatch(/hit an error/);
  });
});

describe('keyboard', () => {
  it('focuses the first card when it opens: a high-risk first card puts focus on Deny', () => {
    const { api } = fixture();
    mount(api);
    const first = cards()[0];
    expect(first.dataset.risk).toBe('high');
    expect(document.activeElement?.textContent).toBe('Deny');
    expect(first.contains(document.activeElement)).toBe(true);
    expect(document.activeElement?.hasAttribute('data-ring')).toBe(true);
  });

  it('J and K move between cards; a card shows a ring when it has the focus', () => {
    const { api } = fixture();
    mount(api);
    press('j');
    expect(document.activeElement).toBe(cards()[1]);
    expect(cards()[1].hasAttribute('data-ring')).toBe(true);
    press('j');
    expect(document.activeElement).toBe(cards()[2]);
    press('k');
    expect(document.activeElement).toBe(cards()[1]);
  });

  it('1 answers with the first visible answer, 2 with the second', () => {
    const { api, answer } = fixture();
    mount(api);
    press('j'); // the low-risk approval is second
    const low = cards()[1];
    expect(low.dataset.risk).toBe('low');
    press('1');
    expect(answer).toHaveBeenCalledWith('lo', 'p-low', 'yes', 'Yes');
    press('3');
    expect(answer).toHaveBeenLastCalledWith('lo', 'p-low', 'no', 'No, tell Claude what to do differently');
  });

  it('O opens the terminal of the focused card and Esc dismisses a question', () => {
    const { api, select, dismissQuestion } = fixture({ questions: [question({ pane_id: 'lo' })] });
    mount(api);
    const q = cards().find((c) => c.dataset.kind === 'question')!;
    act(() => q.focus());
    press('o');
    expect(select).toHaveBeenCalledWith('lo');
    press('Escape');
    expect(dismissQuestion).toHaveBeenCalledWith('q1');
  });

  it('leaves typing alone: keys in the answer field do nothing', () => {
    const { api, answerQuestion } = fixture({ questions: [question({ allow_free_text: true })] });
    mount(api);
    const input = host.querySelector('input')!;
    act(() => input.focus());
    press('1', input);
    press('j', input);
    expect(answerQuestion).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(input);
  });

  it('shows a quiet hint under the title, and only offers number keys when one can answer', () => {
    const { api } = fixture();
    mount(api);
    expect(host.querySelector('.ib-hint')?.textContent).toContain('move');
    expect(host.querySelector('.ib-hint')?.textContent).toContain('answer');
    expect(host.querySelectorAll('.ib-hint .ib-kbd').length).toBeGreaterThanOrEqual(4);
    // a lone high-risk card: nothing a number key may answer
    const only = fixture({ panes: [{ id: 'hi', agent: 'codex', status: 'waiting_input', cwd: 'C:\\work\\billing' }], since: { hi: NOW - 1000 } });
    mount(only.api);
    expect(host.querySelector('.ib-hint')?.textContent).not.toContain('answer');
  });
});

describe('empty and first-run states', () => {
  const calm = () => fixture({ panes: [{ id: 'idle', agent: 'codex', status: 'idle', cwd: 'C:\\work\\shop' }] });

  it('is calm when nothing needs you, with no hint and no list', () => {
    const { api } = calm();
    mount(api);
    expect(host.querySelector('h1')?.textContent).toBe('Nothing needs you');
    expect(host.querySelector('.ck-doto')?.textContent).toBe('00');
    expect(host.querySelector('.ib-hint')).toBeNull();
    expect(host.querySelector('ul[aria-label="Needs you"]')).toBeNull();
  });

  it('welcomes a first-time user with one next step and a quiet way to Simple mode', () => {
    window.localStorage.clear();
    const { api } = fixture({ panes: [] });
    const open = vi.fn();
    mount(api, open);
    expect(host.querySelector('h1')?.textContent).toBe('Welcome to Puppet Master');
    expect(host.querySelector('.ck-sub')?.textContent).toBe('Your AI helpers work here. First, check which ones this PC has.');
    // no "00" counter on a welcome
    expect(host.querySelector('.ck-doto')).toBeNull();
    const buttons = Array.from(host.querySelectorAll('button'));
    const setUp = buttons.find((b) => b.textContent === 'Set up your agents')!;
    expect(setUp.className).toContain('ck-btn--primary');
    act(() => setUp.click());
    expect(open).toHaveBeenCalled();
    const simple = buttons.find((b) => b.textContent === 'Use Simple mode')!;
    expect(host.textContent).toContain("I don't write code.");
    expect(simple.className).toContain('ck-btn--plain');
    act(() => simple.click());
    expect(settings.patchSettings).toHaveBeenCalledWith({ simple_mode: true });
  });

  it('one welcome: "Set up your agents" opens the first-run flow when the cockpit says so, and errors still open Set up agents', () => {
    window.localStorage.clear();
    const { api } = fixture({ panes: [] });
    const doctor = vi.fn();
    const firstRun = vi.fn();
    act(() => root.render(<Inbox c={api} onOpenDoctor={doctor} onSetUp={firstRun} />));
    const setUp = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Set up your agents')!;
    act(() => setUp.click());
    expect(firstRun).toHaveBeenCalledTimes(1);
    expect(doctor).not.toHaveBeenCalled();
  });

  it('while the first-run flow is on screen the Inbox welcome offers no second way in', () => {
    window.localStorage.clear();
    const { api } = fixture({ panes: [] });
    act(() => root.render(<Inbox c={api} onOpenDoctor={vi.fn()} onSetUp={null} />));
    expect(host.querySelector('h1')?.textContent).toBe('Welcome to Puppet Master');
    const labels = Array.from(host.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels).not.toContain('Set up your agents');
    expect(labels).not.toContain('Use Simple mode');
  });

  it('is not a welcome once something was ever opened', () => {
    const { api } = fixture({ panes: [] });
    mount(api, () => {});
    expect(host.querySelector('h1')?.textContent).toBe('Nothing needs you');
    expect(Array.from(host.querySelectorAll('button')).some((b) => b.textContent === 'Use Simple mode')).toBe(false);
  });
});
