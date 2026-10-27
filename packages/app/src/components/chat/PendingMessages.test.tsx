// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { OutboxMessage } from '../../lib/paneOutbox';
import { PendingMessages } from './PendingMessages';

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
const buttons = (label: string) => [...host.querySelectorAll('button')].filter((b) => b.textContent === label);

function message(over: Partial<OutboxMessage>): OutboxMessage {
  return { id: 'm1', text: 'run the tests', createdAt: 1, state: 'queued', waitingFor: null, error: null, ...over };
}

function render(messages: OutboxMessage[], onCancel = vi.fn(async (_id: string): Promise<string | null> => null), onHide = vi.fn()) {
  act(() => root.render(<PendingMessages messages={messages} onCancel={onCancel} onHide={onHide} />));
  return { onCancel, onHide };
}

describe('PendingMessages', () => {
  it('shows a waiting message with what it waits for and a Cancel link', async () => {
    const { onCancel } = render([message({ waitingFor: 'The agent is working. Sends when the turn ends.' })]);
    expect(host.textContent).toContain('run the tests');
    expect(host.querySelector('[data-testid="chat-pending-waiting"]')?.textContent).toBe('The agent is working. Sends when the turn ends.');
    act(() => buttons('Cancel')[0].click());
    await flush();
    expect(onCancel).toHaveBeenCalledWith('m1');
  });

  it('shows why a message could not be cancelled', async () => {
    render([message({})], vi.fn(async (_id: string): Promise<string | null> => 'It is already being sent.'));
    act(() => buttons('Cancel')[0].click());
    await flush();
    expect(host.querySelector('[data-testid="chat-pending-note"]')?.textContent).toBe('It is already being sent.');
  });

  it('has no Cancel link once typing started', () => {
    render([message({ state: 'sending' })]);
    expect(host.textContent).toContain('Sending…');
    expect(buttons('Cancel')).toHaveLength(0);
  });

  it('shows a failed message with its error, Copy text and Hide', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const { onHide } = render([message({ state: 'failed', error: 'The agent is not running in this pane.' })]);
    expect(host.querySelector('[data-testid="chat-pending-error"]')?.textContent).toBe('The agent is not running in this pane.');
    expect(buttons('Cancel')).toHaveLength(0);
    act(() => buttons('Copy text')[0].click());
    await flush();
    expect(writeText).toHaveBeenCalledWith('run the tests');
    expect(host.querySelector('[data-testid="chat-pending-note"]')?.textContent).toBe('Copied.');
    act(() => buttons('Hide')[0].click());
    expect(onHide).toHaveBeenCalledWith('m1');
  });

  it('shows nothing for an empty list', () => {
    render([]);
    expect(host.innerHTML).toBe('');
  });
});
