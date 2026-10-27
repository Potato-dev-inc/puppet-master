// @vitest-environment jsdom
// "What {name} knows": the counter against each cap, Save only when it fits, the notes-full line, outside changes.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type MemoryView } from '../../../lib/companion';
import { MemoryCard } from './MemoryCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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
  vi.restoreAllMocks();
});

const memory = (over: Partial<MemoryView> = {}): MemoryView => ({
  soul: 'Warm and short.',
  user: 'Likes tea.',
  memory: 'Works on the lamp project.',
  caps: { soul: 1500, user: 1400, memory: 2200 },
  pending: [],
  outsideChanges: [],
  ...over,
});

const counter = () => host.querySelector('.ck-companion-meta > span')!;
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;

function type(text: string) {
  const area = host.querySelector<HTMLTextAreaElement>('textarea')!;
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(area, text);
    area.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function render(m: MemoryView, transport = fakeCompanionTransport(), onChanged = vi.fn()) {
  await act(async () =>
    root.render(<MemoryCard companionId="mochi" name="Mochi" memory={m} transport={transport} onChanged={onChanged} />),
  );
  return { transport, onChanged };
}

describe('MemoryCard', () => {
  it('titles the card with the name and counts characters against the cap', async () => {
    await render(memory({ soul: 'x'.repeat(1204) }));
    expect(host.querySelector('h2')?.textContent).toBe('What Mochi knows');
    expect(counter().textContent).toBe('1,204 / 1,500');
  });

  it('Save is off while the text is the same, and over the cap', async () => {
    const { transport } = await render(memory());
    act(() => button('Edit').click());
    expect(button('Save').disabled).toBe(true);
    type('x'.repeat(1501));
    expect(counter().textContent).toBe('1,501 / 1,500');
    expect(counter().classList.contains('ck-memory-err')).toBe(true);
    expect(button('Save').disabled).toBe(true);
    act(() => button('Save').click());
    expect(transport.calls.some((c) => c.method === 'saveMemory')).toBe(false);
  });

  it('saves the file and hands back the view the command returned', async () => {
    const onChanged = vi.fn();
    const { transport } = await render(memory(), fakeCompanionTransport(), onChanged);
    act(() => button('Edit').click());
    type('Warm, brief, and kind.');
    await act(async () => button('Save').click());
    expect(transport.calls.find((c) => c.method === 'saveMemory')?.args).toEqual(['mochi', 'soul', 'Warm, brief, and kind.']);
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(onChanged.mock.calls[0][0].soul).toBe('Warm, brief, and kind.');
  });

  it('says its notes are full and offers to ask for a tidy-up', async () => {
    const { transport } = await render(memory({ memory: 'n'.repeat(2200) }));
    expect(host.textContent).toContain('Its notes are full. Ask Mochi to tidy up.');
    await act(async () => button('Ask to tidy up').click());
    expect(transport.calls.some((c) => c.method === 'tidyUp' && c.args[0] === 'mochi')).toBe(true);
  });

  it('shows a file that changed outside the app, and keeps or undoes it', async () => {
    const { transport } = await render(memory({ outsideChanges: [{ file: 'memory' }] }));
    expect(host.textContent).toContain('MEMORY.md changed outside Puppet Master.');
    await act(async () => button('Keep the change').click());
    expect(transport.calls.find((c) => c.method === 'decideOutsideChange')?.args).toEqual(['mochi', 'memory', 'keep']);
  });
});
