// @vitest-environment jsdom
// The helpers an agent started through Puppet Master: compact rows in the turn they began in, a click opens the pane.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatHelper } from '../../lib/chat-helpers';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { SessionChatView } from './SessionChatView';

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

const items: ChatItem[] = [
  { kind: 'user', id: '1', at: '2026-10-30T10:00:00.000Z', text: 'Map the app' },
  { kind: 'toolCall', id: '2', at: '2026-10-30T10:00:05.000Z', callId: 'c1', name: 'Bash', summary: 'ls' },
  { kind: 'toolResult', id: '3', at: '2026-10-30T10:00:06.000Z', callId: 'c1', ok: true, output: 'a' },
  { kind: 'turnEnd', id: '4', at: '2026-10-30T10:05:00.000Z', outcome: 'completed', durationMs: 300_000 },
  { kind: 'user', id: '5', at: '2026-10-30T11:00:00.000Z', text: 'Now check the tests' },
];
const helper = (id: string, title: string, status: ChatHelper['status'], iso: string, agent: string | null = 'Codex'): ChatHelper => ({ id, title, agent, status, startedAt: Date.parse(iso) });

function render(helpers: ChatHelper[], onOpenPane = vi.fn()) {
  act(() => {
    root.render(<SessionChatView state="ready" reason={null} turns={groupTurns(items).turns} model={null} cliVersion={null} source="file" helpers={helpers} onOpenPane={onOpenPane} />);
  });
  return onOpenPane;
}

const sections = () => [...host.querySelectorAll<HTMLElement>('[data-turn-id]')];
const rowsIn = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('[data-testid="pane-helper-row"]')];

describe('Puppet Master helpers in the chat', () => {
  it('shows "Helper: <task>" with the agent and a live status, in the turn it was started in', () => {
    render([helper('h1', 'Map app frontend', 'working', '2026-10-30T10:01:00Z'), helper('h2', 'Run the tests', 'needs-you', '2026-10-30T11:02:00Z')]);
    const [first, second] = sections();
    expect(rowsIn(first).map((r) => r.textContent)).toEqual(['Helper: Map app frontendCodexworking']);
    expect(rowsIn(second).map((r) => r.textContent)).toEqual(['Helper: Run the testsCodexneeds you']);
    // Before the turn's end line, and the turn's own tool activity is still there.
    expect(first.textContent).toContain('Worked for');
    expect(first.innerHTML.indexOf('pane-helper-rows')).toBeLessThan(first.innerHTML.indexOf('ck-chat-end'));
    expect(first.textContent).toContain('1 command');
  });

  it('marks done and failed helpers', () => {
    render([helper('h1', 'A', 'done', '2026-10-30T10:01:00Z'), helper('h2', 'B', 'failed', '2026-10-30T10:02:00Z', null)]);
    const rows = rowsIn(sections()[0]);
    expect(rows.map((r) => r.getAttribute('data-status'))).toEqual(['done', 'failed']);
    expect(rows[0].textContent).toContain('done');
    expect(rows[1].textContent).toBe('Helper: Bfailed');
  });

  it('opens the helper pane on a click', () => {
    const open = render([helper('h1', 'Map app frontend', 'working', '2026-10-30T10:01:00Z')]);
    act(() => rowsIn(sections()[0])[0].querySelector<HTMLButtonElement>('button')!.click());
    expect(open).toHaveBeenCalledWith('h1');
  });

  it('shows nothing extra for a chat whose agent started no helpers', () => {
    render([]);
    expect(host.querySelector('[data-testid="pane-helper-rows"]')).toBeNull();
  });
});
