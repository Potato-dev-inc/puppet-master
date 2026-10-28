// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { keepGoingContinue, keepGoingSettingsGet, keepGoingTargets, type KeepGoingOffer, type KeepGoingSettings } from '../../lib/keep-going';
import { ContinueOnSheet } from './ContinueOnSheet';

vi.mock('../../lib/keep-going', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/keep-going')>()),
  keepGoingTargets: vi.fn(),
  keepGoingContinue: vi.fn(),
  keepGoingSettingsGet: vi.fn(),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.clearAllMocks();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const codex: KeepGoingOffer = { target: { agent_id: 'codex' }, label: 'Codex', ok: true, whyNot: null };
const cursor: KeepGoingOffer = { target: { agent_id: 'cursor' }, label: 'Cursor', ok: false, whyNot: 'Not signed in' };
const codexWork: KeepGoingOffer = { target: { agent_id: 'codex', account: 'work' }, label: 'Codex · Work', ok: true, whyNot: null };
const settings = (ask_back: boolean): KeepGoingSettings => ({ order: [], ask_back });

// 17:05 in local time, so the clock text is the same on every machine.
const resetAtMs = new Date(2026, 9, 28, 17, 5).getTime();

async function open(props: { offers?: KeepGoingOffer[]; ask_back?: boolean; resetAtMs?: number; onClose?: () => void } = {}) {
  vi.mocked(keepGoingTargets).mockResolvedValue(props.offers ?? [codex, cursor]);
  vi.mocked(keepGoingSettingsGet).mockResolvedValue(settings(props.ask_back ?? true));
  const onClose = props.onClose ?? vi.fn();
  await act(async () => {
    root.render(<ContinueOnSheet paneId="p-claude" cardId="l1" fromLabel="Claude Code" resetAtMs={props.resetAtMs} onClose={onClose} />);
  });
  return onClose;
}

const radios = () => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
const primary = () => host.querySelector<HTMLButtonElement>('.ck-btn--primary');
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text) as HTMLButtonElement | undefined;

describe('ContinueOnSheet: the picker', () => {
  it("asks for this pane's agents and lists them with Ready or the reason they are not", async () => {
    await open();
    expect(keepGoingTargets).toHaveBeenCalledWith('p-claude');
    expect([...host.querySelectorAll('.ck-keep-going-name')].map((n) => n.textContent)).toEqual(['Codex', 'Cursor']);
    expect([...host.querySelectorAll('.ck-keep-going-state')].map((n) => n.textContent)).toEqual(['Ready', 'Not signed in']);
  });

  it('shows a row that is not ok disabled, with its reason, and never lets it be picked', async () => {
    await open();
    const cursorRadio = radios().find((r) => r.value === 'cursor')!;
    expect(cursorRadio.disabled).toBe(true);
    expect(host.querySelector('.ck-keep-going-row.is-off')?.textContent).toContain('Not signed in');
  });

  it('preselects the first ok agent and names it on the primary button', async () => {
    await open();
    expect(radios().find((r) => r.value === 'codex')!.checked).toBe(true);
    expect(primary()!.textContent).toBe('Continue on Codex');
  });

  it("follows the row the person picks, and sends its agent@account text", async () => {
    await open({ offers: [codex, codexWork] });
    await act(async () => radios().find((r) => r.value === 'codex@work')!.click());
    expect(primary()!.textContent).toBe('Continue on Codex · Work');
    await act(async () => primary()!.click());
    expect(keepGoingContinue).toHaveBeenCalledWith('p-claude', 'codex@work', true, 'l1');
  });

  it('says there is no agent when the list is empty, and cannot continue', async () => {
    await open({ offers: [] });
    expect(host.textContent).toContain('No other agent can run in this folder yet');
    expect(primary()!.disabled).toBe(true);
  });

  it('says what the agent gets and which agent is stopped, in plain words', async () => {
    await open();
    expect(host.textContent).toContain('The agent gets a short brief. Claude Code in this pane is stopped first.');
    expect(host.textContent).toContain('Puppet Master never answers its questions for you.');
  });
});

