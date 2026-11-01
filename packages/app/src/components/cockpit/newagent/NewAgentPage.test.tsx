// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentDescriptor } from '../../../lib/agentRegistry';
import type { RecentProject } from '../../../lib/ship-api';
import type { NewAgentApi } from './api';
import { EXAMPLES, EXAMPLE_MS, type HomeCard, type LaneView, type StartRequest } from './model';
import { NewAgentPage, type NewAgentPageProps } from './NewAgentPage';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent = (id: string, label: string, extra: Partial<AgentDescriptor> = {}): AgentDescriptor =>
  ({ id, label, kind: 'cli', installed: true, modes: { headless: true, visible: true }, ...extra }) as unknown as AgentDescriptor;

const project = (name: string, extra: Partial<RecentProject> = {}): RecentProject => ({ path: `/work/${name}`, name, state: 'repo', last_used_ms: 5, ...extra });

interface Rig {
  api: NewAgentApi;
  saveCards: ReturnType<typeof vi.fn>;
  emit: (next: { name?: string; cards?: HomeCard[] }) => void;
}

function rig(over: Partial<{ name: string; cards: HomeCard[]; recent: RecentProject[]; lanes: LaneView | null; paths: string[]; picked: RecentProject | null }> = {}): Rig {
  let listener: ((next: { name?: string; cards?: HomeCard[] }) => void) | null = null;
  const saveCards = vi.fn(async () => undefined);
  const api: NewAgentApi = {
    recent: async () => over.recent ?? [project('alpha-app'), project('beta-site')],
    pickFolder: async () => over.picked ?? null,
    pickPaths: async () => over.paths ?? [],
    defaultFolder: async () => null,
    gitState: async () => 'repo',
    lanes: async () => over.lanes ?? null,
    accounts: async () => [],
    loadHome: async () => ({ name: over.name ?? '', cards: over.cards ?? [] }),
    saveCards,
    onHomeChanged: async (cb) => {
      listener = cb;
      return () => {
        listener = null;
      };
    },
  };
  return { api, saveCards, emit: (next) => listener?.(next) };
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    /* none */
  }
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  vi.useRealTimers();
  await act(async () => root.unmount());
  host.remove();
});

async function mount(r: Rig, props: Partial<NewAgentPageProps> = {}) {
  const onStart = vi.fn(async (_request: StartRequest) => undefined);
  const onOpenPane = vi.fn();
  await act(async () => {
    root.render(
      <NewAgentPage
        agents={[agent('alpha', 'Alpha'), agent('beta', 'Beta'), agent('gone', 'Gone', { installed: false })]}
        panes={[
          { id: 'p1', title: 'Alpha 1', status: 'running' },
          { id: 'p2', title: 'Beta 1', status: 'idle' },
        ]}
        needs={[{ id: 'p3', title: 'Beta 2', status: 'waiting_input' }]}
        api={r.api}
        onStart={onStart}
        onOpenPane={onOpenPane}
        {...props}
      />,
    );
  });
  await act(async () => {});
  return { onStart, onOpenPane };
}

const input = () => host.querySelector<HTMLTextAreaElement>('textarea[aria-label="What should the agent do?"]')!;
const byLabel = (label: string | RegExp) =>
  [...host.querySelectorAll<HTMLElement>('button, [role=menuitem], [role=menuitemradio], [role=radio]')].find((b) => {
    const text = b.getAttribute('aria-label') ?? b.textContent ?? '';
    return typeof label === 'string' ? text.includes(label) : label.test(text);
  });
