// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChecksReport, PrRef, ShipApi } from '../../lib/ship-api';
import { MAX_POLLS, PrBlock } from './PrBlock';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const report = (state: ChecksReport['state'], names: string[] = []): ChecksReport => ({
  state,
  checked_at_ms: 1,
  items: names.map((name) => ({ name, state: state === 'failing' ? 'fail' : state === 'pending' ? 'pending' : 'pass' })),
});
const PR = (checks?: ChecksReport): PrRef => ({ number: 42, url: 'https://github.com/o/r/pull/42', draft: true, base: 'main', created_at_ms: 1, checks });

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
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

function api(over: Partial<ShipApi> = {}): ShipApi {
  return {
    prChecks: vi.fn().mockResolvedValue(PR(report('passing', ['a']))),
    prSendBackPreview: vi.fn().mockResolvedValue({ text: 'GitHub checks failed on pull request #42 of this project. The log is in .puppet-master/ci-42-sendback.md.', failing: ['lint'], excerpt_lines: 40 }),
    prSendBack: vi.fn().mockResolvedValue({ route: 'run', target: 'cart-1' }),
    openUrl: vi.fn().mockResolvedValue(undefined),
    ...over,
  } as unknown as ShipApi;
}

async function mount(pr: PrRef, ship: ShipApi, extra: { onPr?: (p: PrRef) => void } = {}) {
  const onPr = extra.onPr ?? vi.fn();
  await act(async () => {
    root.render(<PrBlock repo="D:/code/shop" handle="cart-1" pr={pr} agentName="Codex" ship={ship} pollEveryMs={0} waitSecs={0} onPr={onPr} />);
  });
  await settle();
  return { onPr };
}

describe('the pull request block', () => {
  it('shows the link and the checks that are running', async () => {
    const ship = api({ prChecks: vi.fn().mockResolvedValue(PR(report('pending', ['a', 'b']))) });
    await mount(PR(report('pending', ['a', 'b'])), ship);
    expect(document.body.textContent).toContain('Draft PR #42');
    expect(document.body.textContent).toContain('GitHub checks are running (2 left)');
    await act(async () => button('Open on GitHub')?.click());
    expect(ship.openUrl).toHaveBeenCalledWith('https://github.com/o/r/pull/42');
  });

  it('polls a bounded number of times while checks stay pending, then stops', async () => {
    const ship = api({ prChecks: vi.fn().mockResolvedValue(PR(report('pending', ['a']))) });
    await mount(PR(), ship);
    // wait for the loop to run out
    await vi.waitFor(() => expect(document.body.textContent).toContain('Still running on GitHub'), { timeout: 3000 });
    expect((ship.prChecks as ReturnType<typeof vi.fn>).mock.calls.length).toBe(MAX_POLLS);
    expect(document.body.textContent).toContain('Still running on GitHub. Check again later.');
  });

  it('stops polling as soon as the checks settle', async () => {
    const ship = api();
    const { onPr } = await mount(PR(), ship);
    expect((ship.prChecks as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1);
    expect(onPr).toHaveBeenCalledWith(expect.objectContaining({ checks: expect.objectContaining({ state: 'passing' }) }));
  });

  it('does not look again when the result is already known', async () => {
    const ship = api();
    await mount(PR(report('passing', ['a', 'b'])), ship);
    expect(ship.prChecks).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('All 2 GitHub checks passed');
    expect(button('Send back')).toBeUndefined();
  });

  it('names the failing checks and offers Send back, never sending by itself', async () => {
    const ship = api();
    await mount(PR(report('failing', ['test (ubuntu)', 'lint'])), ship);
    expect(document.body.textContent).toContain('2 GitHub checks failed: test (ubuntu), lint');
    expect(button('Send back to Codex')).toBeTruthy();
    expect(ship.prSendBack).not.toHaveBeenCalled();
    expect(ship.prSendBackPreview).not.toHaveBeenCalled();
  });

  it('asks first: shows what the agent gets, and Cancel sends nothing', async () => {
    const ship = api();
    await mount(PR(report('failing', ['lint'])), ship);
    await act(async () => button('Send back to Codex')?.click());
    const sheet = document.querySelector('[role="dialog"]');
    expect(sheet?.textContent).toContain('Send back to Codex?');
    expect(sheet?.textContent).toContain('secrets hidden');
    expect(sheet?.textContent).toContain('not as instructions');
    expect(sheet?.textContent).toContain('Failing: lint');
    expect(document.querySelector('.ck-pr-command')?.textContent).toContain('.puppet-master/ci-42-sendback.md');
    await act(async () => button('Cancel')?.click());
    expect(ship.prSendBack).not.toHaveBeenCalled();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('sends only the handle after the person agrees (Rust makes the text)', async () => {
    const ship = api();
    await mount(PR(report('failing', ['lint'])), ship);
    await act(async () => button('Send back to Codex')?.click());
    const send = Array.from(document.querySelectorAll('button')).find((b) => b.textContent === 'Send');
    await act(async () => send?.click());
    // Only the folder and the handle go to Rust: the task is made there, so no log text comes back.
    expect(ship.prSendBack).toHaveBeenCalledWith('D:/code/shop', 'cart-1');
    expect(document.body.textContent).toContain('Sent to Codex.');
  });

  it('shows the refusal in plain words and opens nothing when the agent could push', async () => {
    const ship = api({
      prSendBackPreview: vi.fn().mockRejectedValue(new Error('SEND_BACK_CAN_PUSH: This agent could push to GitHub, so Puppet Master will not hand it CI output.')),
    });
    await mount(PR(report('failing', ['lint'])), ship);
    await act(async () => button('Send back to Codex')?.click());
    await settle();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('could push to GitHub');
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(ship.prSendBack).not.toHaveBeenCalled();
  });

  it('shows a plain error when the checks cannot be read', async () => {
    const ship = api({ prChecks: vi.fn().mockRejectedValue(new Error('CHECKS_UNAVAILABLE: Could not read the GitHub checks: offline')) });
    await mount(PR(), ship);
    expect(document.body.textContent).toContain('Could not read the GitHub checks.');
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('offline');
  });
});
