// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  LOOKALIKE_PROMPT,
  LOOKALIKE_VIEW,
  MANY_LIST,
  MANY_PROMPT,
  MANY_VIEW,
  OFF_VIEW,
  PLAIN_PROMPT,
  PLAIN_VIEW,
} from '../../dev/package-check-fixtures';
import { packageCheckLookup, type PackageCheckView } from '../../lib/package-check';
import { tauri } from '../../lib/tauri';
import { PackageFacts, type PackagePrompt } from './PackageFacts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock('../../lib/package-check', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/package-check')>()),
  packageCheckLookup: vi.fn(),
}));
vi.mock('../../lib/tauri', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../lib/tauri')>();
  return { ...real, tauri: { ...real.tauri, openExternalUrl: vi.fn(() => Promise.resolve()) } };
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.mocked(packageCheckLookup).mockReset();
  vi.mocked(tauri.openExternalUrl).mockClear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function mount(props: { paneId?: string; prompt: PackagePrompt | null | undefined; details?: boolean; variant?: 'card' | 'bar' }) {
  act(() => {
    root.render(<PackageFacts {...props} />);
  });
}

/** Lets resolved promises and their state updates run. */
async function flush() {
  await act(async () => {
    await new Promise((done) => setTimeout(done, 0));
  });
}

function deferred<T>() {
  let answer!: (value: T) => void;
  let fail!: (reason: unknown) => void;
  const promise = new Promise<T>((ok, bad) => {
    answer = ok;
    fail = bad;
  });
  return { promise, answer, fail };
}

const rowCount = () => host.querySelectorAll('.ck-pc-rows > .ck-pc-row').length;
const text = (selector: string) => host.querySelector(selector)?.textContent;
const buttonNamed = (name: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === name);

describe('PackageFacts first paint', () => {
  it('paints one row per package before the registry answers', () => {
    vi.mocked(packageCheckLookup).mockReturnValue(new Promise(() => undefined));
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT });
    expect(rowCount()).toBe(1);
    const row = host.querySelector('.ck-pc-row');
    expect(row?.getAttribute('data-state')).toBe('checking');
    expect(row?.querySelector('.ck-pc-name')?.textContent).toBe('lodahs');
    expect(row?.querySelector('.ck-pc-summary')?.textContent).toBe('Looks like lodash (2 letters swapped) · Checking…');
    expect(text('.ck-pc-status')).toBe('Look closer · 1 of 1');
    expect(packageCheckLookup).toHaveBeenCalledTimes(1);
    expect(packageCheckLookup).toHaveBeenCalledWith('pane-1', 'prompt-1', false);
  });

  it('says which registries it is checking while it waits, when nothing is a look-alike', () => {
    vi.mocked(packageCheckLookup).mockReturnValue(new Promise(() => undefined));
    mount({ paneId: 'pane-1', prompt: PLAIN_PROMPT });
    expect(text('.ck-pc-status')).toBe('Checking npm…');
  });

  it('renders nothing without packages', () => {
    mount({ paneId: 'pane-1', prompt: { promptId: 'prompt-x', packages: undefined } });
    expect(host.innerHTML).toBe('');
  });
});

describe('PackageFacts answer', () => {
  it('fills the same rows in place: same count, same classes, the facts in the same row', async () => {
    const answer = deferred<PackageCheckView>();
    vi.mocked(packageCheckLookup).mockReturnValue(answer.promise);
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT });
    const before = Array.from(host.querySelectorAll('.ck-pc-row')).map((r) => r.className);
    await act(async () => {
      answer.answer(LOOKALIKE_VIEW);
      await answer.promise;
    });
    const after = Array.from(host.querySelectorAll('.ck-pc-row')).map((r) => r.className);
    expect(after).toEqual(before);
    expect(rowCount()).toBe(1);
    expect(host.querySelector('.ck-pc-row')?.getAttribute('data-state')).toBe('checked');
    expect(text('.ck-pc-summary')).toBe('Looks like lodash (2 letters swapped) · New: first published 3 days ago · Few downloads: 12 a week');
  });

  it('keeps the first-paint rows and says Could not check when the command fails', async () => {
    vi.mocked(packageCheckLookup).mockRejectedValue(new Error('unreachable'));
    mount({ paneId: 'pane-1', prompt: PLAIN_PROMPT });
    await flush();
    expect(text('.ck-pc-status')).toBe('Could not check');
    expect(rowCount()).toBe(1);
    expect(host.querySelector('.ck-pc-row')?.getAttribute('data-state')).toBe('failed');
    expect(text('.ck-pc-summary')).toBe('Not checked');
    expect(text('.ck-pc-name')).toBe('mapforge-kit');
  });

  it('shows at most three rows on the card and the rest as a line to Details', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(MANY_VIEW);
    mount({ paneId: 'pane-1', prompt: MANY_PROMPT });
    await flush();
    expect(rowCount()).toBe(3);
    expect(text('.ck-pc-more')).toBe('+4 more in Details');
  });

  it('has no Look closer glyph on a plain row', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(PLAIN_VIEW);
    mount({ paneId: 'pane-1', prompt: PLAIN_PROMPT });
    await flush();
    expect(host.querySelectorAll('[aria-label="Look closer"]')).toHaveLength(0);
    expect(text('.ck-pc-status')).toBe('Checked just now');
  });

  it('marks a look-alike row with a glyph labelled Look closer', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(LOOKALIKE_VIEW);
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT });
    await flush();
    expect(host.querySelector('.ck-pc-row [aria-label="Look closer"]')).not.toBeNull();
    expect(host.querySelector('.ck-pc-row')?.getAttribute('data-look')).toBe('');
  });

  it('shows the not-checked wording when lookups are off, and keeps the look-alike', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(OFF_VIEW);
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT });
    await flush();
    expect(host.querySelector('.ck-pc-row')?.getAttribute('data-state')).toBe('not_checked');
    expect(text('.ck-pc-summary')).toBe('Looks like lodash (2 letters swapped) · Not checked: package lookups are off');
    expect(host.querySelector('.ck-pc-row [aria-label="Look closer"]')).not.toBeNull();
  });

  it('makes one lookup per prompt, not one per render', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(PLAIN_VIEW);
    mount({ paneId: 'pane-1', prompt: PLAIN_PROMPT });
    await flush();
    mount({ paneId: 'pane-1', prompt: { ...PLAIN_PROMPT } });
    await flush();
    expect(packageCheckLookup).toHaveBeenCalledTimes(1);
  });

  it('looks up nothing without a pane, and shows no Checking text', () => {
    mount({ prompt: LOOKALIKE_PROMPT });
    expect(packageCheckLookup).not.toHaveBeenCalled();
    expect(host.querySelector('.ck-pc-row')?.getAttribute('data-state')).toBe('not_checked');
    expect(text('.ck-pc-summary')).toBe('Looks like lodash (2 letters swapped)');
  });
});

