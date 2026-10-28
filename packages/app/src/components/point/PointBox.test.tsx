// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PickView, PointState, Target } from '../../lib/point-api';
import { pointApi } from '../../lib/point-api';
import { PointBox, MAX_PICKS, type PointBoxProps } from './PointBox';
import { PointBoxApp } from './PointBoxApp';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Layout is not measured in jsdom, so this guards the stylesheet rule that stops the pick list from shrinking to one line.
const POINT_CSS = readFileSync(path.resolve(__dirname, '../../styles/point-box.css'), 'utf8');

const PICK: PickView = {
  id: 'pick-1',
  page: 'localhost:5173/pricing',
  title: 'Pricing',
  label: 'button.card-cta',
  text: 'Get started',
  component: 'PricingCard',
  source: { relPath: 'src/components/PricingCard.tsx', line: 42 },
  thumb: 'data:image/png;base64,AAAA',
  shotPath: '.puppet-master/shots/pick-1.png',
};

const TARGETS: Target[] = [
  { paneId: 'p1', agentLabel: 'Agent A', title: 'fix-pricing', sameFolder: true, canSend: true },
  { paneId: 'p2', agentLabel: 'Agent B', title: 'other', sameFolder: false, canSend: true },
  { paneId: 'p3', agentLabel: 'Agent C', title: 'proto', sameFolder: false, canSend: false, reason: 'Agent C takes no typed messages yet.' },
];

const STATE: PointState = { picks: [PICK], targets: TARGETS, defaultTarget: 'p1', notice: null, session: 1 };

function props(over: Partial<PointBoxProps> = {}): PointBoxProps {
  return {
    state: STATE,
    request: 'Make it green',
    onRequest: vi.fn(),
    target: 'p1',
    onTarget: vi.fn(),
    onSend: vi.fn(),
    onEditInChat: vi.fn(),
    onCopy: vi.fn(),
    onPointAnother: vi.fn(),
    onRemove: vi.fn(),
    onDiscard: vi.fn(),
    onHide: vi.fn(),
    onOpenBlocked: vi.fn(),
    error: null,
    info: null,
    busy: false,
    ...over,
  };
}

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
  vi.restoreAllMocks();
});

function render(p: PointBoxProps) {
  act(() => {
    root.render(<PointBox {...p} />);
  });
}

const flush = () => act(async () => {});
const area = () => host.querySelector('textarea') as HTMLTextAreaElement;
const select = () => host.querySelector('select') as HTMLSelectElement;
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;
const text = () => host.textContent ?? '';

function keydown(el: HTMLElement, key: string, shiftKey = false) {
  act(() => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, shiftKey, bubbles: true, cancelable: true }));
  });
}

