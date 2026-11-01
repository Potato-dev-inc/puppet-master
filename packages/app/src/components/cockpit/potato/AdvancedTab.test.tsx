// @vitest-environment jsdom
// The Advanced tab of the bot panel on the fake engine: the sections in order, the Model row, the instruction history with
// Restore, the tool switches (the computer one only with the sandbox on), Rename, Export chat, and Delete with the name typed.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { save } from '@tauri-apps/plugin-dialog';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeBotSettingsTransport, type FakeBotSettingsTransport } from '../../../lib/potato-botsettings';
import { AdvancedTab } from './AdvancedTab';
import { buttonByText } from './test-helpers';

vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn(async () => 'C:\\Users\\me\\Documents\\chat.md') }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(save).mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function flush() {
  await act(async () => {});
}

async function mount(transport: FakeBotSettingsTransport, extra: Partial<Parameters<typeof AdvancedTab>[0]> = {}) {
  await act(async () => root.render(<AdvancedTab bot={{ id: 'fish', name: 'Fish' }} transport={transport} defaultBrain="deepseek" {...extra} />));
  await flush();
}

async function click(el: Element | null | undefined) {
  if (!el) throw new Error('nothing to click');
  await act(async () => (el as HTMLElement).click());
  await flush();
}

async function typeIn(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await flush();
}

async function pick(select: HTMLSelectElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(select, value);
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await flush();
}

const titles = () => [...host.querySelectorAll('.ck-potato-adv-title')].map((h) => h.textContent);
const textarea = () => host.querySelector<HTMLTextAreaElement>('textarea')!;
const switchNamed = (label: string) => {
  const row = [...host.querySelectorAll('.ck-potato-adv-switch-row')].find((r) => r.querySelector('.ck-potato-adv-switch-label')?.textContent === label);
  return row?.querySelector<HTMLButtonElement>('button[role="switch"]') ?? null;
};
const sheet = () => host.querySelector<HTMLElement>('[role="alertdialog"]');

