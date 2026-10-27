// @vitest-environment jsdom
// The Chat tab's rows: the activity line, the tool detail (diff, command, JSON), helper rows, the task card, notices,
// the turn footer and the Summary | Normal | Verbose modes.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { SessionChatView, type SessionChatViewProps } from './SessionChatView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.clear();
});

function render(items: ChatItem[], props: Partial<SessionChatViewProps> = {}) {
  const grouped = groupTurns(items);
  act(() => {
    root.render(
      <SessionChatView
        state="ready"
        reason={null}
        turns={grouped.turns}
        model={null}
        cliVersion={null}
        source="file"
        {...props}
      />,
    );
  });
}

const text = () => host.textContent ?? '';
const lines = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-chat-activity')];
const rows = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-chat-row > button')];
const click = (el: Element | null) => act(() => (el as HTMLButtonElement).click());

const at = (sec: number) => new Date(Date.UTC(2026, 9, 28, 1, 0, sec)).toISOString();

/** One turn: the person asks, the agent reads a file, edits it, says a line, then the turn ends in `seconds`. */
const EDIT_TURN: ChatItem[] = [
  { kind: 'user', id: '1:0', at: null, text: 'fix the title' },
  { kind: 'toolCall', id: '2:0', at: at(0), callId: 'r1', name: 'Read', summary: 'src/app.ts', paths: ['src/app.ts'] },
  { kind: 'toolResult', id: '3:0', at: at(2), callId: 'r1', ok: true, output: 'const title = "old";' },
  {
    kind: 'toolCall',
    id: '4:0',
    at: at(3),
    callId: 'e1',
    name: 'Edit',
    summary: 'src/app.ts',
    paths: ['C:\\work\\app\\src\\app.ts'],
    writes: true,
    input: { file_path: 'C:\\work\\app\\src\\app.ts', old_string: 'const title = "old";', new_string: 'const title = "new";' },
  },
  { kind: 'toolResult', id: '5:0', at: at(4), callId: 'e1', ok: true, output: 'edited' },
  { kind: 'assistant', id: '6:0', at: null, text: 'Done: the title is new.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '7:0', at: null, outcome: 'completed', durationMs: 72_000 },
];

describe('the activity line', () => {
  it('counts the turn by kind with the span of its rows, and says how long the turn worked', () => {
    render(EDIT_TURN);
    expect(lines()).toHaveLength(1);
    expect(lines()[0]!.textContent).toBe('Read 1 file · Edited 1 file · 4s');
    expect(text()).toContain('Worked for 1m 12s');
  });

  it('starts a new line after a text step, so two runs make two lines', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'go' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'a', name: 'Bash', summary: 'npm ci' },
      { kind: 'toolResult', id: '3:0', at: null, callId: 'a', ok: true, output: '' },
      { kind: 'assistant', id: '4:0', at: null, text: 'Installed.', model: null, finalAnswer: false },
      { kind: 'toolCall', id: '5:0', at: null, callId: 'b', name: 'Grep', summary: 'title' },
      { kind: 'turnEnd', id: '6:0', at: null, outcome: 'completed', durationMs: 5_000 },
    ];
    render(items);
    expect(lines().map((line) => line.textContent)).toEqual(['Ran 1 command', 'Searched 1 time']);
  });

  it('shows a helper as its own row, not in the line', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'review' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'h', name: 'Agent', summary: 'Check the tests', helper: true, input: { description: 'Check the tests', prompt: 'Run the suite.' } },
      { kind: 'toolResult', id: '3:0', at: null, callId: 'h', ok: true, output: 'All green.' },
      { kind: 'turnEnd', id: '4:0', at: null, outcome: 'completed', durationMs: null },
    ];
    render(items);
    expect(lines()).toHaveLength(0);
    const helper = host.querySelector('[data-testid="helper-row"]')!;
    expect(helper.textContent).toContain('Helper: Check the tests');
    click(helper.querySelector('button'));
    expect(helper.textContent).toContain('Run the suite.');
    expect(helper.textContent).toContain('All green.');
  });
});