const click = async (el: HTMLElement | undefined) => {
  if (!el) throw new Error('element not found');
  await act(async () => el.click());
};
async function type(el: HTMLTextAreaElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
async function key(el: HTMLElement, k: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
  await act(async () => {
    el.dispatchEvent(event);
  });
  return event;
}
const send = () => host.querySelector<HTMLButtonElement>('button[aria-label="Send"]')!;

describe('the greeting', () => {
  it('has no name until the person saved one', async () => {
    await mount(rig());
    expect(host.querySelector('[data-testid="na-greeting"]')!.textContent).toBe('What\u2019s up next?');
  });

  it('uses the saved name, and follows a change made in Settings', async () => {
    const r = rig({ name: 'Sam' });
    await mount(r);
    expect(host.querySelector('[data-testid="na-greeting"]')!.textContent).toBe('What\u2019s up next, Sam?');
    await act(async () => r.emit({ name: '' }));
    expect(host.querySelector('[data-testid="na-greeting"]')!.textContent).toBe('What\u2019s up next?');
  });
});

describe('the example task', () => {
  it('shows an example as the placeholder, and Tab fills it in', async () => {
    await mount(rig());
    expect(input().placeholder).toBe(EXAMPLES[0]);
    expect(host.textContent).toContain('Tab');
    const event = await key(input(), 'Tab');
    expect(event.defaultPrevented).toBe(true);
    expect(input().value).toBe(EXAMPLES[0]);
    expect(host.querySelector('.ck-na-tabhint')).toBeNull();
  });

  it('leaves Tab alone once there is text', async () => {
    await mount(rig());
    await type(input(), 'my own words');
    const event = await key(input(), 'Tab');
    expect(event.defaultPrevented).toBe(false);
    expect(input().value).toBe('my own words');
  });

  it('shows the next example after a while, only while the box is empty', async () => {
    vi.useFakeTimers();
    await mount(rig());
    await act(async () => void vi.advanceTimersByTime(EXAMPLE_MS));
    expect(input().placeholder).toBe(EXAMPLES[1]);
    await type(input(), 'x');
    await act(async () => void vi.advanceTimersByTime(EXAMPLE_MS * 2));
    expect(input().placeholder).toBe(EXAMPLES[1]);
  });
});

describe('sending', () => {
  it('Send is off until something is written, and says why', async () => {
    await mount(rig());
    expect(send().disabled).toBe(true);
    await type(input(), 'fix the build');
    expect(send().disabled).toBe(false);
  });

  it('Enter starts the first ready agent with the message', async () => {
    const { onStart } = await mount(rig());
    await type(input(), '  fix the build ');
    const event = await key(input(), 'Enter');
    expect(event.defaultPrevented).toBe(true);
    expect(onStart).toHaveBeenCalledTimes(1);
    // The folder is the newest recent project; there is no copy and no chat.
    expect(onStart).toHaveBeenCalledWith({ agentId: 'alpha', accountId: undefined, folder: '/work/alpha-app', isolated: false, runAs: 'terminal', prompt: 'fix the build' });
  });

  it('Shift+Enter is a new line, not a send', async () => {
    const { onStart } = await mount(rig());
    await type(input(), 'line one');
    await key(input(), 'Enter', { shiftKey: true });
    expect(onStart).not.toHaveBeenCalled();
  });

  it('the Send button sends too, and Enter on an empty box does nothing', async () => {
    const { onStart } = await mount(rig());
    await key(input(), 'Enter');
    expect(onStart).not.toHaveBeenCalled();
    await type(input(), 'go');
    await click(send());
    expect(onStart).toHaveBeenCalledTimes(1);
  });

  it('remembers the agent that was used', async () => {
    const r = rig();
    const { onStart } = await mount(r);
    await click(byLabel(/^Agent:/));
    await click(byLabel('Beta'));
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].agentId).toBe('beta');
    await act(async () => root.unmount());
    root = createRoot(host);
    const second = await mount(r);
    await type(input(), 'again');
    await click(send());
    expect(second.onStart.mock.calls[0][0].agentId).toBe('beta');
  });
});

describe('the agent picker', () => {
  it('lists the agents with ready or not installed, and an agent that is not installed cannot send', async () => {
    await mount(rig());
    await click(byLabel(/^Agent:/));
    const menu = host.querySelector('[role=menu][aria-label^="Agent"]')!;
    expect(menu.textContent).toContain('Alpha');
    expect(menu.textContent).toContain('not installed');
    await click(byLabel('Gone'));
    await type(input(), 'go');
    expect(send().disabled).toBe(true);
    expect(host.textContent).toContain('Gone is not installed.');
  });

  it('draws no permission mode, model or effort control: the app has nothing behind them', async () => {
    await mount(rig());
    expect(host.textContent).not.toMatch(/full access|permission|effort|\bmodel\b/i);
  });
});

describe('the folder picker', () => {
  it('starts in the newest recent project and switches to another one', async () => {
    const { onStart } = await mount(rig());
    expect(byLabel(/^Folder:/)!.textContent).toContain('alpha-app');
    await click(byLabel(/^Folder:/));
    await click(byLabel('beta-site'));
    expect(byLabel(/^Folder:/)!.textContent).toContain('beta-site');
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].folder).toBe('/work/beta-site');
  });

  it('Choose folder opens the system dialog and uses what was picked', async () => {
    const { onStart } = await mount(rig({ picked: project('picked-one') }));
    await click(byLabel(/^Folder:/));
    await click(byLabel('Choose folder'));
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].folder).toBe('/work/picked-one');
  });

  it('starts from the folder of the selected pane when it has one', async () => {
    const { onStart } = await mount(rig(), { selectedCwd: '/work/open-pane' });
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].folder).toBe('/work/open-pane');
  });
});

