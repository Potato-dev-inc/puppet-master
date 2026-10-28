// @vitest-environment jsdom
// The agent picker: nothing to press until another agent is picked; "until" is optional; an unusable agent is refused
// with its reason; the time becomes the next moment the clock shows it.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CompanionAgentOption } from '../../../lib/companion';
import { MovePicker } from './MovePicker';
import { setValue } from './test-helpers';

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

const agents: CompanionAgentOption[] = [
  { id: 'claude', label: 'Claude Code', ok: true, reason: null },
  { id: 'codex', label: 'Codex', ok: true, reason: null },
  { id: 'broken', label: 'Broken Agent', ok: false, reason: 'Approve this agent in Settings > Agents first.' },
];

// 2026-10-28 10:00 local.
const NOW = new Date(2026, 9, 28, 10, 0, 0).getTime();

function render(extra: Partial<Parameters<typeof MovePicker>[0]> = {}) {
  const onMove = vi.fn();
  act(() => {
    root.render(<MovePicker currentAgent="claude" agents={agents} submitLabel="Move" now={() => NOW} onMove={onMove} {...extra} />);
  });
  return onMove;
}

const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="Agent"]')!;
const time = () => host.querySelector<HTMLInputElement>('input[type="time"]');
const moveButton = () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Move');

describe('MovePicker', () => {
  it('starts on the current agent, with nothing to press', () => {
    render();
    expect(select().value).toBe('claude');
    expect(moveButton()).toBeUndefined();
    expect(time()).toBeNull();
  });

  it('lists every agent and marks the one that cannot be used', () => {
    render();
    const options = [...select().options].map((o) => [o.textContent, o.disabled]);
    expect(options).toEqual([
      ['Claude Code', false],
      ['Codex', false],
      ['Broken Agent', true],
    ]);
  });

  it('moves with no end time when none is given', () => {
    const onMove = render();
    setValue(select(), 'codex');
    expect(host.textContent).toContain('It stays on the new agent until you move it back.');
    act(() => moveButton()!.click());
    expect(onMove).toHaveBeenCalledWith('codex', null);
  });

  it('turns the time into the next moment the clock shows it', () => {
    const onMove = render();
    setValue(select(), 'codex');
    setValue(time()!, '17:00');
    expect(host.textContent).toContain('Then it goes back to the agent it is on now.');
    act(() => moveButton()!.click());
    expect(onMove).toHaveBeenCalledWith('codex', new Date(2026, 9, 28, 17, 0, 0).getTime());
  });

  it('a time that has passed today means tomorrow', () => {
    const onMove = render();
    setValue(select(), 'codex');
    setValue(time()!, '09:30');
    act(() => moveButton()!.click());
    expect(onMove).toHaveBeenCalledWith('codex', new Date(2026, 9, 29, 9, 30, 0).getTime());
  });

  it('prefills the end time and picks the agent it is told to', () => {
    const onMove = render({ initialAgent: 'codex', defaultUntilMs: new Date(2026, 9, 28, 17, 0, 0).getTime() });
    expect(select().value).toBe('codex');
    expect(time()!.value).toBe('17:00');
    act(() => moveButton()!.click());
    expect(onMove).toHaveBeenCalledWith('codex', new Date(2026, 9, 28, 17, 0, 0).getTime());
  });

  it('an agent that cannot be used cannot be moved to, and says why', () => {
    const onMove = render({ initialAgent: 'broken' });
    expect(host.textContent).toContain('Approve this agent in Settings > Agents first.');
    expect(moveButton()!.disabled).toBe(true);
    act(() => moveButton()!.click());
    expect(onMove).not.toHaveBeenCalled();
  });

  it('is disabled while a move runs', () => {
    render({ initialAgent: 'codex', busy: true });
    expect(moveButton()!.disabled).toBe(true);
    expect(select().disabled).toBe(true);
  });
});
