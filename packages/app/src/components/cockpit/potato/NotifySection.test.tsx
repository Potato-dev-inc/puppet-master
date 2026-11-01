// @vitest-environment jsdom
// Notifications of a bot: the saved choice shows, a pick is saved at once, "When it needs me" says it is coming, and a
// save that fails keeps the old choice and says why.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeNotifyTransport, type NotifyTransport } from '../../../lib/potato-notify';
import { NOTIFY_COPY, NotifySection } from './NotifySection';

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

async function show(transport: NotifyTransport) {
  await act(async () => root.render(<NotifySection botId="nimbus" transport={transport} />));
  await act(async () => {});
}

const radio = (value: string) => host.querySelector<HTMLInputElement>(`input[type="radio"][value="${value}"]`)!;
const checked = () => host.querySelector<HTMLInputElement>('input[type="radio"]:checked')?.value;
const alertText = () => host.querySelector('[role="alert"]')?.textContent;

describe('Notifications', () => {
  it('shows the three choices and the saved one', async () => {
    await show(fakeNotifyTransport({ choices: { nimbus: 'never' } }));
    expect([...host.querySelectorAll('label.ck-potato-radio')].map((l) => l.textContent)).toEqual(['When it replies', 'When it needs me', 'Never']);
    expect(checked()).toBe('never');
  });

  it('a bot with no saved choice says it replies', async () => {
    await show(fakeNotifyTransport());
    expect(checked()).toBe('reply');
  });

  it('saves a pick at once and says the needs-me push is coming', async () => {
    const transport = fakeNotifyTransport();
    await show(transport);
    await act(async () => radio('needs_you').click());
    expect(transport.calls.at(-1)).toEqual({ method: 'set', args: ['nimbus', 'needs_you'] });
    expect(checked()).toBe('needs_you');
    expect(host.textContent).toContain(NOTIFY_COPY.needsYouLater);

    await act(async () => radio('never').click());
    expect(checked()).toBe('never');
    expect(host.textContent).not.toContain(NOTIFY_COPY.needsYouLater);
  });

  it('keeps the old choice and shows the sentence when a save fails', async () => {
    const transport: NotifyTransport = {
      get: async () => 'reply',
      set: () => Promise.reject(new Error(NOTIFY_COPY.saveFailed)),
    };
    await show(transport);
    await act(async () => radio('never').click());
    expect(checked()).toBe('reply');
    expect(alertText()).toBe(NOTIFY_COPY.saveFailed);
  });

  it('the choices are off until the saved one has been read', async () => {
    let answer: (value: 'reply') => void = () => {};
    const transport: NotifyTransport = {
      get: () => new Promise<'reply'>((resolve) => (answer = resolve)),
      set: async (_bot, value) => value,
    };
    await act(async () => root.render(<NotifySection botId="nimbus" transport={transport} />));
    expect(host.querySelector('fieldset')!.disabled).toBe(true);
    await act(async () => answer('reply'));
    expect(host.querySelector('fieldset')!.disabled).toBe(false);
  });
});
