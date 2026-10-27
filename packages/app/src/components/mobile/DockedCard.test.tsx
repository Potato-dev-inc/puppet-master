// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ITEM_HIGH, ITEM_LOW, ITEM_QUESTION, tombstone } from '../../dev/attention-fixtures';
import type { AttentionItem, AttentionSnapshot } from '../../lib/attention';
import type { BridgeClient } from '../../lib/bridge-types';
import { DockedCard } from './DockedCard';

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

const snap = (items: AttentionItem[]): AttentionSnapshot => ({ v: 1, rev: 1, counts: { blocking: items.filter((i) => i.state === 'open').length, ready: 0 }, items });
const bridge = { answerAttention: vi.fn() } as unknown as BridgeClient;

async function render(s: AttentionSnapshot, paneId: string, over: { onOpenPane?: (id: string) => void; onOpenItem?: (id: string) => void } = {}) {
  await act(async () => {
    root.render(
      <DockedCard snapshot={s} paneId={paneId} bridge={bridge} canAnswer onOpenPane={over.onOpenPane ?? (() => undefined)} onOpenItem={over.onOpenItem ?? (() => undefined)} autoFocus />,
    );
  });
}

describe('the docked card', () => {
  it('docks the open pane’s own card, one at a time, with how many more wait', async () => {
    await render(snap([ITEM_HIGH, ITEM_LOW, ITEM_QUESTION]), ITEM_LOW.pane!.id);
    expect(host.querySelectorAll('.ph-appr')).toHaveLength(1);
    expect(host.querySelector('.ph-appr-title')?.textContent).toBe('Run tests');
    expect(host.querySelector('.ph-queue-line')?.textContent).toBe('2 more waitingNext: Fix login redirect ›');
  });

  it('Next is a tap and opens the next pane; it never switches by itself', async () => {
    const onOpenPane = vi.fn();
    await render(snap([ITEM_HIGH, ITEM_LOW]), ITEM_LOW.pane!.id, { onOpenPane });
    expect(onOpenPane).not.toHaveBeenCalled();
    const next = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Next:'))!;
    await act(async () => next.click());
    expect(onOpenPane).toHaveBeenCalledWith(ITEM_HIGH.pane!.id);
  });

  it('a next item that has no pane opens as a card', async () => {
    const onOpenItem = vi.fn();
    const consent: AttentionItem = { ...ITEM_QUESTION, kind: 'consent', pane: null, key: 'consent:c1', id: 'C0nsentX', rank: 0 };
    await render(snap([consent, ITEM_LOW]), ITEM_LOW.pane!.id, { onOpenItem });
    await act(async () => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Next:'))!.click());
    expect(onOpenItem).toHaveBeenCalledWith('C0nsentX');
  });

  it('draws nothing when nothing waits on this pane or anywhere', async () => {
    await render(snap([]), 'pane-claude-1');
    expect(host.textContent).toBe('');
  });

  it('only tells how many wait when the open pane has no card of its own', async () => {
    await render(snap([ITEM_HIGH]), 'pane-codex-1');
    expect(host.querySelector('.ph-appr')).toBeNull();
    expect(host.textContent).toContain('1 more waiting');
  });

  it('keeps showing a card that just left, with what happened', async () => {
    await render(snap([tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' })]), ITEM_LOW.pane!.id);
    expect(host.textContent).toContain('Already answered on your PC: Allowed once.');
  });

  it('puts the focus on Deny for a high-risk card that a push tap opened', async () => {
    await render(snap([ITEM_HIGH]), ITEM_HIGH.pane!.id);
    expect((document.activeElement as HTMLElement).textContent).toBe('Deny');
  });
});

describe('a helper’s question in its parent’s chat', () => {
  const helperItem = { ...ITEM_LOW, id: 'helperQ1', pane: { ...ITEM_LOW.pane!, id: 'helper-pane' } };
  const renderWith = async (s: AttentionSnapshot, helperPaneIds: string[], onOpenPane = vi.fn()) => {
    await act(async () => {
      root.render(<DockedCard snapshot={s} paneId="parent-pane" bridge={bridge} canAnswer onOpenPane={onOpenPane} onOpenItem={() => undefined} helperPaneIds={helperPaneIds} />);
    });
    return onOpenPane;
  };

  it('docks the helper’s card under the parent, with Open helper, and counts it once', async () => {
    const onOpenPane = await renderWith(snap([helperItem, ITEM_HIGH]), ['helper-pane']);
    expect(host.querySelectorAll('[data-testid="helper-question"] .ph-appr')).toHaveLength(1);
    expect(host.querySelector('.ph-dock-helper-lead')?.textContent).toContain('helper asks');
    // Only the OTHER pane's item is "more waiting"; the helper's card is already on screen.
    expect(host.querySelector('.ph-queue-line')?.textContent).toContain('1 more waiting');
    await act(async () => host.querySelector<HTMLButtonElement>('[data-role="open-helper"]')!.click());
    expect(onOpenPane).toHaveBeenCalledWith('helper-pane');
  });

  it('shows nothing for a helper that is not asking, or when no helper is passed', async () => {
    await renderWith(snap([helperItem]), []);
    expect(host.querySelector('[data-testid="helper-question"]')).toBeNull();
    await renderWith(snap([tombstone(helperItem, { state: 'answered' })]), ['helper-pane']);
    expect(host.querySelector('[data-testid="helper-question"]')).toBeNull();
  });
});
