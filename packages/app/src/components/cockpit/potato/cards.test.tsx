// @vitest-environment jsdom
// The two cards: a helper request (nothing starts without Start) and the card for a bot that cannot answer.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BrainStateCard } from './BrainStateCard';
import { HelperCard } from './HelperCard';
import { buttonByText } from './test-helpers';

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

describe('HelperCard', () => {
  const card = (over: Partial<Parameters<typeof HelperCard>[0]> = {}) => (
    <HelperCard
      name="Fish"
      task="fix the failing build"
      project="demo-a"
      agentLabel="Claude Code"
      open
      onStart={() => Promise.resolve()}
      onNotNow={() => Promise.resolve()}
      loadFolders={() => Promise.resolve(['demo-a', 'demo-b'])}
      {...over}
    />
  );

  it('says what the bot wants, the folder and the agent, with three buttons', () => {
    act(() => root.render(card()));
    expect(host.textContent).toContain('Fish wants a helper: fix the failing build');
    expect(host.textContent).toContain('Folder: demo-a');
    expect(host.textContent).toContain('Agent: Claude Code');
    expect([...host.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Start', 'Change folder', 'Not now']);
  });

  it('Start runs the card as it stands, with no folder of its own', async () => {
    const onStart = vi.fn(() => Promise.resolve());
    await act(async () => root.render(card({ onStart })));
    await act(async () => buttonByText(host, 'Start')!.click());
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(onStart).toHaveBeenCalledWith(undefined);
  });

  it('Not now answers not now and starts nothing', async () => {
    const onStart = vi.fn(() => Promise.resolve());
    const onNotNow = vi.fn(() => Promise.resolve());
    await act(async () => root.render(card({ onStart, onNotNow })));
    await act(async () => buttonByText(host, 'Not now')!.click());
    expect(onNotNow).toHaveBeenCalledTimes(1);
    expect(onStart).not.toHaveBeenCalled();
  });

  it('Change folder lists the recent project names; the one you pick goes with Start, and picking starts nothing', async () => {
    const onStart = vi.fn(() => Promise.resolve());
    const loadFolders = vi.fn(() => Promise.resolve(['demo-a', 'demo-b', 'notes']));
    await act(async () => root.render(card({ onStart, loadFolders })));
    await act(async () => buttonByText(host, 'Change folder')!.click());
    expect(loadFolders).toHaveBeenCalledTimes(1);
    expect([...host.querySelectorAll('[aria-label="Recent folders"] button')].map((b) => b.textContent)).toEqual(['demo-a', 'demo-b', 'notes']);
    await act(async () => buttonByText(host, 'notes')!.click());
    expect(onStart).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Folder: notes');
    expect(host.querySelector('[aria-label="Recent folders"]')).toBeNull();
    await act(async () => buttonByText(host, 'Start')!.click());
    expect(onStart).toHaveBeenCalledWith('notes');
  });

  it('Change folder with no recent project says so, and a second press closes the list', async () => {
    await act(async () => root.render(card({ loadFolders: () => Promise.resolve([]) })));
    await act(async () => buttonByText(host, 'Change folder')!.click());
    expect(host.textContent).toContain('No recent folders yet.');
    await act(async () => buttonByText(host, 'Change folder')!.click());
    expect(host.textContent).not.toContain('No recent folders yet.');
  });

  it('shows the sentence the app gave when Start is refused, and keeps the buttons', async () => {
    const refusal = new Error('POTATO_FOLDER_REFUSED: That folder is too broad for a helper. Choose a project folder.');
    await act(async () => root.render(card({ onStart: () => Promise.reject(refusal) })));
    await act(async () => buttonByText(host, 'Start')!.click());
    expect(host.textContent).toContain('That folder is too broad for a helper. Choose a project folder.');
    expect(host.textContent).not.toContain('POTATO_FOLDER_REFUSED');
    expect(buttonByText(host, 'Start')!.disabled).toBe(false);
  });

  it('has no buttons once somebody answered, and says the folder is not chosen when the bot named none', () => {
    act(() => root.render(card({ open: false, project: undefined })));
    expect(host.querySelectorAll('button').length).toBe(0);
    expect(host.textContent).toContain('Folder: not chosen yet');
  });
});

describe('BrainStateCard', () => {
  const base = { name: 'Fish', state: 'ready' as const, lastError: null, limitCents: 50, onStart: () => Promise.resolve(), onInstallNode: () => Promise.resolve(), onOpenSettings: () => {} };
  const show = (over: Partial<Parameters<typeof BrainStateCard>[0]>) => act(() => root.render(<BrainStateCard {...base} {...over} />));

  it('draws nothing while the bot can answer', () => {
    for (const state of ['ready', 'idle', 'starting', 'off', 'agents'] as const) {
      show({ state });
      expect(host.textContent, state).toBe('');
    }
  });

  it('asks for the DeepSeek key', () => {
    const onOpenSettings = vi.fn();
    show({ state: 'needs_key', onOpenSettings });
    expect(host.textContent).toContain('Add your DeepSeek key in Settings to talk with Fish.');
    act(() => buttonByText(host, 'Open Settings')!.click());
    expect(onOpenSettings).toHaveBeenCalled();
  });

  it('asks for Node.js 20 and the Install Node.js button runs the install', async () => {
    const onInstallNode = vi.fn(() => Promise.resolve());
    show({ state: 'needs_node', onInstallNode });
    expect(host.textContent).toContain('Potato Bot needs Node.js 20 or newer.');
    await act(async () => buttonByText(host, 'Install Node.js')!.click());
    expect(onInstallNode).toHaveBeenCalledTimes(1);
  });

  it('says why the install did not start, in the app\'s own words', async () => {
    show({ state: 'needs_node', onInstallNode: () => Promise.reject(new Error('INSTALL_UNAVAILABLE: This PC cannot install Node.js from here (winget is missing). Download it from its website.')) });
    await act(async () => buttonByText(host, 'Install Node.js')!.click());
    expect(host.textContent).toContain('This PC cannot install Node.js from here');
    expect(buttonByText(host, 'Install Node.js')!.disabled).toBe(false);
  });

  it('says the day limit with the dollars', () => {
    show({ state: 'daily_limit', limitCents: 50 });
    expect(host.textContent).toContain("Fish has used today's limit ($0.50). Raise it in Settings or wait until tomorrow.");
  });

  it('says the brain stopped and Start again starts it', async () => {
    const onStart = vi.fn(() => Promise.resolve());
    show({ state: 'stopped', onStart });
    expect(host.textContent).toContain("Fish's brain stopped.");
    await act(async () => buttonByText(host, 'Start again')!.click());
    expect(onStart).toHaveBeenCalled();
  });

  it('says the last error in a plain sentence while the brain runs', () => {
    show({ state: 'ready', lastError: 'PROVIDER_DOWN' });
    expect(host.textContent).toContain('The model is not answering right now. Try again in a minute.');
  });
});
