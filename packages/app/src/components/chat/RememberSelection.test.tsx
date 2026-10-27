// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RememberApi } from '../../lib/chat-remember';
import { REMEMBER_SAVED, RememberSelection } from './RememberSelection';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has no layout: a selection's box is empty here.
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
}

let host: HTMLDivElement;
let root: Root;
let body: HTMLDivElement | null = null;
let message: HTMLDivElement;

const VIEW = { project: 'C:\\work\\shop', memory: '- Use tabs.', global_enabled: true, project_enabled: true, targets: [], report: null };

function fakeApi(over: Partial<RememberApi> = {}): RememberApi {
  return {
    remember: vi.fn().mockResolvedValue({ text: 'Use tabs, not spaces.', added: true, view: VIEW }),
    forget: vi.fn().mockResolvedValue(VIEW),
    setProjectEnabled: vi.fn().mockResolvedValue(VIEW),
    ...over,
  };
}

/** Selects the whole agent message and tells the page the selection changed. */
function selectMessage() {
  const sel = window.getSelection()!;
  const range = document.createRange();
  range.selectNodeContents(message);
  sel.removeAllRanges();
  sel.addRange(range);
  act(() => {
    document.dispatchEvent(new Event('selectionchange'));
  });
}

function pill(): HTMLButtonElement | null {
  return document.querySelector('[data-testid="remember-pill"]');
}

function toast(): HTMLElement | null {
  return document.querySelector('[data-testid="remember-toast"]');
}

function button(scope: HTMLElement | null, label: string): HTMLButtonElement | undefined {
  return Array.from(scope?.querySelectorAll('button') ?? []).find((b) => b.textContent === label);
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function mount(api: RememberApi) {
  body = document.createElement('div');
  body.className = 'ck-chat-body';
  message = document.createElement('div');
  message.className = 'ck-chat-text';
  message.textContent = 'Use tabs, not spaces.';
  body.appendChild(message);
  document.body.appendChild(body);
  const ref = { current: body };
  act(() => {
    root.render(<RememberSelection paneId="p1" root={ref} api={api} />);
  });
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  body?.remove();
  body = null;
  window.getSelection()?.removeAllRanges();
});

describe('RememberSelection', () => {
  it('shows the pill over selected message text, and Escape hides it', () => {
    mount(fakeApi());
    expect(pill()).toBeNull();
    selectMessage();
    expect(pill()?.textContent).toBe('Remember for all agents');
    act(() => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(pill()).toBeNull();
  });

  it('remembers the text on click, says so, and Undo removes that line', async () => {
    const api = fakeApi();
    mount(api);
    selectMessage();
    act(() => pill()!.click());
    await flush();
    expect(api.remember).toHaveBeenCalledWith('p1', 'Use tabs, not spaces.');
    expect(toast()?.textContent).toContain(REMEMBER_SAVED);
    expect(pill()).toBeNull();
    act(() => button(toast()!, 'Undo')!.click());
    await flush();
    expect(api.forget).toHaveBeenCalledWith('p1', 'Use tabs, not spaces.');
    expect(toast()?.textContent).toContain('Removed from the project memory.');
  });

  it('remembers with Ctrl+Shift+M while the text is selected', async () => {
    const api = fakeApi();
    mount(api);
    selectMessage();
    act(() => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'M', ctrlKey: true, shiftKey: true }));
    });
    await flush();
    expect(api.remember).toHaveBeenCalledTimes(1);
  });

  it('offers Turn on when the memory is off for the project, then remembers again', async () => {
    const api = fakeApi({
      remember: vi
        .fn()
        .mockRejectedValueOnce(new Error('MEMORY_OFF_PROJECT: Shared memory is off for this project.'))
        .mockResolvedValue({ text: 'Use tabs, not spaces.', added: true, view: VIEW }),
    });
    mount(api);
    selectMessage();
    act(() => pill()!.click());
    await flush();
    expect(toast()?.textContent).toContain('Shared memory is off for this project.');
    act(() => button(toast()!, 'Turn on')!.click());
    await flush();
    expect(api.setProjectEnabled).toHaveBeenCalledWith('p1', true);
    expect(api.remember).toHaveBeenCalledTimes(2);
  });

  it('explains a refusal in plain words, without the code', async () => {
    const api = fakeApi({
      remember: vi.fn().mockRejectedValue(new Error('SECRET: That text looks like a secret, so it was not saved.')),
    });
    mount(api);
    selectMessage();
    act(() => pill()!.click());
    await flush();
    expect(toast()?.textContent).toBe('That text looks like a secret, so it was not saved.');
  });
});
