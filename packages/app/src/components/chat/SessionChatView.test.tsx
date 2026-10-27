// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { formatDuration, SCREEN_CHAT_NOTE, SESSION_CHAT_NOTE, SessionChatView, type SessionChatViewProps } from './SessionChatView';

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

function turnsFrom(items: ChatItem[]): SessionChatViewProps['turns'] {
  return groupTurns(items).turns;
}

function render(props: Partial<SessionChatViewProps> & Pick<SessionChatViewProps, 'state'>) {
  const full: SessionChatViewProps = {
    reason: null,
    turns: [],
    model: null,
    cliVersion: null,
    ...props,
  };
  act(() => {
    root.render(<SessionChatView {...full} />);
  });
}

const text = () => host.textContent ?? '';

const WAITING = "Waiting for the agent's session file…";

const SAMPLE: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '20:0', at: null, text: 'Listing now.', model: null, finalAnswer: false },
  { kind: 'toolCall', id: '30:0', at: null, callId: 'c1', name: 'Bash', summary: 'ls -la /work' },
  { kind: 'toolResult', id: '40:0', at: null, callId: 'c1', ok: true, output: 'README.md\nsrc' },
  { kind: 'assistant', id: '50:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '60:0', at: null, outcome: 'completed', durationMs: 12_000 },
];

describe('SessionChatView states', () => {
  it('shows the empty-chat note above the waiting note, and only while there are no turns', () => {
    render({ state: 'ready', emptyNote: <span>New session line</span> });
    expect(text().indexOf('New session line')).toBeLessThan(text().indexOf(WAITING));
    render({ state: 'ready', emptyNote: <span>New session line</span>, turns: turnsFrom(SAMPLE) });
    expect(text()).not.toContain('New session line');
  });

  it('shows the waiting note while the session file is not read yet', () => {
    render({ state: 'loading' });
    expect(text()).toContain(WAITING);
  });

  it('shows the waiting note for a ready chat with no items yet', () => {
    render({ state: 'ready', turns: [] });
    expect(text()).toContain(WAITING);
  });

  it('says what it is waiting for in a muted line under the waiting note', () => {
    const line = 'Looking in C:\\x\\projects\\p for a file written after 05:03:05.';
    render({ state: 'ready', turns: [], waitingFor: line });
    expect(text()).toContain(WAITING);
    const note = [...host.querySelectorAll('p')].find((p) => p.textContent === line);
    expect(note?.className).toContain('text-pm-muted');
    expect(note?.previousElementSibling?.textContent).toBe(WAITING);
  });

  it('shows no extra line when nothing is known, and none once there are turns', () => {
    render({ state: 'ready', turns: [] });
    expect(host.querySelector('.ck-chat-waiting-for')).toBeNull();
    render({ state: 'ready', turns: turnsFrom(SAMPLE), waitingFor: 'stale line' });
    expect(text()).not.toContain('stale line');
  });

  it('shows the reason when the chat is unavailable', () => {
    render({ state: 'unavailable', reason: 'This agent keeps no session file.' });
    expect(text()).toContain('This agent keeps no session file.');
    expect(text()).not.toContain(WAITING);
  });

  it('always shows the note under the header: messages wait, questions are answered in the card', () => {
    render({ state: 'ready', turns: [] });
    expect(text()).toContain(SESSION_CHAT_NOTE);
    expect(SESSION_CHAT_NOTE).toBe('Messages wait until the agent is free. Answer its questions in the card at the bottom.');
  });

  it('puts the pending bubbles after the conversation and the message box under the dock', () => {
    render({
      state: 'ready',
      turns: turnsFrom(SAMPLE),
      dock: <div data-testid="the-dock">dock</div>,
      pending: <div data-testid="the-pending">pending</div>,
      composer: <div data-testid="the-composer">composer</div>,
    });
    const pending = host.querySelector('[data-testid="the-pending"]')!;
    const dock = host.querySelector('[data-testid="the-dock"]')!;
    const composer = host.querySelector('[data-testid="the-composer"]')!;
    const body = host.querySelector('.ck-chat-body')!;
    expect(body.contains(pending)).toBe(true);
    expect(body.contains(dock) || body.contains(composer)).toBe(false);
    // Document order: conversation, pending bubbles, dock (the approval card), message box.
    const order = (a: Node, b: Node) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(order(pending, dock)).toBe(true);
    expect(order(dock, composer)).toBe(true);
  });

  it('shows no message box or bubbles while the chat is unavailable', () => {
    render({ state: 'unavailable', reason: 'No chat.', pending: <b>pending</b>, composer: <b>composer</b> });
    expect(text()).not.toContain('composer');
    expect(text()).not.toContain('pending');
  });
});

