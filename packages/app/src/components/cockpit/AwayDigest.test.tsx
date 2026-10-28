// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Digest, DigestFocus } from '../../lib/cockpit-api';
import { AwayDigest, isFocusEmpty } from './AwayDigest';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_800_000_000_000;

function digest(over: Partial<Digest> = {}): Digest {
  return {
    sinceMs: NOW - 23 * 60_000,
    nowMs: NOW,
    agentsWorked: 2,
    turns: 5,
    lines: [
      {
        paneId: 'p1',
        agent: 'Claude',
        turns: 3,
        sentence: 'Refactored the login flow',
        endedAtMs: NOW - 4 * 60_000,
        end: 'idle',
      },
      {
        paneId: 'p2',
        agent: 'Codex',
        turns: 1,
        sentence: 'Fixed the failing build',
        endedAtMs: NOW - 90_000,
        end: 'idle',
      },
    ],
    needsYou: [],
    ...over,
  };
}

function focusSummary(over: Partial<DigestFocus['counts']> = {}): DigestFocus {
  return {
    line: 'While you focused (45 min): 4 turns done, 1 failed, 2 asked for you. 1 still waits.',
    minutes: 45,
    endedBy: 'timer',
    counts: { turnsDone: 4, agents: 2, failed: 1, asked: 2, urgent: 0, stillWaiting: 1, held: 3, ...over },
  };
}

let host: HTMLDivElement;
let root: Root;

function render(props: Partial<Parameters<typeof AwayDigest>[0]> = {}) {
  const handlers = {
    onDismiss: vi.fn(),
    onOpenPane: vi.fn(),
    onOpenInbox: vi.fn(),
  };
  act(() => {
    root.render(<AwayDigest digest={digest()} now={NOW} {...handlers} {...props} />);
  });
  return handlers;
}

const text = () => host.textContent ?? '';
const button = (label: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);
const click = (el: Element | undefined) => {
  expect(el).toBeTruthy();
  act(() => {
    (el as HTMLElement).click();
  });
};

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('AwayDigest', () => {
  it('has a plain title and puts the count in the sentence: no big number, which belongs to the Inbox', () => {
    render({ digest: digest({ agentsWorked: 2 }) });
    expect(host.querySelector('.ck-doto')).toBeNull();
    expect(host.querySelector('h1')?.textContent).toBe('While you were away');
    expect(host.querySelector('.ck-sub')?.textContent).toContain('2 agents worked');
    expect(host.querySelector('section')?.getAttribute('aria-label')).toBe('While you were away');
  });

  it('uses the singular title for one agent', () => {
    render({ digest: digest({ agentsWorked: 1, lines: [digest().lines[0]] }) });
    expect(host.querySelector('h1')?.textContent).toBe('While you were away');
    expect(host.querySelector('.ck-sub')?.textContent).toContain('1 agent worked.');
  });

  it('shows the elapsed time and each line sentence', () => {
    render();
    expect(host.querySelector('.ck-sub')?.textContent).toBe('2 agents worked. You were away for 23m. Here is what happened.');
    const lines = host.querySelectorAll('.ck-away-line');
    expect(lines).toHaveLength(2);
    expect(text()).toContain('Refactored the login flow');
    expect(text()).toContain('Fixed the failing build');
    // the one age format: no "ago" on the card, the full phrase in the tooltip
    expect(lines[1].querySelector('.ck-row-time')?.textContent).toBe('1m');
    expect(lines[1].querySelector('.ck-row-time')?.getAttribute('title')).toBe('1m ago');
  });

  it('shows "turns" only when a line has more than one turn', () => {
    render();
    const lines = host.querySelectorAll('.ck-away-line');
    expect(lines[0].textContent).toContain('3 turns');
    expect(lines[1].textContent).not.toContain('turns');
  });

  it('hides the Needs you section and Open Inbox when nothing needs the user', () => {
    render({ digest: digest({ needsYou: [] }) });
    expect(text()).not.toContain('Needs you');
    expect(host.querySelector('.ck-away-needs')).toBeNull();
    expect(button('Open Inbox')).toBeUndefined();
  });

  it('lists needs-you rows with the prompt text', () => {
    render({
      digest: digest({
        needsYou: [{ paneId: 'p3', agent: 'Gemini', status: 'waiting_input', prompt: 'Overwrite config.json?' }],
      }),
    });
    expect(text()).toContain('Needs you');
    const need = host.querySelector('.ck-away-need');
    expect(need?.textContent).toContain('Gemini');
    expect(need?.textContent).toContain('Overwrite config.json?');
  });

  it('falls back to status text when a need has no prompt', () => {
    render({
      digest: digest({
        needsYou: [
          { paneId: 'p3', agent: 'Gemini', status: 'waiting_input' },
          { paneId: 'p4', agent: 'Codex', status: 'error' },
        ],
      }),
    });
    const needs = host.querySelectorAll('.ck-away-need');
    expect(needs[0].textContent).toContain('Waiting for you');
    expect(needs[1].textContent).toContain('Hit an error');
  });

  it('"Got it" calls onDismiss', () => {
    const h = render();
    click(button('Got it'));
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
  });

  it('Escape calls onDismiss and the listener is removed on unmount', () => {
    const h = render();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
    act(() => root.unmount());
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
    root = createRoot(host);
  });

  it('clicking a need row calls onOpenPane with its pane id', () => {
    const h = render({
      digest: digest({
        needsYou: [{ paneId: 'p3', agent: 'Gemini', status: 'waiting_input', prompt: 'Proceed?' }],
      }),
    });
    click(host.querySelector('.ck-away-need') as HTMLElement);
    expect(h.onOpenPane).toHaveBeenCalledWith('p3');
  });

  it('"Open Inbox" calls onOpenInbox when there are needs', () => {
    const h = render({
      digest: digest({
        needsYou: [{ paneId: 'p3', agent: 'Gemini', status: 'waiting_input' }],
      }),
    });
    click(button('Open Inbox'));
    expect(h.onOpenInbox).toHaveBeenCalledTimes(1);
  });
});

