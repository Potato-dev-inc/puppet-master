// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveNeedsYou } from '../../lib/attention';
import { fakeCompanionTransport, type CompanionView } from '../../lib/companion';
import { isLimit, type Question } from '../../lib/questions';
import { keepGoingContinue, keepGoingSettingsGet, keepGoingTargets, type KeepGoingOffer } from '../../lib/keep-going';
import { LimitCard } from './LimitCard';

// The picker's API is mocked: the card's tests check what is asked, never a real hand-over.
vi.mock('../../lib/keep-going', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/keep-going')>()),
  keepGoingTargets: vi.fn(),
  keepGoingContinue: vi.fn(),
  keepGoingSettingsGet: vi.fn(),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

// The words the backend puts on a card that offers Codex (handoff/card_tests.rs).
const OFFER_TEXT = 'Codex gets a short brief and works in the same folder. Claude Code in this pane is stopped first.';

const usage: Question = {
  id: 'l1',
  agent: 'Claude Code',
  pane_id: 'p-claude',
  question: 'Claude Code ran out of usage. It can continue at 14:05.',
  context: OFFER_TEXT,
  choices: [
    { id: 'continue:codex', label: 'Continue on Codex' },
    { id: 'remind', label: 'Remind me at 14:05' },
    { id: 'stop', label: 'Stop here' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: 0,
  expires_at_ms: 43_200_000,
  state: 'pending',
  limit: { kind: 'usage_limit', agent_label: 'Claude Code', window: 'session limit', reset_at_ms: 1, has_run: true },
};

function render(q: Question, extra: { paneName?: string; onOpenPane?: () => void } = {}) {
  const onAnswer = vi.fn();
  act(() => {
    root.render(
      <ul>
        <LimitCard q={q} now={120_000} onAnswer={onAnswer} {...extra} />
      </ul>,
    );
  });
  return onAnswer;
}

const buttons = () => [...host.querySelectorAll('button')].map((b) => b.textContent);
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);
const sheetButton = (text: string) => [...document.querySelectorAll('.ck-sheet button')].find((b) => b.textContent === text) as HTMLButtonElement | undefined;

describe('LimitCard', () => {
  it('says what happened, offers Continue on Codex first, then its two choices, and Other agent…', () => {
    render(usage);
    expect(host.querySelector('.ib-title')?.textContent).toBe('Claude Code ran out of usage. It can continue at 14:05.');
    // the one age format, no "ran out" prefix
    expect(host.querySelector('.ib-age')?.textContent).toBe('2m');
    expect(buttons()).toEqual(['Continue on Codex', 'Remind me at 14:05', 'Stop here', 'Other agent…']);
    expect(host.querySelector('.ib-reason')?.textContent).toBe(OFFER_TEXT);
    expect(button('Continue on Codex')!.className).toContain('ck-btn--primary');
    expect(button('Remind me at 14:05')!.className).not.toContain('ck-btn--primary');
  });

  it('has no hand-off button of its own: the backend choices answer (Continue on Codex included), Other agent… opens the picker', () => {
    render(usage, { onOpenPane: () => {} });
    for (const label of buttons()) {
      expect(label).not.toMatch(/hand|continue with|resume|switch|cancel/i);
    }
  });

  it('never suggests another login of the same vendor', () => {
    render(usage, { paneName: 'Claude Code 1', onOpenPane: () => {} });
    expect(host.textContent ?? '').not.toMatch(/login|account|personal|use your|switch to/i);
  });

  it('answers with the choice ids the backend reads', () => {
    const onAnswer = render(usage);
    act(() => button('Continue on Codex')!.click());
    act(() => button('Remind me at 14:05')!.click());
    act(() => button('Stop here')!.click());
    expect(onAnswer.mock.calls).toEqual([[{ choice_id: 'continue:codex' }], [{ choice_id: 'remind' }], [{ choice_id: 'stop' }]]);
  });

  it('names the pane and can open its terminal', () => {
    const open = vi.fn();
    render(usage, { paneName: 'Claude Code 1', onOpenPane: open });
    expect(host.querySelector('.ib-meta')?.textContent).toBe('Claude Code 1');
    act(() => button('Open terminal')!.click());
    expect(open).toHaveBeenCalledOnce();
  });

  it('with its own wait on, the backend says so and the card shows it', () => {
    render({ ...usage, context: `Claude Code is set to continue on its own. Continuing on Codex stops that. ${OFFER_TEXT}` });
    expect(host.querySelector('.ib-reason')?.textContent).toContain('set to continue on its own');
  });

  it('a limit with no printed reset says so and offers a reminder in an hour', () => {
    render({
      ...usage,
      question: "Claude Code ran out of usage. It didn't say when it can continue.",
      choices: [
        { id: 'continue:codex', label: 'Continue on Codex' },
        { id: 'remind', label: 'Remind me in 1 hour' },
        { id: 'stop', label: 'Stop here' },
      ],
    });
    expect(buttons()).toEqual(['Continue on Codex', 'Remind me in 1 hour', 'Stop here', 'Other agent…']);
  });

  it('a pane without a run has nothing to stop: Dismiss, and no Other agent…', () => {
    render({ ...usage, choices: [usage.choices[0], usage.choices[1], { id: 'stop', label: 'Dismiss' }], limit: { ...usage.limit!, has_run: false } });
    expect(buttons()).toEqual(['Continue on Codex', 'Remind me at 14:05', 'Dismiss']);
  });

  it('the reminder card has one button and says the limit should have reset', () => {
    const onAnswer = render({
      ...usage,
      id: 'r1',
      question: "Claude Code's usage limit should have reset by now.",
      choices: [{ id: 'ok', label: 'Got it' }],
      limit: { kind: 'reset_reminder', agent_label: 'Claude Code' },
    });
    expect(buttons()).toEqual(['Got it']);
    expect(host.querySelector('.ib-age')?.textContent).toBe('2m');
    act(() => button('Got it')!.click());
    expect(onAnswer).toHaveBeenCalledWith({ choice_id: 'ok' });
  });
});

describe('Other agent… on a limit card', () => {
  const offers: KeepGoingOffer[] = [
    { target: { agent_id: 'codex' }, label: 'Codex', ok: true, whyNot: null },
    { target: { agent_id: 'cursor' }, label: 'Cursor', ok: false, whyNot: 'Not signed in' },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(keepGoingTargets).mockResolvedValue(offers);
    vi.mocked(keepGoingSettingsGet).mockResolvedValue({ order: [], ask_back: true });
  });

  it('is not offered when the card has no pane to stop', () => {
    render({ ...usage, pane_id: undefined });
    expect(buttons()).toEqual(['Continue on Codex', 'Remind me at 14:05', 'Stop here']);
  });

  it('opens the picker for this pane, with its agents listed', async () => {
    render(usage);
    await act(async () => button('Other agent…')!.click());
    expect(keepGoingTargets).toHaveBeenCalledWith('p-claude');
    expect(host.querySelector('#ck-keep-going-title')?.textContent).toBe('Continue on another agent');
    expect(host.querySelector('.ck-keep-going-sheet')).not.toBeNull();
  });

  it('starts the chosen agent with the saved move-back setting and closes the picker', async () => {
    vi.mocked(keepGoingContinue).mockResolvedValue(undefined as never);
    render(usage);
    await act(async () => button('Other agent…')!.click());
    await act(async () => sheetButton('Continue on Codex')!.click());
    expect(keepGoingContinue).toHaveBeenCalledWith('p-claude', 'codex', true, 'l1');
    expect(host.querySelector('.ck-keep-going-sheet')).toBeNull();
  });

  it('closes the picker with Cancel and starts nothing', async () => {
    render(usage);
    await act(async () => button('Other agent…')!.click());
    await act(async () => button('Cancel')!.click());
    expect(host.querySelector('.ck-keep-going-sheet')).toBeNull();
    expect(keepGoingContinue).not.toHaveBeenCalled();
  });
});

describe('a limit card on a companion\'s pane', () => {
  const resetMs = Date.now() + 3 * 3_600_000;
  // The backend never offers Continue on a companion's pane (`NotHere`): two choices and no hand-over line.
  const companionCard: Question = { ...usage, pane_id: 'companion-mochi', context: undefined, choices: usage.choices.slice(1), limit: { ...usage.limit!, reset_at_ms: resetMs } };
  const mochi: CompanionView = {
    id: 'mochi',
    name: 'Mochi',
    avatar: 'mochi',
    persona: 'calm',
    agentType: 'claude',
    agentLabel: 'Claude Code',
    folderName: 'mochi',
    state: 'needs_you',
    stateLine: 'Needs you',
    paneId: 'companion-mochi',
    needs: null,
    pendingMemory: 0,
    nextCheckin: null,
    restarts: 0,
    autoStart: true,
  };
  const agents = [
    { id: 'claude', label: 'Claude Code', ok: true, reason: null },
    { id: 'codex', label: 'Codex', ok: true, reason: null },
  ];

  function renderCompanion(card: Question, companion: CompanionView | null = mochi) {
    const transport = fakeCompanionTransport({ companions: [mochi], agents });
    act(() => {
      root.render(
        <ul>
          <LimitCard q={card} now={120_000} onAnswer={vi.fn()} companion={companion ?? undefined} companionTransport={transport} />
        </ul>,
      );
    });
    return transport;
  }

  it('adds "Wake Mochi on…" after the two choices, and no Other agent… (the companion has its own)', () => {
    renderCompanion(companionCard);
    expect(buttons()).toEqual(['Remind me at 14:05', 'Stop here', 'Wake Mochi on…']);
  });

  it('adds no Wake button for a pane that is not a companion\'s; Other agent… is there instead', () => {
    renderCompanion(companionCard, null);
    expect(buttons()).toEqual(['Remind me at 14:05', 'Stop here', 'Other agent…']);
  });

  it('adds nothing to the reminder card', () => {
    renderCompanion({ ...companionCard, choices: [{ id: 'ok', label: 'Got it' }], limit: { kind: 'reset_reminder', agent_label: 'Claude Code' } });
    expect(buttons()).toEqual(['Got it']);
  });

  it('opens a sheet with another agent picked and the end of the limit as the "until" time', async () => {
    renderCompanion(companionCard);
    await act(async () => button('Wake Mochi on…')!.click());
    const sheet = document.querySelector('.ck-sheet')!;
    expect(sheet.querySelector('h3')?.textContent).toBe('Wake Mochi on…');
    expect(sheet.querySelector<HTMLSelectElement>('select[aria-label="Agent"]')!.value).toBe('codex');
    const at = new Date(resetMs);
    const expected = `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
    expect(sheet.querySelector<HTMLInputElement>('input[type="time"]')!.value).toBe(expected);
  });

  it('wakes it on the other agent with one update call, then closes the sheet', async () => {
    const transport = renderCompanion(companionCard);
    await act(async () => button('Wake Mochi on…')!.click());
    const wake = [...document.querySelectorAll('.ck-sheet button')].find((b) => b.textContent === 'Wake Mochi') as HTMLButtonElement;
    await act(async () => wake.click());
    const updates = transport.calls.filter((c) => c.method === 'update');
    expect(updates).toHaveLength(1);
    const [id, patch] = updates[0].args as [string, { agentType: string; untilMs: number | null }];
    expect([id, patch.agentType]).toEqual(['mochi', 'codex']);
    // The limit ends in about 3 hours; the sheet keeps the minute, so the moment is within a minute of it.
    expect(Math.abs((patch.untilMs ?? 0) - resetMs)).toBeLessThan(60_000);
    expect(document.querySelector('.ck-sheet')).toBeNull();
  });

  it('Cancel closes the sheet and moves nothing', async () => {
    const transport = renderCompanion(companionCard);
    await act(async () => button('Wake Mochi on…')!.click());
    const cancel = [...document.querySelectorAll('.ck-sheet button')].find((b) => b.textContent === 'Cancel') as HTMLButtonElement;
    await act(async () => cancel.click());
    expect(document.querySelector('.ck-sheet')).toBeNull();
    expect(transport.calls.filter((c) => c.method === 'update')).toHaveLength(0);
  });
});

describe('a limit card in the Needs you number', () => {
  it('counts once, as its own kind, on the pane it belongs to', () => {
    expect(isLimit(usage)).toBe(true);
    const items = deriveNeedsYou({ panes: [{ id: 'p-claude', status: 'waiting_input' }], questions: [usage] });
    expect(items.map((i) => i.id)).toEqual(['approval:p-claude', 'limit:l1']);
    expect(items[1]).toMatchObject({ kind: 'limit', paneId: 'p-claude' });
  });
});
