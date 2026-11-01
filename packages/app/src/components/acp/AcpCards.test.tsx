// @vitest-environment jsdom
// The install prompt and the sign-in card.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AddonStatus } from '../../lib/acpAddon';
import { AddonPrompt } from './AddonPrompt';
import { SignInCard } from './SignInCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const status = (over: Partial<AddonStatus> = {}): AddonStatus => ({
  agent: 'claude',
  label: 'Claude',
  state: 'not_installed',
  version: null,
  pinned_version: '0.74.0',
  detail: '',
  size_mb: 48,
  lines: [],
  signin_hint: '',
  ...over,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const button = (text: string | RegExp) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => (typeof text === 'string' ? b.textContent === text : text.test(b.textContent ?? '')));

describe('AddonPrompt', () => {
  it('says what chat needs and offers Install', async () => {
    const onInstall = vi.fn();
    await act(async () => root.render(<AddonPrompt status={status()} onInstall={onInstall} />));
    expect(host.textContent).toContain('Chat needs a small add-on for Claude (about 48 MB).');
    await act(async () => button('Install')!.click());
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it('shows the newest progress line while installing, with no button', async () => {
    await act(async () => root.render(<AddonPrompt status={status({ state: 'installing', lines: ['Downloading x…', 'npm http fetch GET 200'] })} onInstall={vi.fn()} />));
    expect(host.textContent).toContain('Installing the chat add-on for Claude…');
    expect(host.querySelector('[data-testid="addon-progress"]')?.textContent).toBe('npm http fetch GET 200');
    expect(host.querySelector('button')).toBeNull();
  });

  it('says why it failed and offers to try again', async () => {
    const onInstall = vi.fn();
    await act(async () => root.render(<AddonPrompt status={status({ state: 'failed', detail: 'Node.js is needed to install the chat add-on.' })} onInstall={onInstall} />));
    expect(host.querySelector('[data-testid="addon-error"]')?.textContent).toContain('Node.js is needed');
    await act(async () => button('Try again')!.click());
    expect(onInstall).toHaveBeenCalledTimes(1);
  });
});

describe('SignInCard', () => {
  it('opens the sign-in terminal, then tries again once the person says they are signed in', async () => {
    const onOpen = vi.fn(async () => undefined);
    const onRetry = vi.fn(async () => undefined);
    await act(async () => root.render(<SignInCard label="Claude" hint="Type /login in the terminal." onOpen={onOpen} onRetry={onRetry} />));
    expect(host.textContent).toContain('Sign in to Claude to use Chat');
    expect(host.textContent).toContain('Type /login in the terminal.');
    await act(async () => button('Open sign-in')!.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onRetry).not.toHaveBeenCalled();
    await act(async () => button(/signed in — try again/)!.click());
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('shows the reason in plain words when the agent still wants a sign-in', async () => {
    const onRetry = vi.fn(async () => Promise.reject(new Error('ACP_SIGN_IN_NEEDED: The agent needs you to sign in.')));
    await act(async () => root.render(<SignInCard label="Codex" hint="" onOpen={vi.fn()} onRetry={onRetry} />));
    await act(async () => button(/try again/)!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('The agent needs you to sign in.');
    // A general sentence when the agent's table gave no hint.
    expect(host.textContent).toContain('same sign-in as the terminal');
  });
});
