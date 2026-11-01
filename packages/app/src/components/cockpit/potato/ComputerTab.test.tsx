// @vitest-environment jsdom
// The Computer tab: the upkeep section (Running / Stopped, disk, Start / Stop / Restart, Open sandbox settings) goes through
// the potato_sandbox_* commands; a computer that is not set up shows the "Set up the computer" card that opens the wizard;
// with Labs off the tab says how to turn the sandbox on; a stopped computer is asleep in the preview and wakes with Start.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeSandboxTransport, type FakeSandboxTransport, type FakeSandboxSeed } from '../../../lib/potato-sandbox';
import { fakeScreenTransport, type FakeScreenTransport } from '../../../lib/potatoScreen';
import { ComputerTab, type ComputerTabProps } from './ComputerTab';
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
  document.querySelectorAll('.ck-potato-wizard-host').forEach((el) => el.remove());
});

const BOT = { id: 'fish', name: 'Fish' };
const GB = 1024 ** 3;
const running: FakeSandboxSeed = { status: { state: 'running', diskBytes: 3.7 * GB, hasResetPoint: true } };

async function mount(over: Partial<ComputerTabProps> & { sandboxSeed?: FakeSandboxSeed; screen?: FakeScreenTransport } = {}) {
  const sandbox = fakeSandboxTransport(over.sandboxSeed ?? running);
  const screen = over.screen ?? fakeScreenTransport();
  const props: ComputerTabProps = { bot: BOT, screen, sandbox, through: false, onNotify: () => {}, ...over };
  await act(async () => root.render(<ComputerTab {...props} />));
  await act(async () => {});
  return { sandbox, screen };
}

const click = async (label: string) => {
  const button = buttonByText(host, label);
  expect(button, `a button named ${label}`).toBeTruthy();
  await act(async () => button!.click());
  await act(async () => {});
};
const line = () => host.querySelector('.ck-potato-maint-line')?.textContent;
const names = (sandbox: FakeSandboxTransport) => sandbox.calls.map((c) => c.method).filter((m) => m === 'start' || m === 'stop');

