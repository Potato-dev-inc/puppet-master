// @vitest-environment jsdom
// The Copy button on a fenced code block: it copies the code exactly (no language tag, no extra newline),
// says "Copied" for 1.5 s, and stays "Copy" when the clipboard says no.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatMarkdown } from './ChatMarkdown';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

function render(text: string): void {
  act(() => {
    root.render(<ChatMarkdown text={text} />);
  });
}

const copyButton = (): HTMLButtonElement => host.querySelector('button.ck-chat-md-copy') as HTMLButtonElement;

function click(el: HTMLElement): void {
  act(() => {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
}

/** Lets the awaited clipboard call finish. */
async function settle(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('ChatMarkdown code block Copy', () => {
  it('copies the exact code: no language tag, the trailing space kept, no extra newline', async () => {
    render('```sh\nnpm ci \nnpm test\n```');
    click(copyButton());
    await settle();
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith('npm ci \nnpm test');
  });

  it('shows Copied after a copy, then goes back to Copy after 1.5 s', async () => {
    vi.useFakeTimers();
    render('```\none\ntwo\n```');
    click(copyButton());
    await settle();
    expect(copyButton().textContent).toBe('Copied');
    expect(copyButton().dataset.copied).toBe('true');

    act(() => {
      vi.advanceTimersByTime(1499);
    });
    expect(copyButton().textContent).toBe('Copied');

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(copyButton().textContent).toBe('Copy');
    expect(copyButton().dataset.copied).toBeUndefined();
  });

  it('keeps Copy when the clipboard refuses', async () => {
    writeText.mockRejectedValueOnce(new Error('denied'));
    render('```\nkeep me\n```');
    click(copyButton());
    await settle();
    expect(copyButton().textContent).toBe('Copy');
    expect(copyButton().dataset.copied).toBeUndefined();
  });

  it('gives every fenced block its own button, and no button on text or inline code', () => {
    render('Run `npm ci` first.\n\n```\nfirst\n```\n\n```ts\nsecond\n```');
    const buttons = host.querySelectorAll('button.ck-chat-md-copy');
    expect(buttons).toHaveLength(2);
    expect(host.querySelectorAll('pre')).toHaveLength(2);
    expect(host.querySelectorAll('pre')[1]?.dataset.lang).toBe('ts');
  });

  it('copies the second block only when its own button is pressed', async () => {
    render('```\nfirst\n```\n\n```\nsecond\n```');
    click(host.querySelectorAll('button.ck-chat-md-copy')[1] as HTMLElement);
    await settle();
    expect(writeText).toHaveBeenCalledWith('second');
  });
});
