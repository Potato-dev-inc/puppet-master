// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorktreeItem } from '../../lib/cockpit-api';
import type { PrPrepared, ShipApi } from '../../lib/ship-api';
import { CreatePrSheet } from './CreatePrSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ITEM = {
  handle: 'cart-1',
  branch: 'pm/cart-1',
  base: 'main',
  repo: 'D:/code/shop',
  path: 'D:/code/shop/.puppet-master/worktrees/cart-1-ab12',
  exists: true,
  state: 'unreviewed',
  summary: null,
} as WorktreeItem;

const READY: PrPrepared = {
  gh: 'ready',
  has_remote: true,
  on_github: true,
  default_branch: 'main',
  existing: null,
  title: 'Round cart totals to cents',
  body: '## What changed\n\nCodex changed 2 files.\n\n## Checks\n\n14/14 passed',
  base: 'main',
  branch: 'pm/cart-1',
  agent: 'Codex',
  has_uncommitted: true,
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

async function mount(prepared: Partial<PrPrepared> | Error, over: Partial<ShipApi> = {}) {
  const ship = {
    prPrepare: prepared instanceof Error ? vi.fn().mockRejectedValue(prepared) : vi.fn().mockResolvedValue({ ...READY, ...prepared }),
    prCreate: vi.fn().mockResolvedValue({ number: 42, url: 'https://github.com/o/r/pull/42', draft: true, base: 'main', created_at_ms: 1 }),
    ...over,
  } as unknown as ShipApi;
  const onDone = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(<CreatePrSheet item={ITEM} ship={ship} agentName="Codex" onClose={onClose} onDone={onDone} />);
  });
  return { ship, onDone, onClose };
}

describe('Create a draft pull request', () => {
  it('opens prefilled from the receipt and says what it will do', async () => {
    await mount({});
    expect((document.querySelector('input.ck-pr-input') as HTMLInputElement).value).toBe('Round cart totals to cents');
    expect((document.querySelector('textarea') as HTMLTextAreaElement).value).toContain('14/14 passed');
    const text = document.body.textContent ?? '';
    expect(text).toContain('pushes this copy');
    expect(text).toContain('pm/cart-1');
    expect(text).toContain("never overwrites a branch");
    expect(text).toContain("It saves Codex's changes as one commit first.");
  });

  it('creates the draft with what is in the fields', async () => {
    const { ship, onDone } = await mount({});
    const title = document.querySelector('input.ck-pr-input') as HTMLInputElement;
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      set?.call(title, 'Cents, please');
      title.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => button('Create draft PR')?.click());
    expect(ship.prCreate).toHaveBeenCalledWith('D:/code/shop', 'cart-1', 'Cents, please', READY.body, 'main');
    expect(onDone).toHaveBeenCalledWith(expect.objectContaining({ number: 42 }), false);
  });

  it('cannot be created without a title', async () => {
    await mount({ title: '' });
    expect(button('Create draft PR')?.disabled).toBe(true);
  });

  it('shows the error from GitHub and stays open', async () => {
    const { onDone } = await mount({}, { prCreate: vi.fn().mockRejectedValue(new Error('PUSH_FAILED: GitHub already has a different pm/cart-1 branch')) });
    await act(async () => button('Create draft PR')?.click());
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('already has a different');
    expect(onDone).not.toHaveBeenCalled();
    expect(button('Create draft PR')?.disabled).toBe(false);
  });

  it('becomes "Update PR" for a branch that already has one', async () => {
    const existing = { number: 7, url: 'u', draft: true, base: 'main', created_at_ms: 1 };
    const { ship, onDone } = await mount({ existing });
    expect(document.body.textContent).toContain('Update pull request #7');
    expect(document.querySelector('input.ck-pr-input')).toBeNull();
    await act(async () => button('Update PR')?.click());
    expect(ship.prCreate).toHaveBeenCalled();
    expect(onDone).toHaveBeenCalledWith(expect.anything(), true);
  });
});

describe('when Create PR cannot run', () => {
  it('explains a missing GitHub command line and offers the command to copy', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    await mount({ gh: 'missing' });
    expect(document.body.textContent).toContain("Create PR uses the GitHub command line, and it isn't installed on this PC.");
    expect(button('Create draft PR')).toBeUndefined();
    expect(document.querySelector('.ck-pr-command')?.textContent).toContain('git push --set-upstream origin pm/cart-1');
    expect(document.querySelector('.ck-pr-command')?.textContent).toContain('gh pr create --draft');
    await act(async () => button('Copy the command')?.click());
    expect(write).toHaveBeenCalledWith(expect.stringContaining('git push --set-upstream origin pm/cart-1'));
    expect(button('Copied')).toBeTruthy();
  });

  it('asks to sign in first', async () => {
    await mount({ gh: 'signed_out' });
    expect(document.body.textContent).toContain('Sign in to GitHub first');
    expect(document.querySelector('.ck-pr-command')?.textContent).toBe('gh auth login');
  });

  it('says when the project is not on GitHub', async () => {
    await mount({ on_github: false });
    expect(document.body.textContent).toContain("This project isn't on GitHub. Merge into the main branch instead");
    expect(button('Create draft PR')).toBeUndefined();
    expect(document.querySelector('.ck-pr-command')).toBeNull();
  });

  it('with no GitHub remote at all, says so in plain words and offers the exact commands to copy for later', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    await mount({ has_remote: false, on_github: false });
    const text = document.body.textContent ?? '';
    expect(text).toContain('no GitHub remote');
    expect(text).toContain('git remote add origin');
    expect(button('Create draft PR')).toBeUndefined();
    const commands = document.querySelector('.ck-pr-command')?.textContent ?? '';
    expect(commands.split('\n')).toEqual([
      'git push --set-upstream origin pm/cart-1',
      'gh pr create --draft --base main --head pm/cart-1 --title "Round cart totals to cents"',
    ]);
    await act(async () => button('Copy the command')?.click());
    expect(write).toHaveBeenCalledWith(commands);
  });

  it('shows a lookup error', async () => {
    await mount(new Error('WORKTREE_NOT_FOUND: no worktree for cart-1'));
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('no worktree');
  });
});
