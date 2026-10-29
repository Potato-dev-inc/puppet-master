// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { Palette } from './Palette';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const c = {
  registry: { paneList: [], panes: new Map() },
  launchEntries: [],
  readyAgents: new Set(),
  names: new Map(),
  selectedId: null,
  mode: 'orchestration',
  theme: 'dark',
  interrupt: vi.fn(),
} as unknown as CockpitApi;

const row = (title: string) =>
  Array.from(host.querySelectorAll('[role="option"]')).find((r) => r.textContent?.includes(title)) as HTMLElement | undefined;

describe('Palette: Review Desk', () => {
  it('"Review Desk" opens the desk and not the first item', () => {
    const onOpenReview = vi.fn();
    const onReviewRiskiest = vi.fn();
    act(() => root.render(<Palette c={c} onClose={vi.fn()} onOpenReview={onOpenReview} onReviewRiskiest={onReviewRiskiest} />));
    expect(row('Review Desk')).toBeTruthy();
    act(() => row('Review Desk')!.click());
    expect(onOpenReview).toHaveBeenCalledTimes(1);
    expect(onReviewRiskiest).not.toHaveBeenCalled();
  });

  it('"Review the riskiest item" calls only its own handler', () => {
    const onOpenReview = vi.fn();
    const onReviewRiskiest = vi.fn();
    act(() => root.render(<Palette c={c} onClose={vi.fn()} onOpenReview={onOpenReview} onReviewRiskiest={onReviewRiskiest} />));
    expect(row('Review the riskiest item')).toBeTruthy();
    act(() => row('Review the riskiest item')!.click());
    expect(onReviewRiskiest).toHaveBeenCalledTimes(1);
    expect(onOpenReview).not.toHaveBeenCalled();
  });

  it('has no rows when the app cannot show the desk', () => {
    act(() => root.render(<Palette c={c} onClose={vi.fn()} />));
    expect(row('Review Desk')).toBeUndefined();
    expect(row('Review the riskiest item')).toBeUndefined();
  });
});
