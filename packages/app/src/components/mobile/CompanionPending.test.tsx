// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PendingItem } from '../../lib/companion';
import { CompanionPending } from './CompanionPending';

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

const items: PendingItem[] = [
  { id: 'p1', kind: 'note', text: 'Likes short replies', flagged: null, targetLine: null },
  { id: 'p2', kind: 'note', text: 'Check https://example.test', flagged: 'Has a link', targetLine: null },
];

function mount(props: Partial<{ canAnswer: boolean; busyId: string | null; list: PendingItem[] }> = {}) {
  const onDecide = vi.fn();
  act(() => {
    root.render(
      <CompanionPending
        name="Mochi"
        items={props.list ?? items}
        canAnswer={props.canAnswer ?? true}
        busyId={props.busyId ?? null}
        onDecide={onDecide}
      />,
    );
  });
  return { onDecide };
}

const buttons = () => Array.from(host.querySelectorAll<HTMLButtonElement>('button'));
const buttonText = (text: string) => buttons().find((b) => b.textContent === text);

describe('CompanionPending', () => {
  it('shows the title with the companion name and each note with its flag', () => {
    mount();
    expect(host.querySelector('h2')?.textContent).toBe('Mochi wants to remember');
    expect(host.textContent).toContain('Likes short replies');
    expect(host.textContent).toContain('Has a link');
  });

  it('draws nothing when there are no notes', () => {
    mount({ list: [] });
    expect(host.textContent).toBe('');
  });

  it('Keep and Don\'t keep send keep and drop for that note', () => {
    const { onDecide } = mount();
    const keep = host.querySelectorAll('li')[0].querySelectorAll('button')[0];
    const drop = host.querySelectorAll('li')[0].querySelectorAll('button')[1];
    expect(keep.textContent).toBe('Keep');
    expect(drop.textContent).toBe("Don't keep");
    act(() => keep.click());
    act(() => drop.click());
    expect(onDecide).toHaveBeenNthCalledWith(1, 'p1', 'keep');
    expect(onDecide).toHaveBeenNthCalledWith(2, 'p1', 'drop');
  });

  it('hides the buttons without the answer cap, and only reads the notes', () => {
    mount({ canAnswer: false });
    expect(buttons()).toHaveLength(0);
    expect(host.textContent).toContain('Likes short replies');
  });

  it('turns the buttons off while another note is being sent', () => {
    mount({ busyId: 'p2' });
    expect(buttons().every((b) => b.disabled)).toBe(true);
    expect(buttonText('Keep')).toBeDefined();
  });
});
