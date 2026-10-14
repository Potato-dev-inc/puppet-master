// @vitest-environment jsdom
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Sheet } from './Sheet';

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

const render = (el: ReactElement) => act(() => root.render(el));
const scrim = () => host.querySelector<HTMLElement>('.ck-scrim')!;
const sheet = () => host.querySelector<HTMLElement>('.ck-sheet')!;
const escape = () =>
  act(() => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  });
const tab = (shiftKey = false) => {
  const event = new KeyboardEvent('keydown', { key: 'Tab', shiftKey, bubbles: true, cancelable: true });
  act(() => {
    document.dispatchEvent(event);
  });
  return event;
};

describe('Sheet keyboard', () => {
  it('Escape closes the sheet', () => {
    const fn = vi.fn();
    render(
      <Sheet labelledBy="t" onClose={fn}>
        <h2 id="t">Hi</h2>
        <button>A</button>
        <button>B</button>
      </Sheet>,
    );
    escape();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('Escape does nothing without onClose (busy)', () => {
    render(
      <Sheet labelledBy="t">
        <h2 id="t">Hi</h2>
        <button>A</button>
      </Sheet>,
    );
    expect(() => escape()).not.toThrow();
    expect(host.querySelector('.ck-sheet')).not.toBeNull();
  });

  it('Tab stays inside the sheet: Tab on the last button wraps to the first', () => {
    render(
      <Sheet labelledBy="t">
        <h2 id="t">Hi</h2>
        <button>A</button>
        <button>B</button>
      </Sheet>,
    );
    const [first, last] = sheet().querySelectorAll<HTMLButtonElement>('button');
    last.focus();
    const event = tab(false);
    expect(event.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(first);
  });

  it('Shift+Tab on the first button wraps to the last', () => {
    render(
      <Sheet labelledBy="t">
        <h2 id="t">Hi</h2>
        <button>A</button>
        <button>B</button>
      </Sheet>,
    );
    const [first, last] = sheet().querySelectorAll<HTMLButtonElement>('button');
    first.focus();
    const event = tab(true);
    expect(event.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(last);
  });

  it('a plain Tab from the first button is not prevented', () => {
    render(
      <Sheet labelledBy="t">
        <h2 id="t">Hi</h2>
        <button>A</button>
        <button>B</button>
      </Sheet>,
    );
    const [first] = sheet().querySelectorAll<HTMLButtonElement>('button');
    first.focus();
    const event = tab(false);
    expect(event.defaultPrevented).toBe(false);
  });
});

describe('Sheet scrim', () => {
  it('clicking the scrim closes, clicking inside the sheet does not', () => {
    const fn = vi.fn();
    render(
      <Sheet labelledBy="t" onClose={fn}>
        <h2 id="t">Hi</h2>
        <button>A</button>
      </Sheet>,
    );
    act(() => {
      scrim().dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    });
    expect(fn).toHaveBeenCalledTimes(1);

    act(() => {
      sheet().querySelector('button')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    });
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

describe('Sheet role and aria', () => {
  it('defaults to role alertdialog, modal, labelled by labelledBy', () => {
    render(
      <Sheet labelledBy="t">
        <h2 id="t">Hi</h2>
      </Sheet>,
    );
    expect(sheet().getAttribute('role')).toBe('alertdialog');
    expect(sheet().getAttribute('aria-modal')).toBe('true');
    expect(sheet().getAttribute('aria-labelledby')).toBe('t');
  });

  it('role="dialog" overrides the default', () => {
    render(
      <Sheet labelledBy="title" role="dialog">
        <h2 id="title">Hi</h2>
      </Sheet>,
    );
    expect(sheet().getAttribute('role')).toBe('dialog');
    expect(sheet().getAttribute('aria-modal')).toBe('true');
    expect(sheet().getAttribute('aria-labelledby')).toBe('title');
  });
});

describe('Sheet focus restore', () => {
  it('returns focus to the element focused before it opened', () => {
    const opener = document.createElement('button');
    opener.textContent = 'Open';
    document.body.appendChild(opener);
    try {
      opener.focus();
      expect(document.activeElement).toBe(opener);
      render(
        <Sheet labelledBy="t" onClose={() => {}}>
          <h2 id="t">Hi</h2>
          <button>A</button>
        </Sheet>,
      );
      act(() => root.unmount());
      expect(document.activeElement).toBe(opener);
    } finally {
      opener.remove();
    }
  });
});