describe('SessionChatView ready', () => {
  it('shows the person and agent text as plain text', () => {
    render({ state: 'ready', turns: turnsFrom(SAMPLE) });
    expect(text()).toContain('List the files');
    expect(text()).toContain('Listing now.');
    expect(text()).toContain('Two entries.');
    const agentText = [...host.querySelectorAll('div')].find((d) => d.textContent === 'Listing now.');
    expect(agentText?.className).toContain('whitespace-pre-wrap');
  });

  it('folds the tool rows of a turn into one activity line; a click opens the rows, and a row opens its output', () => {
    render({ state: 'ready', turns: turnsFrom(SAMPLE) });
    const line = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(line.textContent).toBe('Ran 1 command');
    expect(text()).not.toContain('README.md');
    expect(host.querySelector('.ck-chat-row')).toBeNull();

    act(() => line.click());
    expect(line.getAttribute('aria-expanded')).toBe('true');
    const row = host.querySelector<HTMLButtonElement>('.ck-chat-row button')!;
    expect(row.textContent).toContain('Bash');
    expect(row.textContent).toContain('ls -la /work');
    expect(row.textContent).toContain('ok');
    expect(text()).not.toContain('README.md');

    act(() => row.click());
    expect(row.getAttribute('aria-expanded')).toBe('true');
    const pre = host.querySelector('[data-testid="tool-output"]');
    expect(pre?.textContent).toBe('README.md\nsrc');
    expect(pre?.className).toContain('max-h-48');
    expect(pre?.className).toContain('overflow-auto');

    act(() => row.click());
    expect(host.querySelector('[data-testid="tool-output"]')).toBeNull();
  });

  it('shows a running tool as a live line with a spinner, and its row has no output yet', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'edit' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'c9', name: 'Edit', summary: 'src/a.ts' },
    ];
    render({ state: 'ready', turns: turnsFrom(items) });
    const line = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(line.textContent).toBe('Working · Edit src/a.ts');
    expect(host.querySelector('[aria-label="Working"]')).not.toBeNull();
    act(() => line.click());
    const row = host.querySelector<HTMLButtonElement>('.ck-chat-row button')!;
    expect(row.textContent).toContain('running');
    act(() => row.click());
    expect(host.querySelector('[data-testid="tool-output"]')?.textContent).toBe('No output yet.');
  });

  it('shows the duration and an interrupted outcome at the turn end', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'go' },
      { kind: 'turnEnd', id: '2:0', at: null, outcome: 'interrupted', durationMs: 12_000 },
    ];
    render({ state: 'ready', turns: turnsFrom(items) });
    expect(text()).toContain('interrupted · Worked for 12s');
  });

  it('shows the model and the CLI version in the header', () => {
    render({ state: 'ready', turns: [], model: 'example-model-1', cliVersion: '2.3.4' });
    const header = host.querySelector('header')!;
    expect(header.textContent).toContain('example-model-1');
    expect(header.textContent).toContain('CLI 2.3.4');
  });

  it('says the model is not known yet when the session has not named it', () => {
    render({ state: 'ready', turns: [] });
    expect(host.querySelector('header')!.textContent).toContain('Model not known yet');
  });
});

describe('formatDuration', () => {
  it('formats seconds and minutes', () => {
    expect(formatDuration(400)).toBe('<1s');
    expect(formatDuration(12_000)).toBe('12s');
    expect(formatDuration(125_000)).toBe('2m 5s');
  });
});

