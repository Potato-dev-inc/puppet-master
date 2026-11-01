// @vitest-environment jsdom
// The way in: the Artifacts button in the bot list's header takes the thread's place; opening a bot gives the thread back.
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeCompanionTransport } from '../../../lib/companion';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoState } from '../../../lib/potato';
import { usePotatoState } from '../../../hooks/usePotato';
import { PotatoPage } from './PotatoPage';
import { bot, msg } from './test-helpers';

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

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)));

function Harness({ transport }: { transport: FakePotatoTransport }) {
  const potato = usePotatoState(transport);
  const [openId, setOpenId] = useState<string | null>(null);
  if (!potato.state) return null;
  return (
    <PotatoPage
      state={potato.state as PotatoState}
      version={potato.version}
      transport={transport}
      companionTransport={fakeCompanionTransport({ companions: [] })}
      companions={[]}
      openBotId={openId}
      onOpenBot={setOpenId}
      onChanged={potato.refresh}
      onCompanionsChanged={() => {}}
      onNotify={() => {}}
      onOpenSettings={() => {}}
    />
  );
}

describe('the Artifacts page inside Potato Bot', () => {
  it('takes the thread place from the list header, and the thread comes back when a bot is opened', async () => {
    const transport = fakePotatoTransport({
      bots: [bot({ id: 'fish', name: 'Fish' })],
      messages: { fish: [msg({ text: 'hello there' })] },
      artifacts: { items: [{ id: 'a-1', name: 'plan.md', bot: 'fish', kind: 'markdown', size: 3, created: 1, version: 1, file: 'a-1.md', content: 'the plan' }] },
    });
    act(() => {
      root.render(<Harness transport={transport} />);
    });
    await settle();
    click(host.querySelector('.ck-potato-conv'));
    await settle();
    expect(host.textContent).toContain('hello there');
    expect(host.querySelector('.ck-artifacts')).toBeNull();

    click(host.querySelector('button[aria-label="Artifacts"]'));
    await settle();
    expect(host.querySelector('.ck-artifacts')).not.toBeNull();
    expect(host.textContent).not.toContain('hello there');
    expect(host.querySelector('button[aria-label="Artifacts"]')?.getAttribute('aria-pressed')).toBe('true');
    expect(host.querySelector('.ck-artifact-row-name')?.textContent).toBe('plan.md');

    click(host.querySelector('.ck-potato-conv'));
    await settle();
    expect(host.querySelector('.ck-artifacts')).toBeNull();
    expect(host.textContent).toContain('hello there');
  });

  it('the Back to chat button gives the thread back as well', async () => {
    const transport = fakePotatoTransport({ bots: [bot({ id: 'fish', name: 'Fish' })], messages: { fish: [msg({ text: 'hi' })] } });
    act(() => {
      root.render(<Harness transport={transport} />);
    });
    await settle();
    click(host.querySelector('.ck-potato-conv'));
    await settle();
    click(host.querySelector('button[aria-label="Artifacts"]'));
    await settle();
    const back = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Back to chat');
    click(back ?? null);
    await settle();
    expect(host.querySelector('.ck-artifacts')).toBeNull();
  });
});

function click(el: Element | null | undefined) {
  if (!el) throw new Error('missing element');
  act(() => (el as HTMLElement).click());
}
