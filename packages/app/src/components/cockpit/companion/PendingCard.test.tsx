// @vitest-environment jsdom
// "{name} wants to remember": Keep and Don't keep go through the transport; flags show but never block.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type PendingItem } from '../../../lib/companion';
import { PendingCard } from './PendingCard';

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
  vi.restoreAllMocks();
});

const item = (over: Partial<PendingItem> = {}): PendingItem => ({
  id: 'a1b2c3d4e5f6',
  kind: 'note',
  text: 'Likes short replies.',
  flagged: null,
  targetLine: null,
  ...over,
});

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;

describe('PendingCard', () => {
  it('draws nothing when no note waits', async () => {
    await act(async () => root.render(<PendingCard companionId="mochi" name="Mochi" items={[]} transport={fakeCompanionTransport()} onChanged={() => {}} />));
    expect(host.innerHTML).toBe('');
  });

  it('titles the list with the name and shows the flag without hiding the buttons', async () => {
    const pending = [item({ flagged: 'Has a link' })];
    await act(async () => root.render(<PendingCard companionId="mochi" name="Mochi" items={pending} transport={fakeCompanionTransport()} onChanged={() => {}} />));
    expect(host.querySelector('h2')?.textContent).toBe('Mochi wants to remember');
    expect(host.querySelector('.ck-memory-tag')?.textContent).toBe('Has a link');
    expect(button('Keep')).toBeTruthy();
    expect(button("Don't keep")).toBeTruthy();
  });

  it('Keep sends "keep" for that item, and hands back the new view', async () => {
    const transport = fakeCompanionTransport();
    const onChanged = vi.fn();
    await act(async () => root.render(<PendingCard companionId="mochi" name="Mochi" items={[item()]} transport={transport} onChanged={onChanged} />));
    await act(async () => button('Keep').click());
    expect(transport.calls.find((c) => c.method === 'decidePending')?.args).toEqual(['mochi', 'a1b2c3d4e5f6', 'keep']);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it("Don't keep sends \"drop\" for that item", async () => {
    const transport = fakeCompanionTransport();
    await act(async () => root.render(<PendingCard companionId="mochi" name="Mochi" items={[item()]} transport={transport} onChanged={() => {}} />));
    await act(async () => button("Don't keep").click());
    expect(transport.calls.find((c) => c.method === 'decidePending')?.args).toEqual(['mochi', 'a1b2c3d4e5f6', 'drop']);
  });

  it('shows a plain sentence when the command fails', async () => {
    const transport = fakeCompanionTransport();
    transport.decidePending = vi.fn(async () => {
      throw new Error('This note is out of date.');
    });
    await act(async () => root.render(<PendingCard companionId="mochi" name="Mochi" items={[item()]} transport={transport} onChanged={() => {}} />));
    await act(async () => button('Keep').click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('This note is out of date.');
  });
});
