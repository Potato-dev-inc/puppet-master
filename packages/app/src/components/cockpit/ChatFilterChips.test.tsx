// @vitest-environment jsdom
// The chips under "Search all chats": four chips, each with a menu of choices from the index.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatFilters } from '../../lib/chatFilters';
import { ChatFilterChips, type ChipKey } from './ChatFilterChips';

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

const facets = { agents: ['codex', 'claude-code'], folders: ['demo-a'] };
const agentName = (id: string) => (id === 'codex' ? 'Codex CLI' : 'Claude Code');

function render(filters: ChatFilters = {}) {
  const onPick = vi.fn<(key: ChipKey, value: string | undefined) => void>();
  act(() => root.render(<ChatFilterChips filters={filters} facets={facets} agentName={agentName} onPick={onPick} />));
  return onPick;
}

const chip = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('.ck-chat-chip')].find((b) => b.textContent?.startsWith(label))!;
const options = () => [...host.querySelectorAll<HTMLButtonElement>('[role="option"]')];
const choices = () => options().map((b) => b.textContent);
const click = (el: Element) => act(() => (el as HTMLElement).click());

describe('ChatFilterChips', () => {
  it('shows the four chips, each with a down arrow when nothing is picked', () => {
    render();
    expect([...host.querySelectorAll('.ck-chat-chip')].map((b) => b.textContent)).toEqual(['Agent ▾', 'Folder ▾', 'When ▾', 'Who ▾']);
    expect(host.querySelector('.ck-chat-chip.is-on')).toBeNull();
  });

  it('Agent lists Any agent and the agents the index holds, by their names', () => {
    render();
    click(chip('Agent'));
    expect(choices()).toEqual(['Any agent', 'Codex CLI', 'Claude Code']);
  });

  it('picking an agent reports its id and closes the menu', () => {
    const onPick = render();
    click(chip('Agent'));
    click(options()[1]);
    expect(onPick).toHaveBeenCalledWith('agent', 'codex');
    expect(host.querySelector('[role="listbox"]')).toBeNull();
  });

  it('Any clears the chip; a picked chip shows its choice', () => {
    const onPick = render({ folder: 'demo-a' });
    expect(chip('Folder').textContent).toBe('Folder: demo-a ▾');
    expect(chip('Folder').classList.contains('is-on')).toBe(true);
    click(chip('Folder'));
    click(options()[0]);
    expect(onPick).toHaveBeenCalledWith('folder', undefined);
  });

  it('When and Who offer their words, and report the one picked', () => {
    const onPick = render({ when: '7d' });
    expect(chip('When').textContent).toBe('When: 7 days ▾');
    click(chip('When'));
    expect(choices()).toEqual(['Any', 'Today', '7 days', '30 days']);
    click(options()[1]);
    expect(onPick).toHaveBeenCalledWith('when', 'today');
    click(chip('Who'));
    expect(choices()).toEqual(['Anyone', 'You', 'Agent', 'Tools']);
    click(options()[3]);
    expect(onPick).toHaveBeenLastCalledWith('who', 'tool');
  });

  it('only one menu is open at a time, and a chip closes its own menu when clicked again', () => {
    render();
    click(chip('Agent'));
    click(chip('Folder'));
    expect(host.querySelectorAll('[role="listbox"]')).toHaveLength(1);
    expect(host.querySelector('[role="listbox"]')?.getAttribute('aria-label')).toBe('Folder');
    click(chip('Folder'));
    expect(host.querySelector('[role="listbox"]')).toBeNull();
  });
});
