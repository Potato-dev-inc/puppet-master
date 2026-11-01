// @vitest-environment jsdom
// Open work of a bot: the list, complete and reopen, remove, the add field, the helper rows, "Show all", and the
// sentences for a failed change or a refresh that failed after a save.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeOpenWorkTransport, type OpenWorkItem, type OpenWorkTransport } from '../../../lib/potato-open-work';
import { OPEN_WORK_COPY, OpenWork, SHOWN_AT_FIRST } from './OpenWork';

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

const item = (id: string, text: string, done = false): OpenWorkItem => ({ id, text, done });

async function show(transport: OpenWorkTransport) {
  await act(async () => root.render(<OpenWork botId="nimbus" transport={transport} />));
  await act(async () => {});
}

const rows = () => [...host.querySelectorAll<HTMLLIElement>('.ck-potato-todo-row')];
const rowText = () => rows().map((r) => r.querySelector('.ck-potato-todo-text')?.textContent);
const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
const alertText = () => host.querySelector('[role="alert"]')?.textContent;

/** Types the way React hears it (a native value setter, then an input event). */
function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  setter.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('Open work', () => {
  it('shows the items in order', async () => {
    await show(fakeOpenWorkTransport({ items: [item('ow-00000001', 'Check the build'), item('ow-00000002', 'Email the list', true)] }));
    expect(rowText()).toEqual(['Check the build', 'Email the list']);
    expect(rows()[1].classList.contains('is-done')).toBe(true);
  });

  it('says so when there is nothing open', async () => {
    await show(fakeOpenWorkTransport());
    expect(host.textContent).toContain(OPEN_WORK_COPY.empty);
  });

  it('completes and reopens an item with its check, and says which item it is', async () => {
    const transport = fakeOpenWorkTransport({ items: [item('ow-00000001', 'Check the build')] });
    await show(transport);
    const check = host.querySelector<HTMLInputElement>('.ck-potato-todo-check')!;
    expect(check.getAttribute('aria-label')).toBe('Complete: Check the build');
    await act(async () => check.click());
    expect(transport.calls.at(-2)).toEqual({ method: 'setDone', args: ['nimbus', 'ow-00000001', true] });
    expect(rows()[0].classList.contains('is-done')).toBe(true);
    expect(host.querySelector<HTMLInputElement>('.ck-potato-todo-check')!.getAttribute('aria-label')).toBe('Reopen: Check the build');
  });

  it('removes one item and keeps the others', async () => {
    const transport = fakeOpenWorkTransport({ items: [item('ow-00000001', 'Check the build'), item('ow-00000002', 'Email the list')] });
    await show(transport);
    await act(async () => button('Remove: Check the build')!.click());
    expect(transport.calls.at(-2)).toEqual({ method: 'remove', args: ['nimbus', 'ow-00000001'] });
    expect(rowText()).toEqual(['Email the list']);
  });

  it('adds an item from the field behind the plus button', async () => {
    const transport = fakeOpenWorkTransport();
    await show(transport);
    expect(host.querySelector('form')).toBeNull();
    await act(async () => button('Add an item')!.click());
    const input = host.querySelector<HTMLInputElement>('.ck-potato-add-input')!;
    expect(input).toBeTruthy();
    await act(async () => type(input, '  Send the weekly summary  '));
    await act(async () => {
      host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    });
    await act(async () => {});
    expect(transport.calls.find((c) => c.method === 'add')?.args).toEqual(['nimbus', 'Send the weekly summary']);
    expect(rowText()).toEqual(['Send the weekly summary']);
    expect(host.querySelector('form')).toBeNull();
  });

  it('keeps an empty item out', async () => {
    const transport = fakeOpenWorkTransport();
    await show(transport);
    await act(async () => button('Add an item')!.click());
    const add = host.querySelector<HTMLButtonElement>('.ck-potato-add button[type="submit"]')!;
    expect(add.disabled).toBe(true);
    expect(transport.calls.some((c) => c.method === 'add')).toBe(false);
  });

  it('shows the helper jobs still running, read-only', async () => {
    await show(fakeOpenWorkTransport({ helpers: [{ requestId: 'r1', project: 'demo', startedMs: 1 }] }));
    expect(host.textContent).toContain('Working');
    expect(host.textContent).toContain('A helper is working in demo.');
    expect(rows()).toHaveLength(0);
  });

  it('shows the first items and opens the rest with Show all', async () => {
    const many = Array.from({ length: SHOWN_AT_FIRST + 1 }, (_, i) => item(`ow-0000000${i}`, `Item ${i}`));
    await show(fakeOpenWorkTransport({ items: many }));
    expect(rows()).toHaveLength(SHOWN_AT_FIRST);
    await act(async () => host.querySelector<HTMLButtonElement>('.ck-potato-show-more')!.click());
    expect(rows()).toHaveLength(SHOWN_AT_FIRST + 1);
  });

  it('shows the sentence of a failed change and keeps the list', async () => {
    const base = fakeOpenWorkTransport({ items: [item('ow-00000001', 'Check the build')] });
    await show({ ...base, setDone: () => Promise.reject(new Error(OPEN_WORK_COPY.changeFailed)) });
    await act(async () => host.querySelector<HTMLInputElement>('.ck-potato-todo-check')!.click());
    expect(alertText()).toBe(OPEN_WORK_COPY.changeFailed);
    expect(rowText()).toEqual(['Check the build']);
  });

  it('says so when the item was saved but the list could not be read again', async () => {
    const base = fakeOpenWorkTransport();
    let reads = 0;
    const flaky: OpenWorkTransport = {
      ...base,
      list: (botId) => {
        reads += 1;
        return reads === 1 ? base.list(botId) : Promise.reject(new Error('offline'));
      },
    };
    await show(flaky);
    await act(async () => button('Add an item')!.click());
    await act(async () => type(host.querySelector<HTMLInputElement>('.ck-potato-add-input')!, 'Call Sam'));
    await act(async () => {
      host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    });
    await act(async () => {});
    expect(alertText()).toBe(OPEN_WORK_COPY.savedNotRefreshed);
  });

  it('shows a sentence when the list cannot be read', async () => {
    await show(fakeOpenWorkTransport({ fail: 'Could not reach the list.' }));
    expect(alertText()).toBe('Could not reach the list.');
  });
});
