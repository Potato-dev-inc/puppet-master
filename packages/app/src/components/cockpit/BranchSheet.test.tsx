// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BranchFrom, BranchOffer, BranchPreview, BranchView } from '../../lib/branch-from';
import { BranchSheet } from './BranchSheet';

// The sheet talks to the desktop only through lib/branch-from.ts: mock the Tauri invoke it calls.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Args = Record<string, unknown>;

// 14:02 and 14:01 in local time, so the clock text is the same on every machine.
const AFTER_AT = new Date(2026, 9, 29, 14, 2).getTime();
const AFTER = { checkpointId: '12', label: 'After turn 5', atMs: AFTER_AT, files: 3 };
const BEFORE = { checkpointId: '11', label: 'Before turn 5', atMs: AFTER_AT - 60_000, files: 0 };

const codex: BranchOffer = { target: { agent_id: 'codex' }, label: 'Codex', ok: true, whyNot: null };
const cursor: BranchOffer = { target: { agent_id: 'cursor' }, label: 'Cursor', ok: false, whyNot: 'Not signed in' };
const codexWork: BranchOffer = { target: { agent_id: 'codex', account: 'work' }, label: 'Codex · Work', ok: true, whyNot: null };

function preview(over: Partial<BranchPreview> = {}): BranchPreview {
  return {
    fromLabel: 'Claude Code',
    point: AFTER,
    canBranch: true,
    whyNot: null,
    offers: [codex, cursor],
    prefill: null,
    turns: 5,
    openBranches: 0,
    brief: null,
    ...over,
  };
}

const started: BranchView = {
  id: 'bf-1a2b3c4d5e6f',
  fromLabel: 'Claude Code',
  toLabel: 'Codex',
  fromPaneId: 'p-claude',
  toPaneId: 'p-new',
  atMs: AFTER_AT,
  pointLabel: 'after turn 5 · 14:02',
  state: 'preparing',
  stateText: 'Making a copy from the save point',
  turns: 5,
  briefBytes: 20,
  branchName: 'pm/bf-1a2b3c4d5e6f',
  closed: null,
  failedFiles: 0,
};

const FROM_TURN: BranchFrom = { kind: 'message', user_item_id: 'u5', when: 'after' };
const FROM_ROW: BranchFrom = { kind: 'save_point', checkpoint_id: '12' };

let host: HTMLDivElement;
let root: Root;

// What the mocked desktop answers. Tests replace these when they need another answer.
let previewAnswer: (args: Args) => Promise<BranchPreview>;
let startAnswer: (args: Args) => Promise<BranchView>;

beforeEach(() => {
  vi.clearAllMocks();
  previewAnswer = async (args) => {
    const from = args.from as BranchFrom;
    if (from.kind === 'message' && from.when === 'before') return preview({ point: BEFORE, prefill: 'Fix the parser' });
    const message = args.message as string | null;
    return preview({ brief: message ? { text: 'Branch: short brief.', bytes: 20, trimmed: [] } : null });
  };
  startAnswer = async () => started;
  vi.mocked(invoke).mockImplementation((async (cmd: string, args: Args) => {
    if (cmd === 'branch_preview') return previewAnswer(args);
    if (cmd === 'branch_start') return startAnswer(args);
    throw new Error(`unexpected command ${cmd}`);
  }) as never);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function open(from: BranchFrom = FROM_TURN, props: { onClose?: () => void; onStarted?: (view: BranchView) => void } = {}) {
  const onClose = props.onClose ?? vi.fn();
  await act(async () => {
    root.render(<BranchSheet paneId="p-claude" from={from} onClose={onClose} onStarted={props.onStarted} />);
  });
  return onClose;
}

const radios = () => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
const textarea = () => host.querySelector<HTMLTextAreaElement>('textarea');
const primary = () => host.querySelector<HTMLButtonElement>('.ck-btn--primary');
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text) as HTMLButtonElement | undefined;
const alertText = () => host.querySelector('[role="alert"]')?.textContent;

/** Types into the task box the way a person does: the value is set, then `input` fires. */
async function typeText(value: string) {
  const el = textarea()!;
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const pause = (ms: number) => act(async () => new Promise((resolve) => setTimeout(resolve, ms)));

describe('BranchSheet: the agents', () => {
  it('asks the desktop for the offers once, with the turn it starts from', async () => {
    await open();
    expect(invoke).toHaveBeenCalledWith('branch_preview', { paneId: 'p-claude', from: FROM_TURN, target: null, message: null });
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === 'branch_preview')).toHaveLength(1);
  });

  it('shows a row that is not ok disabled, with its reason, and never lets it be picked', async () => {
    await open();
    expect(host.querySelector('.ck-branch-row.is-off')?.textContent).toContain('Not signed in');
    expect(radios().find((r) => r.value === 'cursor')!.disabled).toBe(true);
    expect(radios().find((r) => r.value === 'codex')!.checked).toBe(true);
    expect(host.querySelector('.ck-branch-row:not(.is-off) .ck-branch-state')?.textContent).toBe('Ready');
  });

  it('says there is no agent when none is offered', async () => {
    previewAnswer = async () => preview({ offers: [] });
    await open();
    expect(host.textContent).toContain('No agent can run in this folder yet.');
    expect(radios()).toHaveLength(0);
  });
});

