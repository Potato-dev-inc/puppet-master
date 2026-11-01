// @vitest-environment jsdom
// The slash-command menu of a chat session over ACP: "/" opens a listbox above the box, filtered by what is typed;
// arrows move, Enter or Tab or a click picks, Esc closes; plain Enter still sends when the menu is closed.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpCommand } from '../../lib/acpChat';
import { ChatComposer } from './ChatComposer';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const COMMANDS: AcpCommand[] = [
  { name: 'compact', description: 'Shrink the conversation', hint: null },
  { name: 'review', description: 'Review the changes', hint: 'what to review' },
  { name: 'rename', description: 'Rename the session', hint: 'new name' },
];

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

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const field = () => host.querySelector('textarea') as HTMLTextAreaElement;
const menu = () => host.querySelector('[role="listbox"]');
const options = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];

function render(onSend: (text: string) => Promise<string | null>, commands: AcpCommand[] | null = COMMANDS) {
  act(() => root.render(<ChatComposer onSend={onSend} blockedReason={null} slashCommands={commands ?? undefined} />));
}
function type(text: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field(), text);
    field().dispatchEvent(new Event('input', { bubbles: true }));
  });
}
function press(key: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  act(() => {
    field().dispatchEvent(event);
  });
  return event;
}

describe('the slash menu', () => {
  it('opens on "/" with every command, name and description, and is a listbox', () => {
    render(vi.fn(async () => null));
    expect(menu()).toBeNull();
    type('/');
    expect(menu()).not.toBeNull();
    expect(options().map((o) => o.textContent)).toEqual(['/compactShrink the conversation', '/reviewReview the changes', '/renameRename the session']);
    expect(options()[0].getAttribute('aria-selected')).toBe('true');
    expect(field().getAttribute('aria-activedescendant')).toBe(options()[0].id);
    expect(field().getAttribute('aria-expanded')).toBe('true');
  });

  it('filters by prefix as the person types and closes when nothing matches or a space is typed', () => {
    render(vi.fn(async () => null));
    type('/re');
    expect(options().map((o) => o.textContent?.split('/')[1]?.slice(0, 6))).toEqual(['review', 'rename']);
    type('/rev');
    expect(options()).toHaveLength(1);
    type('/zzz');
    expect(menu()).toBeNull();
    type('/review ');
    expect(menu()).toBeNull();
    type('hello /re');
    expect(menu()).toBeNull();
  });

  it('ArrowDown and ArrowUp move the choice and wrap round', () => {
    render(vi.fn(async () => null));
    type('/');
    press('ArrowDown');
    expect(options()[1].getAttribute('aria-selected')).toBe('true');
    expect(field().getAttribute('aria-activedescendant')).toBe(options()[1].id);
    press('ArrowUp');
    press('ArrowUp');
    expect(options()[2].getAttribute('aria-selected')).toBe('true');
    press('ArrowDown');
    expect(options()[0].getAttribute('aria-selected')).toBe('true');
  });

  it('Enter picks the chosen command, puts "/name " in the box and sends nothing', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('/re');
    press('ArrowDown');
    const enter = press('Enter');
    await flush();
    expect(enter.defaultPrevented).toBe(true);
    expect(onSend).not.toHaveBeenCalled();
    expect(field().value).toBe('/rename ');
    expect(menu()).toBeNull();
  });

  it('Tab picks too, and the hint of the command shows in the empty rest of the box', () => {
    render(vi.fn(async () => null));
    type('/rev');
    const tab = press('Tab');
    expect(tab.defaultPrevented).toBe(true);
    expect(field().value).toBe('/review ');
    expect(field().placeholder).toBe('what to review');
  });

  it('a click picks a command', () => {
    render(vi.fn(async () => null));
    type('/');
    act(() => options()[1].click());
    expect(field().value).toBe('/review ');
    expect(menu()).toBeNull();
  });

  it('Escape closes the menu without clearing the text, and typing opens it again', () => {
    render(vi.fn(async () => null));
    type('/re');
    const esc = press('Escape');
    expect(esc.defaultPrevented).toBe(true);
    expect(menu()).toBeNull();
    expect(field().value).toBe('/re');
    type('/rev');
    expect(menu()).not.toBeNull();
  });

  it('plain Enter sends once the menu is closed, and a command with its words goes out as one message', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('/review the login page');
    expect(menu()).toBeNull();
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('/review the login page');
  });

  it('is off without commands: "/" is just text and Enter sends it', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend, null);
    type('/');
    expect(menu()).toBeNull();
    expect(field().getAttribute('role')).toBeNull();
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('/');
  });

  it('is off while the box is switched off', () => {
    act(() => root.render(<ChatComposer onSend={vi.fn(async () => null)} blockedReason="Not now." slashCommands={COMMANDS} />));
    expect(menu()).toBeNull();
  });
});
