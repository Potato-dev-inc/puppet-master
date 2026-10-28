// @vitest-environment jsdom
// "Save as page" in the chat's header menu: the menu offers it, the preview shows the page in a frame with no scripts
// and the same text that gets saved, a cancelled dialog saves nothing, and a failure says so.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { chatPageFileName } from '../../lib/chatExport';
import { ChatExportMenu, type ChatExportApi } from './ChatExportMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PAGE = '<!doctype html><html><body><p>hello</p></body></html>';
const PICKED = 'C:\Users\you\Downloads\chat.html';

let host: HTMLDivElement;
let root: Root;

function fakeApi(over: Partial<ChatExportApi> = {}): ChatExportApi {
  return {
    html: vi.fn(async () => PAGE),
    save: vi.fn(async (path: string) => path),
    pickPath: vi.fn(async () => PICKED),
    ...over,
  };
}

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

function clickable(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label);
  if (!found) throw new Error(`no button named ${label}`);
  return found;
}

function render(api: ChatExportApi) {
  act(() => {
    root.render(<ChatExportMenu paneId="pane-1" api={api} />);
  });
}

async function openPreview() {
  act(() => clickable('Chat menu').click());
  act(() => clickable('Save as page').click());
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('ChatExportMenu', () => {
  it('the chat menu offers Save as page and nothing else', () => {
    render(fakeApi());
    expect(document.querySelector('[role="menu"]')).toBeNull();
    act(() => clickable('Chat menu').click());
    const items = [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map((el) => el.textContent?.trim());
    expect(items).toEqual(['Save as page']);
    expect(clickable('Chat menu').getAttribute('aria-expanded')).toBe('true');
  });

  it('Escape closes the menu', () => {
    render(fakeApi());
    act(() => clickable('Chat menu').click());
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    act(() => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it('the preview is the page in a frame with no scripts', async () => {
    const api = fakeApi();
    render(api);
    await openPreview();
    const frame = document.querySelector('iframe[title="Chat page preview"]');
    expect(frame).not.toBeNull();
    expect(frame?.getAttribute('sandbox')).toBe('');
    expect(frame?.getAttribute('srcdoc')).toBe(PAGE);
    expect(api.html).toHaveBeenCalledWith('pane-1');
  });

  it('saving writes the same page that the preview showed', async () => {
    const api = fakeApi();
    render(api);
    await openPreview();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.pickPath).toHaveBeenCalledWith(chatPageFileName());
    expect(api.save).toHaveBeenCalledWith(PICKED, PAGE);
    const status = document.querySelector('[role="status"]')?.textContent ?? '';
    expect(status).toContain(`Saved to ${PICKED}`);
    expect(status).toContain('Nothing was sent anywhere.');
  });

  it('a cancelled dialog saves nothing and keeps the preview open', async () => {
    const api = fakeApi({ pickPath: vi.fn(async () => null) });
    render(api);
    await openPreview();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.save).not.toHaveBeenCalled();
    expect(document.querySelector('iframe[title="Chat page preview"]')).not.toBeNull();
  });

  it('a failed save says why', async () => {
    const api = fakeApi({ save: vi.fn(async () => Promise.reject(new Error('disk is full'))) });
    render(api);
    await openPreview();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe('Not saved: disk is full');
  });

  it('a page that cannot be made says so and cannot be saved', async () => {
    const api = fakeApi({ html: vi.fn(async () => Promise.reject(new Error('Chat is not available for this pane.'))) });
    render(api);
    await openPreview();
    expect(document.body.textContent).toContain('Chat is not available for this pane.');
    expect(clickable('Save as page…').disabled).toBe(true);
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('Close hides the preview', async () => {
    render(fakeApi());
    await openPreview();
    expect(document.querySelector('iframe')).not.toBeNull();
    act(() => clickable('Close').click());
    expect(document.querySelector('iframe')).toBeNull();
  });
});