describe('ContinueOnSheet: the move-back switch', () => {
  it('shows only when the reset time is known, with that time', async () => {
    await open({ resetAtMs });
    expect(host.querySelector('.ck-keep-going-back')?.textContent).toContain('Ask me to move it back at 17:05');
  });

  it('is absent when the reset time is not known', async () => {
    await open();
    expect(host.querySelector('.ck-keep-going-back')).toBeNull();
  });

  it('shows the saved setting and cannot be changed here', async () => {
    await open({ resetAtMs, ask_back: false });
    const box = host.querySelector<HTMLInputElement>('.ck-keep-going-back input')!;
    expect(box.checked).toBe(false);
    expect(box.disabled).toBe(true);
  });

  it('sends the saved setting as back when the reset time is not known', async () => {
    await open({ ask_back: false });
    await act(async () => primary()!.click());
    expect(keepGoingContinue).toHaveBeenCalledWith('p-claude', 'codex', false, 'l1');
  });

  it('sends the saved setting as back when the reset time is known', async () => {
    await open({ resetAtMs, ask_back: true });
    await act(async () => primary()!.click());
    expect(keepGoingContinue).toHaveBeenCalledWith('p-claude', 'codex', true, 'l1');
  });
});

describe('ContinueOnSheet: starting the hand-over', () => {
  it('closes after the hand-over starts', async () => {
    vi.mocked(keepGoingContinue).mockResolvedValue(undefined as never);
    const onClose = await open();
    await act(async () => primary()!.click());
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('shows the message from the backend without its code, and stays open', async () => {
    vi.mocked(keepGoingContinue).mockRejectedValue(new Error('KEEP_GOING_NOT_AT_LIMIT: The agent is not at its limit now.'));
    const onClose = await open();
    await act(async () => primary()!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('The agent is not at its limit now.');
    expect(host.textContent).not.toContain('KEEP_GOING');
    expect(onClose).not.toHaveBeenCalled();
    expect(primary()!.disabled).toBe(false);
  });

  it('a bare error code is never shown: a plain sentence instead', async () => {
    vi.mocked(keepGoingContinue).mockRejectedValue('KEEP_GOING_ALREADY');
    await open();
    await act(async () => primary()!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Something went wrong. Try again.');
  });

  it('is busy while it starts: no second press, and Cancel waits', async () => {
    let finish: () => void = () => {};
    vi.mocked(keepGoingContinue).mockImplementation(() => new Promise((resolve) => (finish = () => resolve(undefined as never))));
    await open();
    await act(async () => primary()!.click());
    expect(primary()!.textContent).toBe('Starting…');
    expect(primary()!.disabled).toBe(true);
    expect(button('Cancel')!.disabled).toBe(true);
    await act(async () => finish());
  });

  it('Cancel closes the sheet and starts nothing', async () => {
    const onClose = await open();
    await act(async () => button('Cancel')!.click());
    expect(onClose).toHaveBeenCalledOnce();
    expect(keepGoingContinue).not.toHaveBeenCalled();
  });
});

describe('ContinueOnSheet: when it cannot load', () => {
  it('a refused agent list shows the message and offers nothing to start', async () => {
    vi.mocked(keepGoingTargets).mockRejectedValue(new Error('KEEP_GOING_NO_PANE: This pane cannot hand over.'));
    vi.mocked(keepGoingSettingsGet).mockResolvedValue(settings(true));
    await act(async () => {
      root.render(<ContinueOnSheet paneId="p-companion" cardId="l1" fromLabel="Claude Code" onClose={vi.fn()} />);
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('This pane cannot hand over.');
    expect(radios()).toHaveLength(0);
    expect(primary()!.disabled).toBe(true);
  });

  it('without the saved setting it cannot continue, so no back value is guessed', async () => {
    vi.mocked(keepGoingTargets).mockResolvedValue([codex]);
    vi.mocked(keepGoingSettingsGet).mockRejectedValue(new Error('Settings are not ready.'));
    await act(async () => {
      root.render(<ContinueOnSheet paneId="p-claude" cardId="l1" fromLabel="Claude Code" onClose={vi.fn()} />);
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Settings are not ready.');
    expect(primary()!.disabled).toBe(true);
    await act(async () => primary()!.click());
    expect(keepGoingContinue).not.toHaveBeenCalled();
  });
});
