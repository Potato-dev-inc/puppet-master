// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SessionRail } from './SessionRail';

const chat = vi.hoisted(() => ({ current: {} as Record<string, unknown>, calls: [] as Array<[string, boolean]> }));
vi.mock('../../../hooks/useSessionChat', () => ({
  useSessionChat: (paneId: string, enabled: boolean) => {
    chat.calls.push([paneId, enabled]);
    return chat.current;
  },
}));
vi.mock('./usePaneBranch', () => ({ usePaneBranch: () => 'feat/rail' }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  chat.calls = [];
  chat.current = {
    model: 'model-x',
    cliVersion: '1.2.3',
    usage: { kind: 'usage', id: 'u', at: null, inputTokens: 90_000, outputTokens: 6000, contextWindow: 200_000 },
    plan: [
      { text: 'Step one', status: 'completed' },
      { text: 'Step two', status: 'pending' },
    ],
    outputs: [{ path: '/work/shop/notes.txt', tool: 'Write', wrote: true }],
  };
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const props = {
  paneId: 'p1',
  status: 'idle',
  cwd: '/work/shop',
  agentName: 'Claude Code',
  accountLabel: null,
  readsChat: true,
  helperOf: null,
  helpers: [],
  onOpenPane: () => {},
};

describe('SessionRail', () => {
  it('shows every card from the pane chat data', () => {
    act(() => root.render(<SessionRail {...props} helpers={[{ id: 'h', title: 'Check tests', agent: 'Codex', status: 'running' }]} />));
    const text = host.textContent ?? '';
    expect(text).toContain('model-x');
    expect(text).toContain('v1.2.3');
    expect(text).toContain('feat/rail');
    expect(text).toContain('96k of 200k tokens (48%)');
    expect(text).toContain('1 of 2 done');
    expect(text).toContain('notes.txt');
    expect(text).toContain('Check tests');
    expect(chat.calls[0]).toEqual(['p1', true]);
  });
  it('hides the cards that have nothing and says context is not reported', () => {
    chat.current = { model: null, cliVersion: null, usage: null, plan: [], outputs: [] };
    act(() => root.render(<SessionRail {...props} />));
    const text = host.textContent ?? '';
    expect(text).toContain('Not reported by this agent');
    expect(text).not.toContain('Tasks');
    expect(text).not.toContain('Changes');
    expect(text).not.toContain('Helpers');
  });
  it('does not read a chat for a plain shell', () => {
    act(() => root.render(<SessionRail {...props} readsChat={false} />));
    expect(chat.calls[0]).toEqual(['p1', false]);
  });
  it('names the parent of a helper and opens it', () => {
    const open = vi.fn();
    act(() => root.render(<SessionRail {...props} helperOf={{ parentId: 'parent', text: 'Helper of Refactor' }} onOpenPane={open} />));
    const link = host.querySelector<HTMLButtonElement>('.ck-rail-parent')!;
    expect(link.textContent).toBe('Helper of Refactor');
    act(() => link.click());
    expect(open).toHaveBeenCalledWith('parent');
  });
  it('opens the Changes tab from a file only when the pane has one', () => {
    act(() => root.render(<SessionRail {...props} />));
    expect(host.querySelector('[data-testid="rail-changes"] button')).toBeNull();
    const open = vi.fn();
    act(() => root.render(<SessionRail {...props} onOpenChanges={open} />));
    act(() => host.querySelector<HTMLButtonElement>('[data-testid="rail-changes"] button')!.click());
    expect(open).toHaveBeenCalled();
  });
});
