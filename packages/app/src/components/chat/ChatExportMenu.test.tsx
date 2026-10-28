// @vitest-environment jsdom
// "Save as page" and "Save as Markdown" in the chat's header menu: the menu offers both, the preview shows the page in
// a frame with no scripts (or the Markdown as plain text), the format switch shows and saves only the text of the
// format on it, a cancelled dialog saves nothing, and a failure says so.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { chatPageFileName } from '../../lib/chatExport';
import { GO_LIVE_EVENT } from '../../lib/live-room';
import { ChatExportMenu, type ChatExportApi } from './ChatExportMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// The format switch (SegTabs) measures itself with a ResizeObserver, which jsdom does not have.
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const TOKEN = 'one-time-token';
const PAGE = '<!doctype html><html><body><p>hello</p></body></html>';
const MD_TOKEN = 'markdown-token';
const MD = '# Test Agent\n\n**You**\n\nhello\n';
const PICKED = 'C:/Users/you/Downloads/chat.html';
const PICKED_MD = 'C:/Users/you/Downloads/chat.md';

let host: HTMLDivElement;
let root: Root;

function fakeApi(over: Partial<ChatExportApi> = {}): ChatExportApi {
  return {
    html: vi.fn(async () => ({ html: PAGE, token: TOKEN })),
    markdown: vi.fn(async () => ({ markdown: MD, token: MD_TOKEN })),
    save: vi.fn(async (_token: string, path: string) => path),
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

function tab(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((b) => b.textContent?.trim() === label);
  if (!found) throw new Error(`no tab named ${label}`);
  return found;
}

function render(api: ChatExportApi) {
  act(() => {
    root.render(<ChatExportMenu paneId="pane-1" api={api} />);
  });
}

async function openPreview(item = 'Save as page') {
  act(() => clickable('Chat menu').click());
  act(() => clickable(item).click());
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
  it('the chat menu offers Save as page, Save as Markdown and Go live, and nothing else', () => {
    render(fakeApi());
    expect(document.querySelector('[role="menu"]')).toBeNull();
    act(() => clickable('Chat menu').click());
    const items = [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map((el) => el.textContent?.trim());
    expect(items).toEqual(['Save as page', 'Save as Markdown', 'Go live…']);
    expect(clickable('Chat menu').getAttribute('aria-expanded')).toBe('true');
  });

  it('Go live… asks the cockpit to open the sheet with this pane, and closes the menu', () => {
    const seen: Array<{ paneId: string | null }> = [];
    const on = (e: Event) => seen.push((e as CustomEvent<{ paneId: string | null }>).detail);
    window.addEventListener(GO_LIVE_EVENT, on);
    render(fakeApi());
    act(() => clickable('Chat menu').click());
    act(() => clickable('Go live…').click());
    window.removeEventListener(GO_LIVE_EVENT, on);
    expect(seen).toEqual([{ paneId: 'pane-1' }]);
    expect(document.querySelector('[role="menu"]')).toBeNull();
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
    expect(api.markdown).not.toHaveBeenCalled();
  });

  it('saving writes the same page that the preview showed', async () => {
    const api = fakeApi();
    render(api);
    await openPreview();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.pickPath).toHaveBeenCalledWith(chatPageFileName(), 'page');
    expect(api.save).toHaveBeenCalledWith(TOKEN, PICKED);
    const status = document.querySelector('[role="status"]')?.textContent ?? '';
    expect(status).toContain(`Saved to ${PICKED}`);
    expect(status).toContain('Nothing was sent anywhere.');
  });

  it('sends the one-time token and the path, never the page, and gets a new page for the next save', async () => {
    const html = vi
      .fn()
      .mockResolvedValueOnce({ html: PAGE, token: 'first' })
      .mockResolvedValueOnce({ html: PAGE, token: 'second' })
      .mockResolvedValueOnce({ html: PAGE, token: 'third' });
    const api = fakeApi({ html });
    render(api);
    await openPreview();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.save).toHaveBeenLastCalledWith('first', PICKED);
    expect(html).toHaveBeenCalledTimes(2);
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.save).toHaveBeenLastCalledWith('second', PICKED);
    expect(html).toHaveBeenCalledTimes(3);
    for (const call of vi.mocked(api.save).mock.calls) expect(call).toHaveLength(2);
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

describe('ChatExportMenu format switch', () => {
  it('Save as Markdown opens the sheet on Markdown', async () => {
    const api = fakeApi();
    render(api);
    await openPreview('Save as Markdown');
    expect(tab('Markdown').getAttribute('aria-selected')).toBe('true');
    expect(tab('Page').getAttribute('aria-selected')).toBe('false');
    expect(api.markdown).toHaveBeenCalledWith('pane-1');
    expect(api.html).not.toHaveBeenCalled();
  });

  it('switching to Markdown shows the Markdown as plain text, with no frame', async () => {
    const api = fakeApi();
    render(api);
    await openPreview();
    act(() => tab('Markdown').click());
    await flush();
    expect(document.querySelector('iframe')).toBeNull();
    expect(document.querySelector('.ck-chat-export-text')?.textContent).toBe(MD);
    expect(clickable('Save as Markdown…')).not.toBeNull();
  });

  it('saving on Markdown saves the Markdown token as a .md file', async () => {
    const api = fakeApi({ pickPath: vi.fn(async () => PICKED_MD) });
    render(api);
    await openPreview();
    act(() => tab('Markdown').click());
    await flush();
    await act(async () => clickable('Save as Markdown…').click());
    await flush();
    expect(api.pickPath).toHaveBeenCalledWith(chatPageFileName(new Date(), 'markdown'), 'markdown');
    expect(api.save).toHaveBeenCalledWith(MD_TOKEN, PICKED_MD);
    expect(document.querySelector('[role="status"]')?.textContent).toContain(`Saved to ${PICKED_MD}`);
  });

  it('switching back to Page shows the page and saves with the page token, never the Markdown one', async () => {
    const api = fakeApi();
    render(api);
    await openPreview();
    act(() => tab('Markdown').click());
    await flush();
    act(() => tab('Page').click());
    await flush();
    expect(document.querySelector('iframe[title="Chat page preview"]')?.getAttribute('srcdoc')).toBe(PAGE);
    expect(document.querySelector('.ck-chat-export-text')).toBeNull();
    await act(async () => clickable('Save as page…').click());
    await flush();
    expect(api.save).toHaveBeenCalledWith(TOKEN, PICKED);
  });

  it('a Markdown that cannot be made says so and cannot be saved', async () => {
    const api = fakeApi({ markdown: vi.fn(async () => Promise.reject(new Error('Chat is not available for this pane.'))) });
    render(api);
    await openPreview();
    act(() => tab('Markdown').click());
    await flush();
    expect(document.body.textContent).toContain('Chat is not available for this pane.');
    expect(clickable('Save as Markdown…').disabled).toBe(true);
    expect(document.querySelector('.ck-chat-export-text')).toBeNull();
  });
});