describe('the tool detail', () => {
  it('shows an edit as a diff with plus and minus gutters, and the file as a chip with its full path', () => {
    render(EDIT_TURN);
    click(lines()[0]!);
    click(rows()[1]!);
    const diff = host.querySelector('[data-testid="tool-diff"]')!;
    const removed = diff.querySelector('.ck-chat-diff-line--del')!;
    const added = diff.querySelector('.ck-chat-diff-line--add')!;
    expect(removed.querySelector('.ck-chat-diff-gutter')?.textContent).toBe('-');
    expect(removed.textContent).toContain('const title = "old";');
    expect(added.querySelector('.ck-chat-diff-gutter')?.textContent).toBe('+');
    expect(added.textContent).toContain('const title = "new";');
    const chip = host.querySelector('.ck-chat-chip')!;
    expect(chip.textContent).toBe('app.ts');
    expect(chip.getAttribute('title')).toBe('C:\\work\\app\\src\\app.ts');
  });

  it('shows a command in a mono block and its output in a capped block', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'test' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'c', name: 'Bash', summary: 'npm test', input: { command: 'npm test -- --run' } },
      { kind: 'toolResult', id: '3:0', at: null, callId: 'c', ok: true, output: '12 passed' },
      { kind: 'turnEnd', id: '4:0', at: null, outcome: 'completed', durationMs: null },
    ];
    render(items);
    click(lines()[0]!);
    click(rows()[0]!);
    const detail = host.querySelector('.ck-chat-detail')!;
    expect(detail.querySelector('pre')?.textContent).toBe('npm test -- --run');
    expect(detail.querySelector('[data-testid="tool-output"]')?.className).toContain('max-h-48');
    expect(detail.querySelector('[data-testid="tool-output"]')?.textContent).toBe('12 passed');
  });

  it('shows any other tool input as pretty JSON', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'plan' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'p', name: 'TodoWrite', summary: '2 tasks', input: { todos: [1] } },
      { kind: 'turnEnd', id: '3:0', at: null, outcome: 'completed', durationMs: null },
    ];
    render(items);
    click(lines()[0]!);
    click(rows()[0]!);
    expect(host.querySelector('.ck-chat-detail')!.textContent).toContain('"todos": [');
  });
});

describe('the task card, notices and modes', () => {
  const PLAN: ChatItem[] = [
    { kind: 'user', id: '1:0', at: null, text: 'do it' },
    {
      kind: 'plan',
      id: '2:0',
      at: null,
      replace: true,
      items: [
        { text: 'Read the code', status: 'completed' },
        { text: 'Write the fix', status: 'in_progress' },
        { text: 'Run the tests', status: 'pending' },
      ],
    },
    { kind: 'notice', id: '3:0', at: null, noticeKind: 'compacted', text: 'Context compacted' },
    { kind: 'assistant', id: '4:0', at: null, text: 'Working on it.', model: null, finalAnswer: false },
    { kind: 'assistant', id: '5:0', at: null, text: 'Fixed.', model: null, finalAnswer: true },
    { kind: 'turnEnd', id: '6:0', at: null, outcome: 'completed', durationMs: 3_000 },
  ];

  it('pins the task card with the count and a glyph per task', () => {
    render(PLAN);
    const card = host.querySelector('[data-testid="chat-plan"]')!;
    expect(card.querySelector('h3')?.textContent).toBe('Tasks 1/3');
    const statuses = [...card.querySelectorAll('li')].map((li) => li.getAttribute('data-status'));
    expect(statuses).toEqual(['completed', 'in_progress', 'pending']);
    expect(card.querySelector('[aria-label="In progress"]')).not.toBeNull();
    // The card is outside the scrolling conversation.
    expect(host.querySelector('.ck-chat-body')!.contains(card)).toBe(false);
  });

  it('shows a notice as a thin line with its text', () => {
    render(PLAN);
    const notice = host.querySelector('.ck-chat-notice')!;
    expect(notice.textContent).toBe('Context compacted');
    expect(notice.querySelectorAll('.ck-chat-rule')).toHaveLength(2);
  });

  it('Summary keeps the messages, the final answer and the activity lines; no notices, no talk', () => {
    render(PLAN, { paneId: 'pane-summary' });
    const summary = [...host.querySelectorAll<HTMLButtonElement>('[role="group"] button')].find((b) => b.textContent === 'Summary')!;
    click(summary);
    expect(summary.getAttribute('aria-pressed')).toBe('true');
    expect(text()).toContain('do it');
    expect(text()).toContain('Fixed.');
    expect(text()).not.toContain('Working on it.');
    expect(text()).not.toContain('Context compacted');
    expect(host.querySelector('[data-testid="session-chat"]')?.getAttribute('data-view')).toBe('summary');
  });

  it('Verbose opens each line and its rows with their detail', () => {
    render(EDIT_TURN, { paneId: 'pane-verbose' });
    expect(host.querySelector('[data-testid="tool-diff"]')).toBeNull();
    const verbose = [...host.querySelectorAll<HTMLButtonElement>('[role="group"] button')].find((b) => b.textContent === 'Verbose')!;
    click(verbose);
    expect(lines()[0]!.getAttribute('aria-expanded')).toBe('true');
    expect(host.querySelector('[data-testid="tool-diff"]')).not.toBeNull();
  });

  it('remembers the mode per pane, and another pane keeps its own', () => {
    render(EDIT_TURN, { paneId: 'pane-a' });
    click([...host.querySelectorAll<HTMLButtonElement>('[role="group"] button')].find((b) => b.textContent === 'Verbose')!);
    expect(localStorage.getItem('pm.chatView.pane-a')).toBe('verbose');
    render(EDIT_TURN, { paneId: 'pane-b' });
    expect(host.querySelector('[data-testid="session-chat"]')?.getAttribute('data-view')).toBe('normal');
    render(EDIT_TURN, { paneId: 'pane-a' });
    expect(host.querySelector('[data-testid="session-chat"]')?.getAttribute('data-view')).toBe('verbose');
  });
});
