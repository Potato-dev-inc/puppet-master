// @vitest-environment jsdom
// The split view's two sides: the focused one is full strength, the other is dimmed, a click on a side focuses it,
// and only the right side carries the close button. The chat itself is stubbed (it reads the session file).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import { SplitView, focusSideComposer } from './SplitView';

vi.mock('../chat/SessionChatPanel', () => ({
  SessionChatPanel: ({ paneId }: { paneId: string }) => (
    <div data-testid="chat">
      {paneId}
      <textarea aria-label={`message ${paneId}`} />
    </div>
  ),
}));
vi.mock('./rail/SessionRail', () => ({ SessionRail: () => null }));
vi.mock('../../lib/agentRegistry', () => ({ useAgentRegistry: () => ({ agents: [] }) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function fakeApi(): CockpitApi {
  const mk = (id: string) => ({ info: { id, agent_type: 'codex', cwd: 'C:\\work\\shop', title: null } as unknown as PaneInfo, status: 'idle' });
  const panes = [mk('p1'), mk('p2')];
  return {
    registry: { paneList: panes, panes: new Map(panes.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([
      ['p1', 'Codex'],
      ['p2', 'Codex two'],
    ]),
    asked: { questions: [] },
    prompts: {},
    selectedId: 'p1',
    select: vi.fn(),
    notify: vi.fn(),
  } as unknown as CockpitApi;
}

function sides(): HTMLElement[] {
  return [...host.querySelectorAll<HTMLElement>('.ck-split-side')];
}

async function renderSplit(focus: 'left' | 'right', onFocus = vi.fn(), onClose = vi.fn(), ratio = 0.5, onRatio = vi.fn()) {
  await act(async () =>
    root.render(
      <SplitView c={fakeApi()} leftId="p1" rightId="p2" focus={focus} ratio={ratio} onFocus={onFocus} onRatio={onRatio} onClose={onClose} />,
    ),
  );
  return { onFocus, onClose, onRatio };
}

describe('split view sides', () => {
  it('shows the left and right pane, each with its own chat', async () => {
    await renderSplit('left');
    const chats = [...host.querySelectorAll('[data-testid="chat"]')].map((el) => el.textContent);
    expect(chats).toEqual(['p1', 'p2']);
  });

  it('the focused side is full strength and the other is dimmed', async () => {
    await renderSplit('right');
    const [left, right] = sides();
    expect(left.classList.contains('is-dim')).toBe(true);
    expect(right.classList.contains('is-focused')).toBe(true);
    expect(right.getAttribute('aria-current')).toBe('true');
    expect(left.getAttribute('aria-current')).toBeNull();
  });

  it('a click on a side asks to focus it', async () => {
    const { onFocus } = await renderSplit('left');
    await act(async () => sides()[1].dispatchEvent(new MouseEvent('mousedown', { bubbles: true })));
    expect(onFocus).toHaveBeenCalledWith('right');
  });

  it('only the right side has the close button, and it closes the split', async () => {
    const { onClose } = await renderSplit('left');
    const [left, right] = sides();
    expect(left.querySelector('button[aria-label="Close split view"]')).toBeNull();
    const close = right.querySelector<HTMLButtonElement>('button[aria-label="Close split view"]');
    expect(close).toBeTruthy();
    await act(async () => close!.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('split view divider', () => {
  // The split is 1000px wide, so the two sides share 991px (the divider is 9px).
  const SPAN = 991;
  const MIN = 320 / SPAN;
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      left: 0, top: 0, right: 1000, bottom: 400, width: 1000, height: 400, x: 0, y: 0, toJSON: () => ({}),
    } as DOMRect);
  });
  afterEach(() => vi.restoreAllMocks());

  const divider = () => host.querySelector<HTMLElement>('[role="separator"]')!;
  const pointer = (type: string, clientX = 0) =>
    new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX });

  it('is a focusable separator that shows the left side share', async () => {
    await renderSplit('left', vi.fn(), vi.fn(), 0.4);
    expect(divider().tabIndex).toBe(0);
    expect(divider().getAttribute('aria-orientation')).toBe('vertical');
    expect(divider().getAttribute('aria-valuenow')).toBe('40');
  });

  it('Left and Right arrows move the divider 24px', async () => {
    const { onRatio } = await renderSplit('left');
    await act(async () => divider().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })));
    expect(onRatio).toHaveBeenLastCalledWith(expect.closeTo(0.5 + 24 / SPAN, 9));
    await act(async () => divider().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true })));
    expect(onRatio).toHaveBeenLastCalledWith(expect.closeTo(0.5 - 24 / SPAN, 9));
  });

  it('a double-click goes back to the even split', async () => {
    const { onRatio } = await renderSplit('left', vi.fn(), vi.fn(), 0.7);
    await act(async () => divider().dispatchEvent(new MouseEvent('dblclick', { bubbles: true })));
    expect(onRatio).toHaveBeenLastCalledWith(0.5);
  });

  it('a drag follows the pointer and stops at the 320px minimum on each side', async () => {
    const { onRatio } = await renderSplit('left');
    await act(async () => divider().dispatchEvent(pointer('pointerdown', 500)));
    await act(async () => divider().dispatchEvent(pointer('pointermove', 300)));
    expect(onRatio).toHaveBeenLastCalledWith(expect.closeTo(MIN, 9));
    await act(async () => divider().dispatchEvent(pointer('pointermove', 900)));
    expect(onRatio).toHaveBeenLastCalledWith(expect.closeTo(1 - MIN, 9));
  });

  it('moving the pointer without holding the button does not resize', async () => {
    const { onRatio } = await renderSplit('left');
    await act(async () => divider().dispatchEvent(pointer('pointermove', 300)));
    await act(async () => divider().dispatchEvent(pointer('pointerdown', 500)));
    await act(async () => divider().dispatchEvent(pointer('pointerup', 500)));
    onRatio.mockClear();
    await act(async () => divider().dispatchEvent(pointer('pointermove', 300)));
    expect(onRatio).not.toHaveBeenCalled();
  });
});

describe('split view focus', () => {
  it('focusSideComposer moves the keyboard focus into that side message box', async () => {
    await renderSplit('left');
    act(() => focusSideComposer('right'));
    expect(document.activeElement).toBe(host.querySelector('[aria-label="message p2"]'));
    act(() => focusSideComposer('left'));
    expect(document.activeElement).toBe(host.querySelector('[aria-label="message p1"]'));
  });
});