describe('BranchSheet: Start', () => {
  it('is disabled until a row and a message are set', async () => {
    await open();
    expect(primary()!.disabled).toBe(true);
    await typeText('Try it again');
    expect(primary()!.disabled).toBe(false);
    expect(primary()!.textContent).toBe('Start Codex');
  });

  it('stays disabled when no agent can be picked, even with a message', async () => {
    previewAnswer = async () => preview({ offers: [cursor] });
    await open();
    await typeText('Try it again');
    expect(primary()!.disabled).toBe(true);
    expect(primary()!.textContent).toBe('Start');
  });

  it('a message of spaces only does not count as a message', async () => {
    await open();
    await typeText('   ');
    expect(primary()!.disabled).toBe(true);
  });

  it('starts the picked agent with the trimmed message and the same turn, then closes', async () => {
    const onStarted = vi.fn();
    const onClose = await open(FROM_TURN, { onStarted });
    await typeText('  Try it again  ');
    await act(async () => primary()!.click());
    expect(invoke).toHaveBeenCalledWith('branch_start', {
      paneId: 'p-claude',
      from: FROM_TURN,
      target: 'codex',
      message: 'Try it again',
    });
    expect(onStarted).toHaveBeenCalledWith(started);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('sends the agent@account text of the row the person picks', async () => {
    previewAnswer = async () => preview({ offers: [codex, codexWork] });
    await open(FROM_ROW);
    await act(async () => radios().find((r) => r.value === 'codex@work')!.click());
    await typeText('Go');
    await act(async () => primary()!.click());
    expect(invoke).toHaveBeenCalledWith('branch_start', { paneId: 'p-claude', from: FROM_ROW, target: 'codex@work', message: 'Go' });
  });
});

describe('BranchSheet: Before and After this turn', () => {
  it('shows the switch for a chat turn only, not for a History row', async () => {
    await open(FROM_ROW);
    expect(host.querySelector('[role="group"]')).toBeNull();
    expect(button('Before this turn')).toBeUndefined();
  });

  it("Before fills the message with that turn's text and changes the save point line", async () => {
    await open();
    await act(async () => button('Before this turn')!.click());
    expect(invoke).toHaveBeenCalledWith('branch_preview', {
      paneId: 'p-claude',
      from: { kind: 'message', user_item_id: 'u5', when: 'before' },
      target: null,
      message: null,
    });
    expect(textarea()!.value).toBe('Fix the parser');
    expect(button('Before this turn')!.getAttribute('aria-pressed')).toBe('true');
    expect(host.querySelector('.ck-branch-point')?.textContent).toBe('Save point: Before turn 5 · 14:01');
  });

  it('keeps a message the person typed when switching to Before', async () => {
    await open();
    await typeText('My own words');
    await act(async () => button('Before this turn')!.click());
    expect(textarea()!.value).toBe('My own words');
  });

  it('switching back to After keeps the text and the After line', async () => {
    await open();
    await act(async () => button('Before this turn')!.click());
    await act(async () => button('After this turn')!.click());
    expect(textarea()!.value).toBe('Fix the parser');
    expect(host.querySelector('.ck-branch-point')?.textContent).toBe('Save point: After turn 5 · 14:02 · 3 files changed in that turn');
  });
});

describe('BranchSheet: the save point line', () => {
  it('leaves the file part out when no file changed, and says one file in the singular', async () => {
    previewAnswer = async () => preview({ point: { ...AFTER, files: 0 } });
    await open(FROM_ROW);
    expect(host.querySelector('.ck-branch-point')?.textContent).toBe('Save point: After turn 5 · 14:02');
  });

  it('says one file in the singular', async () => {
    previewAnswer = async () => preview({ point: { ...AFTER, files: 1 } });
    await open(FROM_ROW);
    expect(host.querySelector('.ck-branch-point')?.textContent).toBe('Save point: After turn 5 · 14:02 · 1 file changed in that turn');
  });
});

describe('BranchSheet: when it cannot branch', () => {
  it('shows only the reason and Close', async () => {
    const reason = 'Branching needs Git in this folder. Use "Set up Git here" first.';
    previewAnswer = async () => preview({ canBranch: false, whyNot: reason });
    const onClose = await open();
    expect(host.textContent).toContain(reason);
    expect(textarea()).toBeNull();
    expect(radios()).toHaveLength(0);
    expect(primary()!.textContent).toBe('Close');
    expect([...host.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Close']);
    await act(async () => primary()!.click());
    expect(onClose).toHaveBeenCalledOnce();
    expect(invoke).not.toHaveBeenCalledWith('branch_start', expect.anything());
  });

  it('a refused preview shows its message without the code, and only Close', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("BRANCH_NOT_HERE: Branching isn't available for this pane."));
    await open();
    expect(host.textContent).toContain("Branching isn't available for this pane.");
    expect(host.textContent).not.toContain('BRANCH_NOT_HERE');
    expect([...host.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Close']);
  });
});

describe('BranchSheet: the task box', () => {
  it('shows a counter only after 3,500 characters', async () => {
    await open();
    await typeText('a'.repeat(3_500));
    expect(host.querySelector('.ck-branch-count')).toBeNull();
    await typeText('a'.repeat(3_501));
    expect(host.querySelector('.ck-branch-count')?.textContent).toBe('3,501 / 4,000');
  });

  it('over 4,000 characters Start is disabled and the box says why', async () => {
    await open();
    await typeText('a'.repeat(4_001));
    expect(host.querySelector('.ck-branch-count.is-over')).not.toBeNull();
    expect(host.textContent).toContain('Make your message shorter (4,000 characters at most).');
    expect(primary()!.disabled).toBe(true);
  });
});

describe('BranchSheet: errors from the start', () => {
  it('shows the message from the desktop without its code, and stays open', async () => {
    vi.mocked(invoke).mockImplementation((async (cmd: string, args: Args) => {
      if (cmd === 'branch_preview') return previewAnswer(args);
      throw new Error('BRANCH_BUSY: A branch is already being made from this chat. Wait a moment.');
    }) as never);
    const onClose = await open();
    await typeText('Try it');
    await act(async () => primary()!.click());
    expect(alertText()).toBe('A branch is already being made from this chat. Wait a moment.');
    expect(host.textContent).not.toContain('BRANCH_BUSY');
    expect(onClose).not.toHaveBeenCalled();
    expect(primary()!.disabled).toBe(false);
  });

  it('a bare error code is never shown: a plain sentence instead', async () => {
    vi.mocked(invoke).mockImplementation((async (cmd: string, args: Args) => {
      if (cmd === 'branch_preview') return previewAnswer(args);
      throw 'BRANCH_COPY_FAILED';
    }) as never);
    await open();
    await typeText('Try it');
    await act(async () => primary()!.click());
    expect(alertText()).toBe('Something went wrong. Try again.');
  });

  it('is busy while it starts: Start says so and Cancel and the box wait', async () => {
    let finish: () => void = () => {};
    startAnswer = () => new Promise<BranchView>((resolve) => (finish = () => resolve(started)));
    await open();
    await typeText('Try it');
    await act(async () => primary()!.click());
    expect(primary()!.textContent).toBe('Starting…');
    expect(primary()!.disabled).toBe(true);
    expect(button('Cancel')!.disabled).toBe(true);
    expect(textarea()!.disabled).toBe(true);
    await act(async () => finish());
  });
});

describe('BranchSheet: what the agent gets', () => {
  it('shows the brief for the message in the box, with its size and trimmed parts', async () => {
    await open();
    await typeText('Try it');
    await act(async () => button('Show what the agent gets')!.click());
    expect(button('Show what the agent gets')!.getAttribute('aria-expanded')).toBe('true');
    await pause(600);
    expect(invoke).toHaveBeenCalledWith('branch_preview', { paneId: 'p-claude', from: FROM_TURN, target: null, message: 'Try it' });
    expect(host.querySelector('.ck-branch-brief')?.textContent).toBe('Branch: short brief.');
    expect(host.querySelector('.ck-branch-brief-box .ck-sub')?.textContent).toBe('20 B · trimmed: none');
  });

  it('with an empty box it says what to type and asks the desktop for nothing', async () => {
    await open();
    await act(async () => button('Show what the agent gets')!.click());
    await pause(600);
    expect(host.querySelector('.ck-branch-brief-box')?.textContent).toBe('Type what it should do to see the brief.');
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === 'branch_preview')).toHaveLength(1);
  });

  it('says the copy and the not-copied files in plain words', async () => {
    await open();
    expect(host.textContent).toContain('Puppet Master makes a separate copy of your files from this save point.');
    expect(host.textContent).toContain('Not in the copy: files Git ignores (like node_modules) and files over 20 MB.');
  });
});
