// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BriefSheet, briefSize } from './BriefSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.useFakeTimers({ shouldAdvanceTime: true });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

async function mount(props: Partial<Parameters<typeof BriefSheet>[0]> = {}) {
  const onClose = vi.fn();
  await act(async () => {
    root.render(<BriefSheet agent="Fake B · Work" text="Continue: fix the total" onClose={onClose} {...props} />);
  });
  return { onClose };
}
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

describe('briefSize', () => {
  it('uses bytes under 1 KB and KB with one decimal above', () => {
    expect(briefSize('')).toBe('0 B');
    expect(briefSize('x'.repeat(1023))).toBe('1023 B');
    expect(briefSize('x'.repeat(3174))).toBe('3.1 KB');
  });

  it('counts UTF-8 bytes, not characters', () => {
    // "é" is 2 bytes in UTF-8: 512 of them is 1024 bytes, which is exactly 1.0 KB.
    expect(briefSize('é'.repeat(512))).toBe('1.0 KB');
  });
});

describe('BriefSheet', () => {
  it('titles the sheet with the new agent name and shows the brief read-only', async () => {
    await mount();
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog).toBeTruthy();
    expect(dialog!.getAttribute('aria-labelledby')).toBeTruthy();
    expect(host.querySelector('.ck-title')!.textContent).toBe('The brief Fake B · Work got');
    const pre = host.querySelector('pre')!;
    expect(pre.textContent).toBe('Continue: fix the total');
    expect(pre.getAttribute('tabindex')).toBe('0');
    expect(host.querySelector('textarea, input')).toBeNull();
  });

  it('shows the size alone when the trimmed list is not given', async () => {
    await mount({ text: 'x'.repeat(3174) });
    expect(host.querySelector('.ck-sub')!.textContent).toBe('3.1 KB');
  });

  it('says trimmed: none when the list is empty, and names the parts when not', async () => {
    await mount({ text: 'x'.repeat(100), trimmed: [] });
    expect(host.querySelector('.ck-sub')!.textContent).toBe('100 B · trimmed: none');
    await act(async () => {
      root.render(<BriefSheet agent="Fake B" text="x" trimmed={['files', 'last_message']} onClose={() => {}} />);
    });
    expect(host.querySelector('.ck-sub')!.textContent).toBe('1 B · trimmed: files, last_message');
  });

  it('shows a loading line and no copy text while the brief is still loading', async () => {
    await mount({ text: null });
    expect(host.textContent).toContain('Loading the brief…');
    expect(host.querySelector('pre')).toBeNull();
    expect(button('Copy')!.disabled).toBe(true);
    expect(host.querySelector('.ck-sub')!.textContent).not.toMatch(/KB|B ·/);
  });

  it('shows a plain-English error with role alert and no brief text', async () => {
    await mount({ text: null, error: 'No brief is stored for this hand-over.' });
    const alert = host.querySelector('[role="alert"]');
    expect(alert!.textContent).toBe('No brief is stored for this hand-over.');
    expect(host.querySelector('pre')).toBeNull();
  });

  it('Copy writes the exact brief to the clipboard and says Copied', async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await mount({ text: 'Continue: a\nsecond line' });
    await act(async () => button('Copy')!.click());
    expect(writeText).toHaveBeenCalledWith('Continue: a\nsecond line');
    expect(button('Copied')).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(1600);
    });
    expect(button('Copy')).toBeTruthy();
  });

  it('says what to do when the clipboard refuses, without the raw error', async () => {
    const writeText = vi.fn(async () => {
      throw new Error('NotAllowedError: denied');
    });
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await mount();
    await act(async () => button('Copy')!.click());
    const alert = host.querySelector('[role="alert"]');
    expect(alert!.textContent).toBe('Could not copy. Select the text and copy it.');
    expect(host.textContent).not.toContain('NotAllowedError');
  });

  it('Done closes the sheet', async () => {
    const { onClose } = await mount();
    await act(async () => button('Done')!.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('Escape closes the sheet', async () => {
    const { onClose } = await mount();
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
