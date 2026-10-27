// @vitest-environment jsdom
// The Inbox card of a companion that keeps stopping: "Try again" restarts it, "Open terminal" opens its pane.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CompanionView } from '../../../lib/companion';
import { CompanionStoppedCard } from './StoppedCard';

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

const view: CompanionView = {
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'companion-test',
  state: 'stopped',
  stateLine: 'Mochi keeps stopping. Open the terminal to see why.',
  paneId: null,
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 3,
  autoStart: true,
};
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);

describe('CompanionStoppedCard', () => {
  it('names the companion and Try again restarts that one', async () => {
    const restart = vi.fn(() => Promise.resolve());
    await act(async () => root.render(<CompanionStoppedCard companion={view} onTryAgain={restart} />));
    expect(host.textContent).toContain('Mochi stopped 3 times');
    await act(async () => button('Try again')!.click());
    expect(restart).toHaveBeenCalledWith('mochi');
  });

  it('says so when the restart is refused, and keeps the button', async () => {
    await act(async () => root.render(<CompanionStoppedCard companion={view} onTryAgain={() => Promise.reject(new Error('no'))} />));
    await act(async () => button('Try again')!.click());
    expect(host.textContent).toContain('Could not start it');
    expect(button('Try again')!.disabled).toBe(false);
  });

  it('offers the terminal only when there is a pane to open', async () => {
    const open = vi.fn();
    await act(async () => root.render(<CompanionStoppedCard companion={view} onTryAgain={() => {}} onOpenPane={open} />));
    await act(async () => button('Open terminal')!.click());
    expect(open).toHaveBeenCalled();
    await act(async () => root.render(<CompanionStoppedCard companion={view} onTryAgain={() => {}} />));
    expect(button('Open terminal')).toBeUndefined();
  });
});