describe('PackageFacts details', () => {
  it('shows Check again only when the answer says a row can be retried, and asks again', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(MANY_VIEW);
    mount({ paneId: 'pane-1', prompt: MANY_PROMPT, details: true });
    await flush();
    const again = buttonNamed('Check again');
    expect(again).toBeDefined();
    await act(async () => {
      again?.click();
    });
    expect(packageCheckLookup).toHaveBeenLastCalledWith('pane-1', 'prompt-1', true);
  });

  it('has no Check again when every row settled', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(LOOKALIKE_VIEW);
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT, details: true });
    await flush();
    expect(buttonNamed('Check again')).toBeUndefined();
  });

  it('lists every package with its facts, the footer lines, and the outside counts', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(MANY_VIEW);
    mount({ paneId: 'pane-1', prompt: MANY_PROMPT, details: true });
    await flush();
    expect(host.querySelectorAll('.ck-pc-detail')).toHaveLength(7);
    const footer = Array.from(host.querySelectorAll('.ck-pc-foot')).map((p) => p.textContent);
    expect(footer).toEqual([
      'Facts from the public npm registry. Only the package names were sent. Puppet Master never answers for you.',
      '2 more packages were not checked',
      '1 name looked like a secret, so it was not shown or checked',
    ]);
  });

  it('opens the registry page for a package through the desktop shell', async () => {
    vi.mocked(packageCheckLookup).mockResolvedValue(LOOKALIKE_VIEW);
    mount({ paneId: 'pane-1', prompt: LOOKALIKE_PROMPT, details: true });
    await flush();
    await act(async () => {
      buttonNamed('Open on npm')?.click();
    });
    expect(tauri.openExternalUrl).toHaveBeenCalledWith('https://www.npmjs.com/package/lodahs');
  });

  it('shows a secret-only command as a footer line, with no rows', () => {
    mount({ prompt: { promptId: 'prompt-x', packages: { items: [], more: 0, hidden: 2 } }, details: true });
    expect(rowCount()).toBe(0);
    expect(text('.ck-pc-foot')).toBe('2 names looked like a secret, so they were not shown or checked');
  });
});

describe('PackageFacts layout and wording', () => {
  it('keeps each row to one fixed-height line in the stylesheet', () => {
    const css = readFileSync(join(__dirname, '../../styles/package-facts.css'), 'utf8');
    expect(css).toMatch(/\.cockpit \.ck-pc-row \{[^}]*height: 20px/);
    expect(css).toMatch(/\.cockpit \.ck-pc-summary \{[^}]*white-space: nowrap/);
    expect(css).toMatch(/\.cockpit \.ck-pc-summary \{[^}]*text-overflow: ellipsis/);
  });

  it('uses the bar class in the compact variant', () => {
    vi.mocked(packageCheckLookup).mockReturnValue(new Promise(() => undefined));
    mount({ prompt: PLAIN_PROMPT, variant: 'bar' });
    expect(host.querySelector('.ck-pc.ck-pc--bar')).not.toBeNull();
  });

  it('never uses a reassuring word, in what it shows or in its source', async () => {
    const forbidden = /\b(safe|verified|trusted|malicious|malware|approved|secure|clean)\b|\bOK\b/i;
    vi.mocked(packageCheckLookup).mockResolvedValue(MANY_VIEW);
    mount({ paneId: 'pane-1', prompt: MANY_PROMPT, details: true });
    await flush();
    expect(host.textContent).not.toMatch(forbidden);
    const source = readFileSync(join(__dirname, 'PackageFacts.tsx'), 'utf8');
    expect(source).not.toMatch(forbidden);
  });
});
