// @vitest-environment jsdom
// The sidebar row: face, name and state word; the needs-you tint; no count of its own; the quiet "Make" row.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CompanionView } from '../../../lib/companion';
import { CompanionRow, MakeCompanionRow } from './CompanionRow';

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

const view = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'companion-test',
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

describe('CompanionRow', () => {
  it('shows the face, the name and the state word', () => {
    act(() => root.render(<CompanionRow companion={view()} active={false} onOpen={() => {}} />));
    expect(host.querySelector('svg[role="img"]')?.getAttribute('aria-label')).toBe('Mochi, Here');
    expect(host.querySelector('.ck-row-title')?.textContent).toBe('Mochi');
    expect(host.querySelector('.ck-companion-state')?.textContent).toBe('Here');
  });

  it('says Needs you in the state word and tints the row, with no count of its own', () => {
    act(() => root.render(<CompanionRow companion={view({ state: 'needs_you', needs: { itemId: 'a1', title: 'Approve a command' } })} active={false} onOpen={() => {}} />));
    const row = host.querySelector('.ck-companion-row')!;
    expect(row.classList.contains('is-needs')).toBe(true);
    expect(host.querySelector('.ck-companion-state')?.textContent).toBe('Needs you');
    expect(host.querySelector('.ck-count')).toBeNull();
  });

  it('opens the companion when clicked and marks the open one', () => {
    const onOpen = vi.fn();
    act(() => root.render(<CompanionRow companion={view()} active onOpen={onOpen} />));
    const row = host.querySelector<HTMLButtonElement>('button')!;
    expect(row.getAttribute('aria-current')).toBe('true');
    act(() => row.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});

describe('MakeCompanionRow', () => {
  it('asks to make a companion, and calls back on click', () => {
    const onMake = vi.fn();
    act(() => root.render(<MakeCompanionRow onMake={onMake} />));
    expect(host.textContent).toContain('Make a companion');
    act(() => host.querySelector<HTMLButtonElement>('button')!.click());
    expect(onMake).toHaveBeenCalledTimes(1);
  });
});
