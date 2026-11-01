// @vitest-environment jsdom
// The hover bar's grey cost: "about 2¢" for a reply, "under 1¢" below a cent, nothing when there is no cost.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HoverActions, replyCostText } from './HoverActions';

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

function render(costNano?: number) {
  act(() => root.render(<HoverActions text="hi" onReply={vi.fn()} costNano={costNano} />));
  return host.querySelector('.ck-potato-cost');
}

describe('replyCostText', () => {
  it('rounds to whole cents and always says about', () => {
    expect(replyCostText(20_000_000)).toBe('about 2¢');
    expect(replyCostText(14_000_000)).toBe('about 1¢');
    expect(replyCostText(15_000_000)).toBe('about 2¢');
  });

  it('says under 1¢ below a cent, including zero', () => {
    expect(replyCostText(9_999_999)).toBe('under 1¢');
    expect(replyCostText(500)).toBe('under 1¢');
    expect(replyCostText(0)).toBe('under 1¢');
  });
});

describe('HoverActions cost', () => {
  it('shows the grey estimate next to Copy and Reply', () => {
    const cost = render(20_000_000);
    expect(cost?.textContent).toBe('about 2¢');
    expect(cost?.className).toContain('ck-sub');
  });

  it('shows under 1¢ for a tiny reply', () => {
    expect(render(120_000)?.textContent).toBe('under 1¢');
  });

  it('shows nothing when the reply has no cost', () => {
    expect(render(undefined)).toBeNull();
  });
});
