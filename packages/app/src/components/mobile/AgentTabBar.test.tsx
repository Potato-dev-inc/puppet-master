// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import { AgentTabBar } from './AgentTabBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has no scrollIntoView, and the tab bar calls CSS.escape in an effect.
Element.prototype.scrollIntoView = () => {};
if (!(globalThis as { CSS?: unknown }).CSS) {
  (globalThis as { CSS?: unknown }).CSS = { escape: (s: string) => s };
}

const pane = (id: string, status: string, created_at: number, extra = {}) =>
  ({
    id,
    status,
    created_at,
    agent_type: 'claude',
    cwd: 'C:\\work\\proj',
    pid: 1,
    last_output_at: Date.now() - 120000,
    cols: 44,
    rows: 30,
    ...extra,
  }) as unknown as PaneInfo;

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

const panes = [pane('a', 'waiting_input', 1), pane('b', 'running', 2)];
const labels = new Map([
  ['a', 'Fix login'],
  ['b', 'codex'],
]);

function mount(
  props: Partial<{
    active: string;
    bridgeReady: boolean;
    onSelect: (tab: string) => void;
    onNew: () => void;
    onMenu: () => void;
    needsYou: number;
  }> = {},
) {
  const onSelect = props.onSelect ?? vi.fn();
  const onNew = props.onNew ?? vi.fn();
  const onMenu = props.onMenu ?? vi.fn();
  act(() => {
    root.render(
      <AgentTabBar
        panes={panes}
        labels={labels}
        active={props.active ?? 'all'}
        bridgeReady={props.bridgeReady ?? true}
        needsYou={props.needsYou}
        onSelect={onSelect}
        onNew={onNew}
        onMenu={onMenu}
      />,
    );
  });
  return { onSelect, onNew, onMenu };
}

const tabs = () => Array.from(host.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
const tabByLabel = (text: string) => tabs().find((t) => t.querySelector('.ph-tab-label')?.textContent === text);
const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;

describe('phone AgentTabBar', () => {
  it('renders All plus one tab per pane, titled with the pane label', () => {
    mount();
    expect(tabs()).toHaveLength(3);
    expect(tabs()[0].textContent).toContain('All');
    expect(tabs()[1].textContent).toBe('Fix login');
    expect(tabs()[2].textContent).toBe('codex');
  });

  it('counts only the panes that need the user on the All tab', () => {
    mount();
    const badge = tabs()[0].querySelector('.ph-count');
    expect(badge?.textContent).toBe('1');
  });

  it('shows the state glyph on each agent tab', () => {
    mount();
    const glyphA = tabByLabel('Fix login')!.querySelector('[role="img"]');
    const glyphB = tabByLabel('codex')!.querySelector('[role="img"]');
    expect(glyphA?.getAttribute('aria-label')).toBe('Needs you');
    expect(glyphB?.getAttribute('aria-label')).toBe('Working');
  });

  it('selects All as the active tab and calls onSelect with the pane id on tap', () => {
    const { onSelect } = mount({ active: 'all' });
    expect(tabs()[0].getAttribute('aria-selected')).toBe('true');
    expect(tabByLabel('Fix login')!.getAttribute('aria-selected')).toBe('false');
    act(() => tabByLabel('Fix login')!.click());
    expect(onSelect).toHaveBeenCalledWith('a');
  });

  it('calls onNew and onMenu from the icon buttons', () => {
    const { onNew, onMenu } = mount();
    expect(button('Open a new agent').disabled).toBe(false);
    act(() => button('Open a new agent').click());
    expect(onNew).toHaveBeenCalledTimes(1);
    act(() => button('Menu').click());
    expect(onMenu).toHaveBeenCalledTimes(1);
  });

  it('has a search icon only when a search is given, and opens search from it', () => {
    mount();
    expect(host.querySelector('button[aria-label="Search all chats"]')).toBeNull();
    act(() => root.unmount());
    root = createRoot(host);
    const onSearch = vi.fn();
    act(() => {
      root.render(<AgentTabBar panes={panes} labels={labels} active="all" bridgeReady onSelect={vi.fn()} onNew={vi.fn()} onSearch={onSearch} onMenu={vi.fn()} />);
    });
    act(() => button('Search all chats').click());
    expect(onSearch).toHaveBeenCalledTimes(1);
  });

  it('disables the new-agent button and marks the connection off when the bridge is not ready', () => {
    const { onNew } = mount({ bridgeReady: false });
    expect(button('Open a new agent').disabled).toBe(true);
    act(() => button('Open a new agent').click());
    expect(onNew).not.toHaveBeenCalled();
    const conn = host.querySelector('.ph-conn');
    expect(conn?.classList.contains('ph-conn--off')).toBe(true);
  });

  it('shows the one number on the All tab, not its own count of waiting panes', () => {
    // One pane is waiting, but the PC says three things need you (a question, a consent card, the pane).
    mount({ needsYou: 3 });
    expect(host.querySelector('.ph-count')?.textContent).toBe('3');
    act(() => root.unmount());
    root = createRoot(host);
    mount({ needsYou: 0 });
    expect(host.querySelector('.ph-count')).toBeNull();
  });

  it('falls back to counting waiting panes when the PC sends no number (an older PC)', () => {
    mount();
    expect(host.querySelector('.ph-count')?.textContent).toBe('1');
  });
});