describe('Terminal | Chat', () => {
  const lanes = (labsEnabled: boolean, lane: 'terminal' | 'acp' = 'terminal'): LaneView => ({ labsEnabled, agents: [{ id: 'alpha', lane, disabled: false }] });

  it('is not drawn without Labs chat, or for an agent with no chat way to run', async () => {
    await mount(rig({ lanes: lanes(false) }));
    expect(host.querySelector('[aria-label="Run as"]')).toBeNull();
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(rig({ lanes: lanes(true) }));
    expect(host.querySelector('[aria-label="Run as"]')).not.toBeNull();
    await click(byLabel(/^Agent:/));
    await click(byLabel('Beta'));
    expect(host.querySelector('[aria-label="Run as"]')).toBeNull();
  });

  it('starts on the agent\u2019s saved way and sends the chosen one', async () => {
    const { onStart } = await mount(rig({ lanes: lanes(true, 'acp') }));
    expect(byLabel('Chat')!.getAttribute('aria-checked')).toBe('true');
    await click(byLabel('Terminal'));
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].runAs).toBe('terminal');
  });

  it('a chat session needs a folder', async () => {
    await mount(rig({ lanes: lanes(true, 'acp'), recent: [] }));
    await type(input(), 'go');
    expect(send().disabled).toBe(true);
    expect(host.textContent).toContain('Pick a folder for a chat session.');
  });
});

describe('add files', () => {
  it('puts the chosen paths in the message', async () => {
    await mount(rig({ paths: ['/a/notes.md', 'C:\\My Docs\\x.txt'] }));
    await type(input(), 'Read these');
    await click(byLabel('Add to your message'));
    await click(byLabel('Add files'));
    expect(input().value).toBe('Read these\n/a/notes.md\n"C:\\My Docs\\x.txt"\n');
  });
});

describe('home cards', () => {
  it('shows no card until one is added, then Add card pins it and saves the list', async () => {
    const r = rig();
    await mount(r);
    expect(host.querySelector('.ck-na-hc')).toBeNull();
    expect(byLabel('Customize')).toBeUndefined();
    await click(byLabel('Add card'));
    await click(byLabel('Recent projects'));
    expect(host.querySelector('.ck-na-hc')!.textContent).toContain('alpha-app');
    expect(r.saveCards).toHaveBeenCalledTimes(1);
    const saved = r.saveCards.mock.calls[0][0] as HomeCard[];
    expect(saved.map((c) => c.kind)).toEqual(['recent']);
  });

  it('pins a saved prompt, and a click on it fills the box', async () => {
    const r = rig();
    await mount(r);
    await click(byLabel('Add card'));
    await click(byLabel('A saved prompt'));
    await type(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Saved prompt text"]')!, 'Write short commit messages.');
    await click(byLabel(/^Pin$/));
    expect((r.saveCards.mock.calls[0][0] as HomeCard[])[0]).toMatchObject({ kind: 'prompt', text: 'Write short commit messages.' });
    await click(host.querySelector<HTMLElement>('.ck-na-prompt')!);
    expect(input().value).toBe('Write short commit messages.');
  });

  it('Customize shows remove and move buttons; Remove drops the card and saves', async () => {
    const r = rig({
      cards: [
        { id: 'a', kind: 'recent' },
        { id: 'b', kind: 'running' },
        { id: 'c', kind: 'needs' },
      ],
    });
    await mount(r);
    expect(byLabel('Remove Running agents')).toBeUndefined();
    await click(byLabel('Customize'));
    expect(byLabel('Done')).toBeDefined();
    await click(byLabel('Remove Running agents'));
    expect(host.querySelectorAll('.ck-na-hc')).toHaveLength(2);
    expect((r.saveCards.mock.calls[0][0] as HomeCard[]).map((c) => c.id)).toEqual(['a', 'c']);
    await click(byLabel('Move Needs you up'));
    expect((r.saveCards.mock.calls[1][0] as HomeCard[]).map((c) => c.id)).toEqual(['c', 'a']);
    await click(byLabel('Done'));
    expect(byLabel('Remove Needs you')).toBeUndefined();
  });

  it('the running and needs-you cards open the pane that is clicked', async () => {
    const { onOpenPane } = await mount(
      rig({
        cards: [
          { id: 'b', kind: 'running' },
          { id: 'c', kind: 'needs' },
        ],
      }),
    );
    const lines = [...host.querySelectorAll<HTMLElement>('.ck-na-line')];
    expect(lines.map((l) => l.textContent)).toEqual(['Alpha 1', 'Beta 2']);
    await click(lines[1]);
    expect(onOpenPane).toHaveBeenCalledWith('p3');
  });

  it('the recent card sets the folder', async () => {
    const { onStart } = await mount(rig({ cards: [{ id: 'a', kind: 'recent' }] }));
    await click([...host.querySelectorAll<HTMLElement>('.ck-na-line')].find((l) => l.textContent?.includes('beta-site')));
    await type(input(), 'go');
    await click(send());
    expect(onStart.mock.calls[0][0].folder).toBe('/work/beta-site');
  });

  it('a card list that changes in Settings shows up here', async () => {
    const r = rig();
    await mount(r);
    await act(async () => r.emit({ cards: [{ id: 'z', kind: 'needs' }] }));
    expect(host.querySelector('.ck-na-hc')!.textContent).toContain('Needs you');
  });
});