describe('AwayDigest, end of Focus', () => {
  it('keeps the normal title and sentence when the digest has no focus summary', () => {
    render({ digest: digest({ focus: undefined }) });
    expect(host.querySelector('h1')?.textContent).toBe('While you were away');
    expect(text()).not.toContain('While you focused');
    expect(host.querySelector('.ck-sub')?.textContent).toContain('agents worked');
  });

  it('shows the "While you focused" title and the focus line as the sub line', () => {
    const focus = focusSummary();
    render({ digest: digest({ focus }) });
    expect(host.querySelector('h1')?.textContent).toBe('While you focused');
    expect(host.querySelector('section')?.getAttribute('aria-label')).toBe('While you focused');
    expect(host.querySelector('.ck-sub')?.textContent).toBe(focus.line);
    expect(text()).not.toContain('While you were away');
  });

  it('still lists the agent lines and the Needs you rows under the focus line', () => {
    render({
      digest: digest({
        focus: focusSummary(),
        needsYou: [{ paneId: 'p3', agent: 'Gemini', status: 'waiting_input', prompt: 'Proceed?' }],
      }),
    });
    expect(host.querySelectorAll('.ck-away-line')).toHaveLength(2);
    expect(text()).toContain('Needs you');
    expect(host.querySelector('.ck-away-need')?.textContent).toContain('Gemini');
  });

  it('shows a summary that only has something still waiting', () => {
    render({ digest: digest({ focus: focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, held: 0, stillWaiting: 1 }) }) });
    expect(host.querySelector('h1')?.textContent).toBe('While you focused');
  });

  it('"Got it" calls onDismiss on a focus digest', () => {
    const h = render({ digest: digest({ focus: focusSummary() }) });
    click(button('Got it'));
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
  });

  it('Escape calls onDismiss on a focus digest', () => {
    const h = render({ digest: digest({ focus: focusSummary() }) });
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
  });

  it('an empty focus summary shows nothing and is cleared at once', () => {
    const h = render({ digest: digest({ focus: focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, stillWaiting: 0, held: 0 }) }) });
    expect(host.querySelector('.ck-away')).toBeNull();
    expect(text()).toBe('');
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
  });

  it('Escape does nothing extra when an empty focus summary is shown as nothing', () => {
    const h = render({ digest: digest({ focus: focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, stillWaiting: 0, held: 0 }) }) });
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(h.onDismiss).toHaveBeenCalledTimes(1);
  });

  it('isFocusEmpty is true only when every count is zero', () => {
    expect(isFocusEmpty(focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, urgent: 0, stillWaiting: 0, held: 0 }))).toBe(true);
    expect(isFocusEmpty(focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, urgent: 0, stillWaiting: 0, held: 1 }))).toBe(false);
    expect(isFocusEmpty(focusSummary({ turnsDone: 1, agents: 1, failed: 0, asked: 0, urgent: 0, stillWaiting: 0, held: 0 }))).toBe(false);
    expect(isFocusEmpty(focusSummary({ turnsDone: 0, agents: 0, failed: 0, asked: 0, urgent: 1, stillWaiting: 0, held: 0 }))).toBe(false);
  });
});
