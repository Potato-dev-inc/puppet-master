// @vitest-environment jsdom
// The bot panel reads the bot's Computer switch (Advanced > Tools) when the Computer tab is shown: off, the tab says so, and
// the link opens the Advanced tab.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeBotSettingsTransport } from '../../../lib/potato-botsettings';
import { fakeScreenTransport } from '../../../lib/potatoScreen';
import { BotPanel } from './BotPanel';
import { buttonByText } from './test-helpers';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => { throw new Error('no engine'); }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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

async function mount(botSettings: ReturnType<typeof fakeBotSettingsTransport>, onTab = vi.fn()) {
  await act(async () =>
    root.render(
      <BotPanel bot={{ id: 'fish', name: 'Fish' }} avatar="mochi" tab="computer" onTab={onTab} onClose={() => {}} screen={fakeScreenTransport()} through={false} onNotify={() => {}} botSettings={botSettings} />,
    ),
  );
  await act(async () => {});
  return onTab;
}

describe('the Computer tab and the bot Computer switch', () => {
  it('shows "Computer is off for this bot" when the switch is off, and the link goes to Advanced', async () => {
    const transport = fakeBotSettingsTransport({ sandboxOn: true });
    await transport.setTools('fish', { helpers: true, computer: false, web: true });
    const onTab = await mount(transport);
    expect(host.querySelector('.ck-potato-computer-toolsoff')!.textContent).toContain('Computer is off for this bot.');
    await act(async () => buttonByText(host, 'Turn it on in Advanced › Tools')!.click());
    expect(onTab).toHaveBeenCalledWith('advanced');
  });

  it('says nothing while the switch is on', async () => {
    await mount(fakeBotSettingsTransport({ sandboxOn: true }));
    expect(host.querySelector('.ck-potato-computer-toolsoff')).toBeNull();
  });
});
