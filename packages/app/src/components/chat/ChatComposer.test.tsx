// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatComposer, TOO_LONG_TEXT } from './ChatComposer';

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

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const field = () => host.querySelector('textarea') as HTMLTextAreaElement;
const sendButton = () => host.querySelector('button[type="submit"]') as HTMLButtonElement;
const note = () => host.querySelector('[data-testid="chat-composer-note"]')?.textContent ?? '';

function render(onSend: (text: string) => Promise<string | null>, blockedReason: string | null = null) {
  act(() => root.render(<ChatComposer onSend={onSend} blockedReason={blockedReason} />));
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

describe('ChatComposer', () => {
  it('Enter sends the text, trimmed, and empties the box', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('  fix the login test \n');
    const enter = press('Enter');
    await flush();
    expect(enter.defaultPrevented).toBe(true);
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith('fix the login test');
    expect(field().value).toBe('');
  });

  it('Shift+Enter makes a new line and sends nothing', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('line one');
    const event = press('Enter', { shiftKey: true });
    await flush();
    expect(event.defaultPrevented).toBe(false);
    expect(onSend).not.toHaveBeenCalled();
    expect(field().value).toBe('line one');
  });

  it('a multi-line text goes as one message with its line breaks', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('first\nsecond');
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('first\nsecond');
  });

  it('Enter while an input method is composing a word sends nothing', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('nihon');
    press('Enter', { isComposing: true });
    await flush();
    expect(onSend).not.toHaveBeenCalled();
  });

  it('the Send button sends too, and is off for an empty box', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    expect(sendButton().disabled).toBe(true);
    type('   ');
    expect(sendButton().disabled).toBe(true);
    press('Enter');
    await flush();
    expect(onSend).not.toHaveBeenCalled();
    type('hello');
    expect(sendButton().disabled).toBe(false);
    act(() => sendButton().click());
    await flush();
    expect(onSend).toHaveBeenCalledWith('hello');
  });

  it('keeps the text and says why when the message is refused', async () => {
    const onSend = vi.fn(async () => 'Too many waiting messages.');
    render(onSend);
    type('one more');
    press('Enter');
    await flush();
    expect(field().value).toBe('one more');
    expect(note()).toContain('Too many waiting messages.');
    // Writing again clears the old complaint.
    type('one more!');
    expect(note()).toBe('');
  });

  it('is switched off with the reason when the pane cannot take messages', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend, 'Chat replies need an agent pane.');
    expect(field().disabled).toBe(true);
    expect(sendButton().disabled).toBe(true);
    expect(note()).toBe('Chat replies need an agent pane.');
    press('Enter');
    await flush();
    expect(onSend).not.toHaveBeenCalled();
  });

  it('refuses more than 8000 characters, counting characters and not bytes', async () => {
    const onSend = vi.fn(async () => null);
    render(onSend);
    type('é'.repeat(8000));
    expect(sendButton().disabled).toBe(false);
    expect(note()).toContain('8000 / 8000');
    type('x'.repeat(8001));
    expect(sendButton().disabled).toBe(true);
    expect(note()).toContain(TOO_LONG_TEXT);
    press('Enter');
    await flush();
    expect(onSend).not.toHaveBeenCalled();
  });
});