describe('the upkeep section', () => {
  it('says Running with the disk it uses, and has Stop, Restart and the way to the settings', async () => {
    await mount({ onOpenSettings: () => {} });
    expect(host.querySelector('.ck-potato-computer')).toBeTruthy();
    expect(line()).toBe('Running. Uses 3.7 GB');
    expect([...host.querySelectorAll('.ck-potato-maint button')].map((b) => b.textContent)).toEqual(['Stop', 'Restart', 'Open sandbox settings']);
  });

  it('Stop stops it, the preview falls asleep, and Start wakes it', async () => {
    const { sandbox } = await mount();
    expect(host.querySelector('iframe')).toBeTruthy();
    await click('Stop');
    expect(names(sandbox)).toEqual(['stop']);
    expect(line()).toBe('Stopped. Uses 3.7 GB');
    expect(host.querySelector('iframe')).toBeNull();
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Asleep');
    expect(buttonByText(host, 'Stop')).toBeUndefined();
    await click('Start');
    expect(names(sandbox)).toEqual(['stop', 'start']);
    expect(line()).toBe('Running. Uses 3.7 GB');
    expect(host.querySelector('iframe')).toBeTruthy();
  });

  it('Restart stops it and starts it again, in that order', async () => {
    const { sandbox } = await mount();
    await click('Restart');
    expect(names(sandbox)).toEqual(['stop', 'start']);
    expect(line()).toBe('Running. Uses 3.7 GB');
  });

  it('the Wake button of the asleep preview starts the computer', async () => {
    const { sandbox } = await mount({ sandboxSeed: { status: { state: 'stopped', diskBytes: 3.7 * GB } } });
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Asleep');
    await click('Wake');
    expect(names(sandbox)).toEqual(['start']);
    expect(host.querySelector('iframe')).toBeTruthy();
  });

  it('a call that fails says one plain sentence', async () => {
    const told = vi.fn();
    const sandbox = fakeSandboxTransport({ status: { state: 'stopped' } });
    sandbox.start = () => Promise.reject(new Error('SANDBOX_NOT_READY: Windows is still starting its Linux part.'));
    await act(async () => root.render(<ComputerTab bot={BOT} screen={fakeScreenTransport()} sandbox={sandbox} through={false} onNotify={told} />));
    await act(async () => {});
    await click('Start');
    expect(told).toHaveBeenCalledWith('Windows is still starting its Linux part.');
  });

  it('Open sandbox settings goes to the settings', async () => {
    const onOpenSettings = vi.fn();
    await mount({ onOpenSettings });
    await click('Open sandbox settings');
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('has no upkeep section when nothing can read the sandbox, and the screen is shown as it is', async () => {
    await mount({ sandbox: undefined });
    expect(host.querySelector('.ck-potato-maint')).toBeNull();
    expect(host.querySelector('iframe')).toBeTruthy();
  });
});

describe('a computer that is not set up', () => {
  it('shows "Set up the computer" in place of the preview, and the button opens the wizard', async () => {
    const { sandbox } = await mount({ sandboxSeed: { status: { state: 'not_installed' } } });
    const card = host.querySelector('.ck-potato-setup');
    expect(card?.textContent).toContain('Set up the computer');
    expect(card?.textContent).toContain('Fish can use a small private computer');
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    expect(host.querySelector('.ck-potato-maint')).toBeNull();
    expect(document.querySelector('.ck-potato-wizard-host')).toBeNull();
    await click('Set up the computer');
    const wizard = document.querySelector('.ck-potato-wizard-host');
    expect(wizard).toBeTruthy();
    expect(wizard!.classList.contains('settings')).toBe(true);
    expect(wizard!.textContent).toContain('Set up the sandbox');
    expect(sandbox.calls.map((c) => c.method)).toContain('menu');
  });

  it('while it is being set up the card says how far it is, and Show opens the wizard', async () => {
    await mount({ sandboxSeed: { status: { state: 'installing', install: { state: 'running', percent: 42, step: 'image', title: 'Downloading Ubuntu', index: 1, of: 4, detail: null, cancelling: false, error: null, needsLinuxFeature: false } } } });
    expect(host.querySelector('.ck-potato-setup')?.textContent).toContain('Setting up the computer… 42%');
    await click('Show');
    expect(document.querySelector('.ck-potato-wizard-host')).toBeTruthy();
  });

  it('the card turns into the preview once the computer is set up', async () => {
    const { sandbox } = await mount({ sandboxSeed: { status: { state: 'not_installed' } } });
    expect(host.querySelector('.ck-potato-setup')).toBeTruthy();
    await act(async () => sandbox.patch({ state: 'running', diskBytes: 3.7 * GB }));
    await act(async () => {});
    expect(host.querySelector('.ck-potato-setup')).toBeNull();
    expect(host.querySelector('.ck-potato-computer')).toBeTruthy();
    expect(host.querySelector('.ck-potato-maint')).toBeTruthy();
  });
});

describe('when there is no computer to show', () => {
  it('Labs off: one line that says how to turn Sandbox on, and nothing else', async () => {
    const { sandbox } = await mount({ screen: fakeScreenTransport({ enabled: false }) });
    expect(host.textContent).toBe('Fish has no computer here. Turn on Sandbox in Settings > General > Labs.');
    expect(host.querySelector('.ck-potato-maint')).toBeNull();
    expect(host.querySelector('.ck-potato-setup')).toBeNull();
    expect(host.querySelector('iframe')).toBeNull();
    void sandbox;
  });

  it('no screen at all says the same line', async () => {
    await mount({ screen: undefined as unknown as FakeScreenTransport });
    expect(host.textContent).toBe('Fish has no computer here. Turn on Sandbox in Settings > General > Labs.');
  });

  it('a bot that talks through the agents has no computer of its own, and nothing is asked', async () => {
    const { screen, sandbox } = await mount({ through: true });
    expect(host.textContent).toBe('A bot that talks through your agents has no computer of its own.');
    expect(screen.calls).toHaveLength(0);
    expect(sandbox.calls).toHaveLength(0);
  });
});