describe('SessionChatView read from the screen', () => {
  const SCREEN: ChatItem[] = [
    { kind: 'user', id: 'screen:1:user', at: null, text: 'List the files' },
    { kind: 'toolCall', id: 'screen:1:tool:0', at: null, callId: 'screen:1:tool:0', name: '', summary: '$ ls -la' },
    { kind: 'assistant', id: 'screen:1:reply', at: null, text: 'Two entries.', model: null, finalAnswer: true },
    { kind: 'turnEnd', id: 'screen:1:end', at: null, outcome: 'completed', durationMs: null },
  ];

  it('says the text is read from the screen, in a muted note under the read-only note', () => {
    render({ state: 'ready', source: 'screen', turns: turnsFrom(SCREEN) });
    const note = host.querySelector('[data-testid="session-chat-screen-note"]');
    expect(note?.textContent).toBe(SCREEN_CHAT_NOTE);
    expect(note?.textContent).toBe('Read from the screen: text may be cut or wrapped.');
    expect(text()).toContain(SESSION_CHAT_NOTE);
  });

  it('shows no screen note for a chat read from a file, or when the source is not known', () => {
    render({ state: 'ready', source: 'file', turns: turnsFrom(SAMPLE) });
    expect(host.querySelector('[data-testid="session-chat-screen-note"]')).toBeNull();
    render({ state: 'ready', turns: turnsFrom(SAMPLE) });
    expect(host.querySelector('[data-testid="session-chat-screen-note"]')).toBeNull();
  });

  it('counts the rows of a screen chat in the activity line, and a row shows just its text and says output is not read', () => {
    render({ state: 'ready', source: 'screen', turns: turnsFrom(SCREEN) });
    const line = host.querySelector('button[aria-expanded]') as HTMLButtonElement;
    expect(line.textContent).toBe('Ran 1 command');
    act(() => line.click());
    const row = host.querySelector('.ck-chat-row button') as HTMLButtonElement;
    expect(row.textContent).toBe('$ ls -la');
    act(() => row.click());
    expect(text()).toContain('Tool output is not read from the screen.');
    expect(text()).not.toContain('No output yet.');
  });

  it('waits for a message on the terminal, not for a session file', () => {
    render({ state: 'ready', source: 'screen', turns: [] });
    expect(text()).toContain('Waiting for a message on the terminal');
    expect(text()).not.toContain('session file');
  });
});

describe('the docked card under the conversation', () => {
  it('is outside the scrolling conversation, so it stays in view', () => {
    render({ state: 'ready', turns: turnsFrom(SAMPLE), dock: <p>Needs you · Run tests</p> });
    const body = host.querySelector('.ck-chat-body')!;
    const dock = host.querySelector('.ck-chat-dock')!;
    expect(dock.textContent).toContain('Needs you · Run tests');
    expect(body.contains(dock)).toBe(false);
  });

  it('keeps a reader at the end at the end when the card takes height, and a reader above keeps the place', () => {
    // jsdom has no layout: pin the sizes and the scroll position, and capture the resize observer's callback.
    let resized: (() => void) | null = null;
    const saved = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
      constructor(cb: () => void) {
        resized = cb;
      }
      observe() {}
      disconnect() {}
    };
    try {
      render({ state: 'ready', turns: turnsFrom(SAMPLE) });
      const el = host.querySelector<HTMLElement>('.ck-chat-body')!;
      let top = 0;
      Object.defineProperty(el, 'scrollHeight', { configurable: true, get: () => 500 });
      Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => 200 });
      Object.defineProperty(el, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => (top = v) });
      el.scrollTop = 500;
      el.dispatchEvent(new Event('scroll'));

      // a card appears under the chat: the reader at the end is still at the end
      act(() => resized?.());
      expect(top).toBe(500);

      // a reader who scrolled up keeps the place when the card takes height
      top = 100;
      el.dispatchEvent(new Event('scroll'));
      act(() => resized?.());
      expect(top).toBe(100);
    } finally {
      (globalThis as { ResizeObserver?: unknown }).ResizeObserver = saved;
    }
  });
});
