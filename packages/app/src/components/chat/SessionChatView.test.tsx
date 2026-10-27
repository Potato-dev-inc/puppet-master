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

  it('always shows the read-only note under the header', () => {
    render({ state: 'ready', turns: [] });
    expect(text()).toContain(SESSION_CHAT_NOTE);
    expect(SESSION_CHAT_NOTE).toBe('Read-only. Approvals and questions still appear in the terminal.');
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

  it('shows a tool call as one row and expands its output on click', () => {
    render({ state: 'ready', turns: turnsFrom(SAMPLE) });
    const row = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(row.textContent).toContain('Bash');
    expect(row.textContent).toContain('ls -la /work');
    expect(row.textContent).toContain('ok');
    expect(text()).not.toContain('README.md');

    act(() => row.click());
    expect(row.getAttribute('aria-expanded')).toBe('true');
    const pre = host.querySelector('pre');
    expect(pre?.textContent).toBe('README.md\nsrc');
    expect(pre?.className).toContain('max-h-48');
    expect(pre?.className).toContain('overflow-auto');

    act(() => row.click());
    expect(host.querySelector('pre')).toBeNull();
  });

  it('shows a running tool with no output yet', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'edit' },
      { kind: 'toolCall', id: '2:0', at: null, callId: 'c9', name: 'Edit', summary: 'src/a.ts' },
    ];
    render({ state: 'ready', turns: turnsFrom(items) });
    const row = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    expect(row.textContent).toContain('running');
    act(() => row.click());
    expect(host.querySelector('pre')?.textContent).toBe('No output yet.');
  });

  it('shows the duration and an interrupted outcome at the turn end', () => {
    const items: ChatItem[] = [
      { kind: 'user', id: '1:0', at: null, text: 'go' },
      { kind: 'turnEnd', id: '2:0', at: null, outcome: 'interrupted', durationMs: 12_000 },
    ];
    render({ state: 'ready', turns: turnsFrom(items) });
    expect(text()).toContain('interrupted · 12s');
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

  it('shows a tool row as just its text: no name, no running/ok label, and says output is not read', () => {
    render({ state: 'ready', source: 'screen', turns: turnsFrom(SCREEN) });
    const row = host.querySelector('button[aria-expanded]') as HTMLButtonElement;
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
