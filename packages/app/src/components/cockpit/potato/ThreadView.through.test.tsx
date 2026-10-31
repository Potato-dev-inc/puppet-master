// @vitest-environment jsdom
// "Through my agents": the thread is the companion's own chat (SessionChatPanel of its pane), as on the companion's page.
// Mood, voice and the Potato message box are hidden, with the plain line.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type PotatoSettings } from '../../../lib/potato';
import { ThreadView, type ThreadViewProps } from './ThreadView';
import { bot, buttonByText } from './test-helpers';

const { panel } = vi.hoisted(() => ({ panel: vi.fn() }));
vi.mock('../../chat/SessionChatPanel', () => ({
  SessionChatPanel: (props: { paneId: string; reply?: unknown }) => {
    panel(props);
    return <div data-testid="session-chat">chat of {props.paneId}</div>;
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  panel.mockClear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const SETTINGS: PotatoSettings = { v: 1, brain: 'agents', defaultPersona: 'fish', dailyLimitCents: 50 };
const props = (over: Partial<ThreadViewProps> = {}): ThreadViewProps => ({
  bot: bot({ brainState: 'agents', mood: { mood: 'genki', intensity: 1, energy: 70, hunger: 0 } }),
  messages: [],
  settings: SETTINGS,
  avatar: 'mochi',
  agentLabel: 'Claude Code',
  transport: fakePotatoTransport({ bots: [bot()] }),
  onOpenSettings: () => {},
  onNotify: () => {},
  onChanged: () => {},
  ...over,
});

describe('ThreadView through my agents', () => {
  it("is the companion's own chat in its pane, with the plain line and no Potato box, mood or voice", () => {
    act(() => root.render(<ThreadView {...props({ companionPaneId: 'companion-fish' })} />));
    expect(host.querySelector('[data-testid="session-chat"]')?.textContent).toBe('chat of companion-fish');
    expect(panel).toHaveBeenCalledWith(expect.objectContaining({ paneId: 'companion-fish', reply: { blockedReason: null } }));
    expect(host.textContent).toContain('Mood and personas need the DeepSeek or OpenCode Go brain.');
    expect(host.querySelector('textarea[aria-label="Message Fish"]')).toBeNull();
    expect(host.querySelector('[role="tab"]')).toBeNull();
    expect(host.textContent).not.toContain('energy');
  });

  it("with no pane yet (the companion is asleep) it offers the companion's page instead of a chat", () => {
    const onOpenCompanion = vi.fn();
    act(() => root.render(<ThreadView {...props({ companionPaneId: null, onOpenCompanion })} />));
    expect(host.querySelector('[data-testid="session-chat"]')).toBeNull();
    expect(host.textContent).toContain('Fish is not awake.');
    act(() => buttonByText(host, 'Open Fish')!.click());
    expect(onOpenCompanion).toHaveBeenCalledWith('fish');
  });

  it('a new pane id for the same bot makes a new chat', () => {
    act(() => root.render(<ThreadView {...props({ companionPaneId: 'companion-fish' })} />));
    act(() => root.render(<ThreadView {...props({ companionPaneId: 'companion-fish-2' })} />));
    expect(host.querySelector('[data-testid="session-chat"]')?.textContent).toBe('chat of companion-fish-2');
  });
});
