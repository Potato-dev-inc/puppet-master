// @vitest-environment jsdom
// The chat menu offers "Make clip of last turn" only when the panel gives it a handler.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatExportMenu, type ChatExportApi } from './ChatExportMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;

const api: ChatExportApi = {
  html: vi.fn(async () => ({ html: '<p>x</p>', token: 't' })),
  markdown: vi.fn(async () => ({ markdown: 'x', token: 't' })),
  save: vi.fn(async (_t: string, path: string) => path),
  pickPath: vi.fn(async () => null),
};

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const openMenu = () => act(() => (document.querySelector('[aria-label="Chat menu"]') as HTMLButtonElement).click());
const items = () => [...document.querySelectorAll('[role="menuitem"]')].map((n) => n.textContent);

describe('ChatExportMenu clip item', () => {
  it('is not in the menu without a handler', () => {
    act(() => root.render(<ChatExportMenu paneId="p" api={api} />));
    openMenu();
    expect(items()).toEqual(['Save as page', 'Save as Markdown']);
  });

  it('is in the menu with a handler, calls it once and closes the menu', () => {
    const onMakeClip = vi.fn();
    act(() => root.render(<ChatExportMenu paneId="p" api={api} onMakeClip={onMakeClip} />));
    openMenu();
    expect(items()).toEqual(['Save as page', 'Save as Markdown', 'Make clip of last turn']);
    act(() => (document.querySelector('[role="menuitem"]:last-child') as HTMLButtonElement).click());
    expect(onMakeClip).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});
