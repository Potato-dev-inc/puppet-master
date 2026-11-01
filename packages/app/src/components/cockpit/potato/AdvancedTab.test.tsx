// @vitest-environment jsdom
// The Advanced tab of the bot panel: the three sections in order, read from the app's own Tauri commands for this bot,
// and an add that calls the open-work command with the bot's id and the text.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AdvancedTab } from './AdvancedTab';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'potato_notify_get':
        return 'reply';
      case 'potato_open_work_list':
        return { items: [], helpers: [] };
      case 'potato_open_work_add':
        return { id: 'ow-00000001', text: 'Call Sam', done: false };
      case 'potato_memory_list':
        return [];
      default:
        throw new Error(`unexpected command ${command}`);
    }
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function show() {
  await act(async () => root.render(<AdvancedTab bot={{ id: 'nimbus', name: 'Nimbus' }} />));
  await act(async () => {});
}

describe('AdvancedTab', () => {
  it('shows Notifications, Open work and Memory, in that order', async () => {
    await show();
    const titles = [...host.querySelectorAll('.ck-potato-section-title')].map((t) => t.textContent);
    expect(titles).toEqual(['Notifications', 'Open work', 'Memory']);
  });

  it('reads each section for this bot from its own command', async () => {
    await show();
    expect(invoke).toHaveBeenCalledWith('potato_notify_get', { botId: 'nimbus' });
    expect(invoke).toHaveBeenCalledWith('potato_open_work_list', { botId: 'nimbus' });
    expect(invoke).toHaveBeenCalledWith('potato_memory_list', { botId: 'nimbus' });
  });

  it('adds an open-work item through the add command with the bot id and the text', async () => {
    await show();
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Add an item"]')!.click());
    const input = host.querySelector<HTMLInputElement>('.ck-potato-add-input')!;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    setter.call(input, 'Call Sam');
    await act(async () => input.dispatchEvent(new Event('input', { bubbles: true })));
    await act(async () => {
      host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    });
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('potato_open_work_add', { botId: 'nimbus', text: 'Call Sam' });
  });
});
