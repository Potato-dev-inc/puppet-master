// @vitest-environment jsdom
// The card for a companion whose home is too broad: it says where the home is and why that is a problem,
// and one button moves the companion to a folder of its own. Nothing moves before the click.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CompanionView } from '../../../lib/companion';
import { HomeProblemCard } from './HomeProblemCard';

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
  id: 'pipi',
  name: 'Pipi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'Desktop',
  state: 'idle',
  stateLine: 'Here.',
  paneId: 'companion-pipi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  homeProblem: { place: 'your Desktop', suggestedFolder: 'C:\Users\you\PuppetMaster\companions\pipi' },
};
const button = () => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('to its own folder'));

describe('HomeProblemCard', () => {
  it('says where the home is and what that does, and names the new folder', async () => {
    await act(async () => root.render(<HomeProblemCard companion={view} onMove={() => {}} />));
    expect(host.textContent).toContain("Pipi's home is your Desktop, so other agents there think they are Pipi.");
    expect(host.textContent).toContain('C:\Users\you\PuppetMaster\companions\pipi');
    expect(button()?.textContent).toBe('Move Pipi to its own folder');
  });

  it('moves nothing until the button is pressed, then moves that companion', async () => {
    const move = vi.fn(() => Promise.resolve());
    await act(async () => root.render(<HomeProblemCard companion={view} onMove={move} />));
    expect(move).not.toHaveBeenCalled();
    await act(async () => button()!.click());
    expect(move).toHaveBeenCalledExactlyOnceWith('pipi');
  });

  it('shows the reason in plain words when the move is refused, and keeps the button', async () => {
    const move = () => Promise.reject(new Error('COMPANION_REHOME_FAILED: The new folder is not empty. Pick an empty folder.'));
    await act(async () => root.render(<HomeProblemCard companion={view} onMove={move} />));
    await act(async () => button()!.click());
    expect(host.textContent).toContain('The new folder is not empty. Pick an empty folder.');
    expect(host.textContent).not.toContain('COMPANION_REHOME_FAILED');
    expect(button()!.disabled).toBe(false);
  });

  it('shows nothing for a companion whose home is fine', async () => {
    await act(async () => root.render(<HomeProblemCard companion={{ ...view, homeProblem: null }} onMove={() => {}} />));
    expect(host.textContent).toBe('');
  });
});
