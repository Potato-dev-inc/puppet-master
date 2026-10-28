// @vitest-environment jsdom
// The split view's two sides: the focused one is full strength, the other is dimmed, a click on a side focuses it,
// and only the right side carries the close button. The chat itself is stubbed (it reads the session file).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import { SplitView } from './SplitView';

vi.mock('../chat/SessionChatPanel', () => ({
  SessionChatPanel: ({ paneId }: { paneId: string }) => <div data-testid="chat">{paneId}</div>,
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

async function renderSplit(focus: 'left' | 'right', onFocus = vi.fn(), onClose = vi.fn()) {
  await act(async () =>
    root.render(<SplitView c={fakeApi()} leftId="p1" rightId="p2" focus={focus} onFocus={onFocus} onClose={onClose} />),
  );
  return { onFocus, onClose };
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