/** Set the value the way React sees a user edit, then fire the input event. */
function typeInto(el: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('PointBox', () => {
  it('renders the pick: label, quoted text, component and file with line, and the page', () => {
    render(props());
    const row = host.querySelector('.point-box-pick')!;
    expect(row.querySelector('.point-box-tag')!.textContent).toBe('button.card-cta');
    expect(row.querySelector('.point-box-text')!.textContent).toBe('"Get started"');
    expect(row.querySelector('.point-box-chip')!.textContent).toBe('PricingCard · src/components/PricingCard.tsx:42');
    expect(row.querySelector('.point-box-page')!.textContent).toBe('localhost:5173/pricing');
    expect(row.querySelector('img')!.getAttribute('src')).toBe(PICK.thumb);
    expect(host.textContent).toContain('1 of 3');
  });

  it('the pick list keeps its height and at least one full pick row (thumbnail included)', () => {
    const rule = POINT_CSS.match(/\.point-box-picks \{([^}]*)\}/)?.[1] ?? '';
    expect(rule).toMatch(/flex:\s*0 0 auto;/);
    const min = Number(/min-height:\s*(\d+)px/.exec(rule)?.[1]);
    expect(min).toBeGreaterThanOrEqual(60);
  });

  it('shows a grey tile with the reason as a tooltip when there is no screenshot', () => {
    render(props({ state: { ...STATE, picks: [{ ...PICK, thumb: null, noShotReason: 'Screenshots need Windows for now.' }] } }));
    expect(host.querySelector('img')).toBeNull();
    const tile = host.querySelector('.point-box-thumb--none')!;
    expect(tile.textContent).toBe('No screenshot');
    expect(tile.getAttribute('title')).toBe('Screenshots need Windows for now.');
  });

  it('removes one pick by its x button', () => {
    const p = props();
    render(p);
    act(() => (host.querySelector('.point-box-x') as HTMLButtonElement).click());
    expect(p.onRemove).toHaveBeenCalledWith('pick-1');
  });

  it('Enter sends, and Shift+Enter does not', () => {
    const p = props();
    render(p);
    keydown(area(), 'Enter', true);
    expect(p.onSend).not.toHaveBeenCalled();
    keydown(area(), 'Enter');
    expect(p.onSend).toHaveBeenCalledTimes(1);
  });

  it('Esc hides the box', () => {
    const p = props();
    render(p);
    keydown(area(), 'Escape');
    expect(p.onHide).toHaveBeenCalledTimes(1);
  });

  it('typing reports the new text through onRequest', () => {
    const p = props({ request: '' });
    render(p);
    typeInto(area(), 'Bigger');
    expect(p.onRequest).toHaveBeenCalledWith('Bigger');
  });

  it('a greyed pane is listed but cannot be chosen: Send and Edit in Chat wait, Copy works', () => {
    const p = props({ target: 'p3' });
    render(p);
    const options = Array.from(select().querySelectorAll('option')) as HTMLOptionElement[];
    expect(options.find((o) => o.value === 'p3')!.disabled).toBe(true);
    expect(options.find((o) => o.value === 'p2')!.disabled).toBe(false);
    expect(text()).toContain('Agent C takes no typed messages yet.');

    expect(button('Send')!.disabled).toBe(true);
    expect(button('Edit in Chat')!.disabled).toBe(true);
    expect(button('Copy')!.disabled).toBe(false);
    act(() => button('Copy')!.click());
    expect(p.onCopy).toHaveBeenCalledTimes(1);

    keydown(area(), 'Enter');
    expect(p.onSend).not.toHaveBeenCalled();
  });

  it('the To list shows the agent label, then the title only when it differs', () => {
    render(
      props({
        state: {
          ...STATE,
          targets: [
            { paneId: 'p1', agentLabel: 'Claude Code', title: 'Claude Code', sameFolder: true, canSend: true },
            { paneId: 'p2', agentLabel: 'Claude Code', title: 'fix-pricing', sameFolder: false, canSend: true },
          ],
        },
        target: 'p1',
      }),
    );
    const labels = Array.from(select().querySelectorAll('option')).map((o) => o.textContent);
    expect(labels).toEqual(['Claude Code', 'Claude Code · fix-pricing']);
  });

  it('choosing another pane reports it through onTarget', () => {
    const p = props();
    render(p);
    select().value = 'p2';
    act(() => {
      select().dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(p.onTarget).toHaveBeenCalledWith('p2');
  });

  it('shows the sentence for a refused send and keeps the text in the box', () => {
    render(props({ error: 'Shells take no messages.', request: 'Make it green' }));
    expect(host.querySelector('.point-box-error')!.textContent).toBe('Shells take no messages.');
    expect(host.querySelector('.point-box-error')!.getAttribute('role')).toBe('alert');
    expect(area().value).toBe('Make it green');
  });

  it('three picks disable Point another', () => {
    const many = [PICK, { ...PICK, id: 'pick-2' }, { ...PICK, id: 'pick-3' }];
    render(props({ state: { ...STATE, picks: many } }));
    expect(many.length).toBe(MAX_PICKS);
    expect(button('Point another')!.disabled).toBe(true);
    expect(host.textContent).toContain('3 of 3');
  });

  it('with two picks, Point another is on and calls onPointAnother', () => {
    const p = props({ state: { ...STATE, picks: [PICK, { ...PICK, id: 'pick-2' }] } });
    render(p);
    expect(button('Point another')!.disabled).toBe(false);
    act(() => button('Point another')!.click());
    expect(p.onPointAnother).toHaveBeenCalledTimes(1);
  });

  it('with no picks, asks for a pick and disables Send, Copy and Edit in Chat', () => {
    render(props({ state: { ...STATE, picks: [] } }));
    expect(text()).toContain('Click Point in the preview');
    expect(button('Send')!.disabled).toBe(true);
    expect(button('Copy')!.disabled).toBe(true);
    expect(button('Edit in Chat')!.disabled).toBe(true);
  });

  it('shows the notice with Open in browser, and the button calls onOpenBlocked', () => {
    const p = props({ state: { ...STATE, notice: 'Blocked a link to example.com. The preview only shows your app.' } });
    render(p);
    expect(host.querySelector('.point-box-notice')!.textContent).toContain('Blocked a link to example.com.');
    act(() => button('Open in browser')!.click());
    expect(p.onOpenBlocked).toHaveBeenCalledTimes(1);
  });

  it('Send shows Sending while busy and cannot be pressed again', () => {
    const p = props({ busy: true });
    render(p);
    expect(button('Sending…')!.disabled).toBe(true);
    keydown(area(), 'Enter');
    expect(p.onSend).not.toHaveBeenCalled();
  });

  it('Discard and Edit in Chat call their handlers', () => {
    const p = props();
    render(p);
    act(() => button('Discard')!.click());
    act(() => button('Edit in Chat')!.click());
    expect(p.onDiscard).toHaveBeenCalledTimes(1);
    expect(p.onEditInChat).toHaveBeenCalledTimes(1);
  });
});

describe('PointBoxApp', () => {
  /** A fake shell. Its own state follows Discard, Send and Edit in Chat: those end the session (no picks left). */
  function fakeApi() {
    let onStateCb: ((s: PointState) => void) | null = null;
    let shell: PointState = STATE;
    const ended = async () => {
      shell = { ...shell, picks: [], session: null };
    };
    return {
      state: vi.fn(async () => shell),
      send: vi.fn(async () => {
        await ended();
        return {};
      }),
      compose: vi.fn().mockResolvedValue('Please change this in the app: Make it green'),
      editInChat: vi.fn(async () => {
        await ended();
      }),
      arm: vi.fn().mockResolvedValue(undefined),
      removePick: vi.fn().mockResolvedValue({ ...STATE, picks: [] }),
      discard: vi.fn(async () => {
        await ended();
      }),
      hide: vi.fn().mockResolvedValue(undefined),
      openBlockedLink: vi.fn().mockResolvedValue(undefined),
      onState: vi.fn((cb: (s: PointState) => void) => {
        onStateCb = cb;
        return () => {};
      }),
      onDraft: vi.fn(() => () => {}),
      push: (s: PointState) => onStateCb?.(s),
    };
  }
  type Fake = ReturnType<typeof fakeApi>;

  async function renderApp(api: Fake) {
    await act(async () => {
      root.render(<PointBoxApp api={api as unknown as typeof pointApi} />);
    });
    await flush();
  }

  it('loads the state on open and picks the default pane', async () => {
    const api = fakeApi();
    await renderApp(api);
    expect(api.state).toHaveBeenCalled();
    expect(select().value).toBe('p1');
    expect(host.querySelector('.point-box-pick')).not.toBeNull();
  });

  it('a state push from the shell updates the picks', async () => {
    const api = fakeApi();
    await renderApp(api);
    act(() => api.push({ ...STATE, picks: [{ ...PICK, id: 'pick-9', label: 'h1' }] }));
    expect(host.querySelector('.point-box-tag')!.textContent).toBe('h1');
  });

  it('Enter sends the text to the chosen pane and clears the box', async () => {
    const api = fakeApi();
    await renderApp(api);
    act(() => {
      select().value = 'p2';
      select().dispatchEvent(new Event('change', { bubbles: true }));
    });
    typeInto(area(), '  Make it green  ');
    keydown(area(), 'Enter');
    await flush();
    expect(api.send).toHaveBeenCalledWith('p2', 'Make it green');
    expect(area().value).toBe('');
  });

  it('a refused send keeps the text and shows the sentence', async () => {
    const api = fakeApi();
    api.send.mockRejectedValue('Agent B is busy with a run.');
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Send')!.click());
    await flush();
    expect(host.querySelector('.point-box-error')!.textContent).toBe('Agent B is busy with a run.');
    expect(area().value).toBe('Make it green');
  });

  it('an empty request is not sent and says what to write', async () => {
    const api = fakeApi();
    await renderApp(api);
    act(() => button('Send')!.click());
    await flush();
    expect(api.send).not.toHaveBeenCalled();
    expect(host.querySelector('.point-box-error')!.textContent).toBe('Write what should change first.');
  });

  it('Copy composes the message and puts it on the clipboard', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Copy')!.click());
    await flush();
    expect(api.compose).toHaveBeenCalledWith('p1', 'Make it green');
    expect(writeText).toHaveBeenCalledWith('Please change this in the app: Make it green');
    expect(host.querySelector('.point-box-info')!.textContent).toBe('Copied.');
  });

  it('Discard clears the picks and the text', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Discard')!.click());
    await flush();
    expect(api.discard).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.point-box-pick')).toBeNull();
    expect(area().value).toBe('');
  });

  it('after Discard the next pick opens with an empty box', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Discard')!.click());
    await flush();
    act(() => api.push({ ...STATE, picks: [{ ...PICK, id: 'pick-7' }] }));
    await flush();
    expect(host.querySelector('.point-box-pick')).not.toBeNull();
    expect(area().value).toBe('');
    expect(text()).not.toContain('Make it green');
  });

  it('a failed Discard still clears the text, shows the error and reloads what the shell holds', async () => {
    const api = fakeApi();
    api.discard.mockRejectedValueOnce('Could not delete a screenshot.');
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Discard')!.click());
    await flush();
    expect(area().value).toBe('');
    expect(host.querySelector('.point-box-error')!.textContent).toBe('Could not delete a screenshot.');
    expect(host.querySelector('.point-box-pick')).not.toBeNull();
  });

  it('after a successful Send the next pick opens with an empty box', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    keydown(area(), 'Enter');
    await flush();
    expect(api.send).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.point-box-pick')).toBeNull();
    act(() => api.push({ ...STATE, picks: [{ ...PICK, id: 'pick-8' }] }));
    await flush();
    expect(host.querySelector('.point-box-pick')).not.toBeNull();
    expect(area().value).toBe('');
  });

  it('a new session id clears the text and the notes, and shows only the new picks', async () => {
    const api = fakeApi();
    await renderApp(api);
    act(() => api.push({ ...STATE, notice: 'Blocked a link to example.com. The preview only shows your app.' }));
    typeInto(area(), 'Make it green');
    expect(host.querySelector('.point-box-notice')).not.toBeNull();
    act(() => api.push({ ...STATE, session: 2, notice: null, picks: [{ ...PICK, id: 'pick-7' }] }));
    await flush();
    expect(area().value).toBe('');
    expect(host.querySelector('.point-box-notice')).toBeNull();
    expect(host.querySelectorAll('.point-box-pick')).toHaveLength(1);
  });

  it('the same session id keeps the text when another element is added', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => api.push({ ...STATE, picks: [PICK, { ...PICK, id: 'pick-2' }] }));
    await flush();
    expect(area().value).toBe('Make it green');
    expect(host.querySelectorAll('.point-box-pick')).toHaveLength(2);
  });

  it('a state with no session clears the text', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => api.push({ ...STATE, picks: [], session: null }));
    await flush();
    expect(area().value).toBe('');
  });

  it('Esc hides the box, the session later expires, and the next pick opens with an empty box', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    keydown(area(), 'Escape');
    await flush();
    expect(api.hide).toHaveBeenCalledTimes(1);
    act(() => api.push({ ...STATE, session: 3, picks: [{ ...PICK, id: 'pick-8' }] }));
    await flush();
    expect(area().value).toBe('');
  });

  it('after Edit in Chat the next pick opens with an empty box', async () => {
    const api = fakeApi();
    await renderApp(api);
    typeInto(area(), 'Make it green');
    act(() => button('Edit in Chat')!.click());
    await flush();
    act(() => api.push({ ...STATE, picks: [{ ...PICK, id: 'pick-9' }] }));
    await flush();
    expect(area().value).toBe('');
  });


  it('Esc calls hide on the window', async () => {
    const api = fakeApi();
    await renderApp(api);
    keydown(area(), 'Escape');
    await flush();
    expect(api.hide).toHaveBeenCalledTimes(1);
  });
});
