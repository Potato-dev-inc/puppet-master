// @vitest-environment jsdom
// A message queued behind a menu or a setup screen says what is true (the agent is not working) and keeps Cancel.
// The sentence is the one the Rust outbox sends (`WAIT_MENU` in pane_outbox/model.rs), read from the source so the
// two cannot drift apart.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { OutboxMessage } from '../../lib/paneOutbox';
import { PendingMessages } from './PendingMessages';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const model = readFileSync(resolve(__dirname, '../../../src-tauri/src/pane_outbox/model.rs'), 'utf8');
const constant = (name: string) => new RegExp(`pub const ${name}: &str = "([^"]+)";`).exec(model)?.[1] ?? '';
const WAIT_MENU = constant('WAIT_MENU');
const WAIT_BUSY = constant('WAIT_BUSY');

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

const message = (over: Partial<OutboxMessage> = {}): OutboxMessage => ({
  id: 'm1',
  text: 'hi',
  createdAt: 1,
  state: 'queued',
  waitingFor: WAIT_MENU,
  error: null,
  ...over,
});

describe('a message that waits behind a menu', () => {
  it('has a sentence of its own that does not say the agent is working', () => {
    expect(WAIT_MENU).not.toBe('');
    expect(WAIT_MENU).not.toBe(WAIT_BUSY);
    expect(WAIT_MENU.toLowerCase()).not.toContain('working');
    expect(WAIT_MENU).toContain('answer it first');
    expect(WAIT_BUSY).toContain('working');
  });

  it('shows that sentence and keeps Cancel', async () => {
    const onCancel = vi.fn(async (): Promise<string | null> => null);
    act(() => root.render(<PendingMessages messages={[message()]} onCancel={onCancel} onHide={vi.fn()} />));
    expect(host.querySelector('[data-testid="chat-pending-waiting"]')?.textContent).toBe(WAIT_MENU);
    expect(host.textContent).not.toContain('The agent is working');
    const cancel = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')!;
    await act(async () => cancel.click());
    expect(onCancel).toHaveBeenCalledWith('m1');
  });
});
