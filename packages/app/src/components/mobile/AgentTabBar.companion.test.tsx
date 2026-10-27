// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CompanionView } from '../../lib/companion';
import { AgentTabBar, TAB_FACE_PX } from './AgentTabBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has no scrollIntoView, and the tab bar calls CSS.escape in an effect.
Element.prototype.scrollIntoView = () => {};
if (!(globalThis as { CSS?: unknown }).CSS) {
  (globalThis as { CSS?: unknown }).CSS = { escape: (s: string) => s };
}

const pane = (id: string, status: string, created_at: number) =>
  ({ id, status, created_at, agent_type: 'claude', cwd: 'C:\\work\\proj', pid: 1, cols: 44, rows: 30 }) as unknown as PaneInfo;

const companion = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'proj',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

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

function mount(panes: PaneInfo[], companions: CompanionView[], active = 'all') {
  act(() => {
    root.render(
      <AgentTabBar
        panes={panes}
        labels={new Map(panes.map((p) => [p.id, `Agent ${p.id}`]))}
        active={active}
        bridgeReady
        onSelect={vi.fn()}
        onNew={vi.fn()}
        onMenu={vi.fn()}
        companions={companions}
      />,
    );
  });
}

const tabIds = () => Array.from(host.querySelectorAll<HTMLElement>('[role="tab"]')).map((t) => t.dataset.tab);

describe('AgentTabBar with companions', () => {
  it('shows the companion tabs first, then All, then the agent tabs', () => {
    mount([pane('a', 'idle', 1)], [companion({ id: 'mochi' }), companion({ id: 'bit', name: 'Bit', avatar: 'bit' })]);
    expect(tabIds()).toEqual(['companion:mochi', 'companion:bit', 'all', 'a']);
  });

  it('a companion tab shows its face at 22 px and its name', () => {
    mount([], [companion()]);
    const face = host.querySelector<SVGSVGElement>('[data-tab="companion:mochi"] svg');
    expect(face?.getAttribute('width')).toBe(String(TAB_FACE_PX));
    expect(face?.getAttribute('aria-label')).toBe('Mochi, Here');
    expect(host.querySelector('[data-tab="companion:mochi"] .ph-tab-label')?.textContent).toBe('Mochi');
  });

  it('the companion pane has no tab of its own', () => {
    mount([pane('companion-mochi', 'idle', 1), pane('a', 'idle', 2)], [companion()]);
    expect(tabIds()).toEqual(['companion:mochi', 'all', 'a']);
  });

  it('the selected companion tab is marked', () => {
    mount([], [companion()], 'companion:mochi');
    expect(host.querySelector('[data-tab="companion:mochi"]')?.getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-tab="all"]')?.getAttribute('aria-selected')).toBe('false');
  });

  it('the All count leaves out the companion pane', () => {
    mount([pane('companion-mochi', 'waiting_input', 1), pane('a', 'waiting_input', 2)], [companion({ state: 'needs_you' })]);
    expect(host.querySelector('[data-tab="all"] .ph-count')?.textContent).toBe('1');
  });
});
