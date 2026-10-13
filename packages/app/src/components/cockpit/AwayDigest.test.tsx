// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Digest } from '../../lib/cockpit-api';
import { AwayDigest } from './AwayDigest';

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
  it('shows the zero-padded counter and the plural title', () => {
    render({ digest: digest({ agentsWorked: 2 }) });
    const counter = host.querySelector('.ck-doto');
    expect(counter?.textContent).toBe('02');
    expect(counter?.getAttribute('aria-label')).toBe('2 agents worked');
    expect(host.querySelector('h1')?.textContent).toBe('2 agents worked while you were away');
    expect(host.querySelector('section')?.getAttribute('aria-label')).toBe('While you were away');
  });

  it('uses the singular title for one agent', () => {
    render({ digest: digest({ agentsWorked: 1, lines: [digest().lines[0]] }) });
    expect(host.querySelector('h1')?.textContent).toBe('1 agent worked while you were away');
  });

  it('shows the elapsed time and each line sentence', () => {
    render();
    expect(host.querySelector('.ck-sub')?.textContent).toBe('Away for 23m. Here is what happened.');
    const lines = host.querySelectorAll('.ck-away-line');
    expect(lines).toHaveLength(2);
    expect(text()).toContain('Refactored the login flow');
    expect(text()).toContain('Fixed the failing build');
    expect(lines[1].querySelector('.ck-row-time')?.textContent).toBe('1m ago');
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