describe('the Advanced tab', () => {
  it('shows its sections in order: Name, Model, Instructions, Tools, then the bot', async () => {
    await mount(fakeBotSettingsTransport());
    expect(titles()).toEqual(['Name', 'Model', 'Instructions', 'Tools', 'Bot']);
  });

  it('chooses the bot brain, and "Use the default" goes back to the Settings brain', async () => {
    const transport = fakeBotSettingsTransport();
    await mount(transport);
    const select = host.querySelector<HTMLSelectElement>('select')!;
    expect(select.value).toBe('default');
    await pick(select, 'go');
    expect(transport.calls.find((c) => c.method === 'setBrain')!.args).toEqual(['fish', 'go']);
    await pick(host.querySelector<HTMLSelectElement>('select')!, 'default');
    expect(transport.calls.filter((c) => c.method === 'setBrain').at(-1)!.args).toEqual(['fish', null]);
  });

  it('names the default brain in the first choice', async () => {
    await mount(fakeBotSettingsTransport(), { defaultBrain: 'go' });
    expect(host.querySelector('option')!.textContent).toBe('Use the default (OpenCode Go)');
  });

  it('turns the tool switches on and off, and shows the computer switch only with the sandbox on', async () => {
    const transport = fakeBotSettingsTransport({ sandboxOn: false });
    await mount(transport);
    expect(switchNamed('Computer')).toBeNull();
    expect(switchNamed('Helpers')!.getAttribute('aria-checked')).toBe('true');
    await click(switchNamed('Helpers'));
    expect(transport.calls.find((c) => c.method === 'setTools')!.args).toEqual(['fish', { helpers: false, computer: true, web: true }]);
    expect(switchNamed('Helpers')!.getAttribute('aria-checked')).toBe('false');
  });

  it('shows the computer switch when the sandbox is on', async () => {
    await mount(fakeBotSettingsTransport({ sandboxOn: true }));
    expect(switchNamed('Computer')).not.toBeNull();
    expect(switchNamed('Web reading')).not.toBeNull();
  });

  it('saves an instruction as a version, then restores an older one from the history', async () => {
    let clock = 1_000;
    const transport = fakeBotSettingsTransport({ now: () => clock });
    await mount(transport);
    const saveButton = buttonByText(host, 'Save')!;
    expect(saveButton.disabled).toBe(true);
    await typeIn(textarea(), 'Be brief.');
    expect(buttonByText(host, 'Save')!.disabled).toBe(false);
    await click(buttonByText(host, 'Save'));
    expect(transport.calls.find((c) => c.method === 'saveInstructions')!.args).toEqual(['fish', 'Be brief.']);
    expect(host.querySelector('[role="status"]')!.textContent).toBe('Saved.');

    clock += 10;
    await typeIn(textarea(), 'Be long and kind.');
    await click(buttonByText(host, 'Save'));
    await click(buttonByText(host, 'History'));
    const rows = [...host.querySelectorAll('.ck-potato-adv-version')];
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelector('.ck-potato-adv-pill')!.textContent).toBe('In use');
    expect(rows[1].querySelector('.ck-potato-adv-version-text')!.textContent).toBe('Be brief.');
    await click(buttonByText(host, 'Restore'));
    expect(transport.calls.find((c) => c.method === 'restoreInstructions')!.args[0]).toBe('fish');
    expect(textarea().value).toBe('Be brief.');
    expect(host.querySelector('[role="status"]')!.textContent).toBe('Restored.');
    expect(host.querySelectorAll('.ck-potato-adv-version')).toHaveLength(3);
  });

  it('counts the characters near the limit and does not save over it', async () => {
    await mount(fakeBotSettingsTransport());
    await typeIn(textarea(), 'a'.repeat(4001));
    expect(host.querySelector('.ck-potato-adv-count')!.textContent).toBe('4001 / 4000');
    expect(buttonByText(host, 'Save')!.disabled).toBe(true);
  });

  it('shows a plain sentence when a save fails', async () => {
    await mount(fakeBotSettingsTransport({ fail: { saveInstructions: 'Could not save that.' } }));
    await typeIn(textarea(), 'Hello.');
    await click(buttonByText(host, 'Save'));
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('Could not save that.');
  });

  it('renames the bot and tells the page to read it again', async () => {
    const transport = fakeBotSettingsTransport();
    const changed = vi.fn();
    await mount(transport, { onChanged: changed });
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Bot name"]')!;
    expect(input.value).toBe('Fish');
    await typeIn(input, 'Nimbus');
    await click(buttonByText(host, 'Save name'));
    expect(transport.calls.find((c) => c.method === 'rename')!.args).toEqual(['fish', 'Nimbus']);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it('exports the chat through the save dialog and writes the file the engine kept', async () => {
    const transport = fakeBotSettingsTransport({ markdown: '# Chat with Fish' });
    await mount(transport);
    await click(buttonByText(host, 'Export chat'));
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ filters: [{ name: 'Markdown', extensions: ['md'] }] }));
    expect(transport.calls.find((c) => c.method === 'saveExport')!.args).toEqual(['tok-fake', 'C:\\Users\\me\\Documents\\chat.md']);
    expect(host.querySelector('[role="status"]')!.textContent).toContain('chat.md');
  });

  it('deletes only when the name is typed, then tells the page', async () => {
    const transport = fakeBotSettingsTransport({ name: 'Fish' });
    const changed = vi.fn();
    await mount(transport, { onChanged: changed });
    await click(buttonByText(host, 'Delete bot'));
    expect(sheet()).not.toBeNull();
    expect(sheet()!.querySelector('h2')!.textContent).toBe('Delete Fish?');
    const confirm = buttonByText(host, 'Delete')!;
    expect(confirm.disabled).toBe(true);
    await typeIn(sheet()!.querySelector('input')!, 'fis');
    expect(buttonByText(host, 'Delete')!.disabled).toBe(true);
    await typeIn(sheet()!.querySelector('input')!, 'Fish');
    await click(buttonByText(host, 'Delete'));
    expect(transport.calls.find((c) => c.method === 'remove')!.args).toEqual(['fish', 'Fish']);
    expect(changed).toHaveBeenCalledTimes(1);
    expect(sheet()).toBeNull();
  });

  it('keeps the sheet open and says why when the delete is refused', async () => {
    const transport = fakeBotSettingsTransport({ name: 'Fish', fail: { remove: 'Could not delete the bot.' } });
    await mount(transport);
    await click(buttonByText(host, 'Delete bot'));
    await typeIn(sheet()!.querySelector('input')!, 'Fish');
    await click(buttonByText(host, 'Delete'));
    expect(sheet()).not.toBeNull();
    expect(sheet()!.querySelector('[role="alert"]')!.textContent).toBe('Could not delete the bot.');
  });
});
