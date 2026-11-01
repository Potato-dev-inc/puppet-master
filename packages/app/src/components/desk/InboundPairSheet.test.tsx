// @vitest-environment jsdom
// The question on the other screen: name, code, No first, Escape is No, nothing without a question.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InboundPairSheet } from './InboundPairSheet';
import type { PairPending } from '../../lib/desk-pair';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Test-only fixture. */
const ASK: PairPending = { id: 7, role: 'host', name: 'Laptop', code: '4F7Q-9K2M', expires_at: 1 };

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

const find = (text: string) => [...document.querySelectorAll('button')].find((b) => b.textContent === text)!;

describe('InboundPairSheet', () => {
  it('draws nothing when nobody is asking', () => {
    act(() => root.render(<InboundPairSheet pending={null} onAnswer={vi.fn()} />));
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('says who wants to pair and shows the code', () => {
    act(() => root.render(<InboundPairSheet pending={ASK} onAnswer={vi.fn()} />));
    const dialog = document.querySelector('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain('Laptop wants to pair');
    expect(dialog.textContent).toContain('4F7Q-9K2M');
    expect(dialog.textContent).toContain('Only say yes if you started this');
  });

  it('the side that typed the code is asked too', () => {
    act(() => root.render(<InboundPairSheet pending={{ ...ASK, role: 'joiner', name: 'Ren-PC' }} onAnswer={vi.fn()} />));
    expect(document.querySelector('[role="alertdialog"]')!.textContent).toContain('Pair with Ren-PC?');
  });

  it('Allow and No send the answer for this question', () => {
    const onAnswer = vi.fn();
    act(() => root.render(<InboundPairSheet pending={ASK} onAnswer={onAnswer} />));
    act(() => find('Allow').click());
    expect(onAnswer).toHaveBeenLastCalledWith(7, true);
    act(() => find('No').click());
    expect(onAnswer).toHaveBeenLastCalledWith(7, false);
  });

  it('focus starts on No, and Escape means No', () => {
    const onAnswer = vi.fn();
    act(() => root.render(<InboundPairSheet pending={ASK} onAnswer={onAnswer} />));
    expect(document.activeElement).toBe(find('No'));
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(onAnswer).toHaveBeenCalledWith(7, false);
    expect(onAnswer).not.toHaveBeenCalledWith(7, true);
  });
});
