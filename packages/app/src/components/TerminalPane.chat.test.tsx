// @vitest-environment jsdom
// The Terminal | Chat toggle: the terminal host is hidden, never unmounted, while Chat is shown.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneData } from '../hooks/usePaneRegistry';

const life = vi.hoisted(() => ({ mounts: 0, unmounts: 0 }));

vi.mock('../hooks/useTerminalSession', async () => {
  const react = await import('react');
  return {
    useTerminalSession: () => {
      const containerRef = react.useRef<HTMLDivElement>(null);
      react.useEffect(() => {
        life.mounts += 1;
        return () => {
          life.unmounts += 1;
        };
      }, []);
      return { containerRef, nudgeReflow: () => undefined };
    },
  };
});
vi.mock('./chat/SessionChatPanel', () => ({
  SessionChatPanel: ({ paneId }: { paneId: string }) => <div data-testid="chat-panel">{paneId}</div>,
}));
vi.mock('../lib/agentDefinitions', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../lib/agentDefinitions')>()),
  tauriRecorderApi: {
    start: vi.fn(async () => ({})),
    status: vi.fn(async () => ({ recording: false })),
    stop: vi.fn(async () => ({ path: '' })),
  },
}));

import { TerminalPane } from './TerminalPane';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function pane(agentType: string): PaneData {
  return {
    status: 'idle',
    info: { id: 'p1', agent_type: agentType, status: 'idle', pid: 1, created_at: 1, cwd: 'proj', cols: 80, rows: 24 },
  } as unknown as PaneData;
}

let host: HTMLDivElement;
let root: Root;

function render(agentType: string) {
  return act(async () => {
    root.render(
      <TerminalPane
        pane={pane(agentType)}
        subscribePaneData={() => () => undefined}
        onClose={() => undefined}
        onPopOut={() => undefined}
        onSwitchAgent={async () => undefined}
      />,
    );
  });
}

const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === label) ?? null;
const terminalHost = () => host.querySelector<HTMLDivElement>('.terminal-host')!;
const click = (el: HTMLElement) => act(async () => el.click());

beforeEach(() => {
  life.mounts = 0;
  life.unmounts = 0;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('TerminalPane Terminal | Chat toggle', () => {
  it('is not shown for a plain shell pane', async () => {
    await render('powershell');
    expect(button('Chat')).toBeNull();
    expect(button('Terminal')).toBeNull();
  });

  it('is shown for an agent pane, with Terminal chosen first', async () => {
    await render('claude');
    expect(button('Terminal')?.getAttribute('aria-pressed')).toBe('true');
    expect(button('Chat')?.getAttribute('aria-pressed')).toBe('false');
    expect(terminalHost().className).not.toContain('invisible');
    expect(host.querySelector('[data-testid="chat-panel"]')).toBeNull();
  });

  it('keeps the terminal mounted while Chat is shown', async () => {
    await render('claude');
    const before = terminalHost();
    expect(life.mounts).toBe(1);

    await click(button('Chat')!);
    expect(button('Chat')?.getAttribute('aria-pressed')).toBe('true');
    expect(host.querySelector('[data-testid="chat-panel"]')?.textContent).toBe('p1');
    expect(terminalHost()).toBe(before);
    expect(terminalHost().className).toContain('invisible');
    expect(life.unmounts).toBe(0);

    await click(button('Terminal')!);
    expect(host.querySelector('[data-testid="chat-panel"]')).toBeNull();
    expect(terminalHost()).toBe(before);
    expect(terminalHost().className).not.toContain('invisible');
    expect(life.mounts).toBe(1);
    expect(life.unmounts).toBe(0);
  });
});
