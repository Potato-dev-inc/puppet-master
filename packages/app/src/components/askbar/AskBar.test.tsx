// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import type { AskbarAgent, AskbarApi, AskbarClipboard, AskbarSettings, AskRequest, AskResult } from '../../lib/askbar';
import { AskBar } from './AskBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Fake = { [K in keyof AskbarApi]: Mock<AskbarApi[K]> };

const AGENTS: AskbarAgent[] = [
  { id: 'claude', label: 'Claude Code', ready: true },
  { id: 'kilo', label: 'Kilo', ready: false },
  { id: 'codex', label: 'Codex', ready: false },
];

const SETTINGS: AskbarSettings = {
  enabled: true,
  shortcut: 'Alt+Space',
  default_agent: 'claude',
  last_agent: null,
  attach_clipboard_default: false,
};

function fakeApi(): Fake {
  return {
    settingsGet: vi.fn().mockResolvedValue(SETTINGS),
    settingsSet: vi.fn().mockResolvedValue(SETTINGS),
    agents: vi.fn().mockResolvedValue(AGENTS),
    clipboard: vi.fn().mockResolvedValue(null),
    ask: vi.fn(),
    hide: vi.fn().mockResolvedValue(undefined),
    resize: vi.fn().mockResolvedValue(undefined),
    continueInPane: vi.fn().mockResolvedValue(undefined),
    notices: vi.fn().mockResolvedValue([]),
    dismissNotice: vi.fn().mockResolvedValue(undefined),
    openNotice: vi.fn().mockResolvedValue(undefined),
    onShown: vi.fn().mockResolvedValue(() => {}),
    onShowResult: vi.fn().mockResolvedValue(() => {}),
    onNotice: vi.fn().mockResolvedValue(() => {}),
    onNoticeRemoved: vi.fn().mockResolvedValue(() => {}),
  } as unknown as Fake;
}

const okResult = (req: AskRequest, answer = 'Answer.'): AskResult => ({
  ask_id: req.ask_id,
  agent: req.agent,
  agent_label: AGENTS.find((a) => a.id === req.agent)?.label ?? req.agent,
  question: req.question,
  ok: true,
  answer,
  error: null,
});

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

async function renderBar(api: Fake) {
  await act(async () => {
    root.render(<AskBar api={api as unknown as AskbarApi} />);
  });
}

const flush = () => act(async () => {});
const input = () => host.querySelector('input.askbar-input') as HTMLInputElement;
const chipLabel = () => host.querySelector('.askbar-chip-label')!.textContent;
const buttonByText = (text: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;

/** Set the value the way React sees a user edit, then fire the input event. */
function typeQuestion(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function pressEnter() {
  act(() => {
    input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  });
}

async function ask(api: Fake, text: string) {
  typeQuestion(text);
  pressEnter();
  await flush();
}

describe('ask bar', () => {
  it('starts empty with the default agent in the chip', async () => {
    const api = fakeApi();
    await renderBar(api);
    expect(host.querySelector('.askbar')!.getAttribute('data-state')).toBe('empty');
    expect(input().placeholder).toBe('Ask anything…');
    expect(chipLabel()).toBe('Claude Code');
    expect(api.resize).toHaveBeenCalledWith(false);
  });

  it('lists only the agents from the api and marks the ready ones, and choosing one sets the agent for the next ask', async () => {
    const api = fakeApi();
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req));
    await renderBar(api);

    act(() => (host.querySelector('.askbar-chip') as HTMLButtonElement).click());
    const rows = Array.from(host.querySelectorAll('.askbar-menu-row'));
    expect(rows.map((r) => r.querySelector('.askbar-menu-label')!.textContent)).toEqual(['Claude Code', 'Kilo', 'Codex']);
    expect(rows.map((r) => r.querySelector('.askbar-menu-ready')?.textContent ?? null)).toEqual(['Ready', null, null]);

    act(() => (rows[2] as HTMLButtonElement).click());
    expect(host.querySelector('.askbar-menu')).toBeNull();
    expect(chipLabel()).toBe('Codex');

    await ask(api, 'Why is the build red?');
    expect(api.ask).toHaveBeenCalledTimes(1);
    expect(api.ask.mock.calls[0][0]).toMatchObject({ agent: 'codex', question: 'Why is the build red?' });
  });

  it('the clipboard is off by default: nothing is read on open and the chip offers "+ Clipboard"', async () => {
    const api = fakeApi();
    api.clipboard.mockResolvedValue({ text: 'password=hunter2', chars: 16, truncated: false });
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req));
    await renderBar(api);

    expect(api.clipboard).not.toHaveBeenCalled();
    expect(host.querySelector('.askbar-clip-wrap')).toBeNull();
    expect(buttonByText('+ Clipboard')).toBeDefined();

    await ask(api, 'Why is the build red?');
    expect(api.clipboard).not.toHaveBeenCalled();
    expect(api.ask.mock.calls[0][0].clipboard).toBeNull();
  });

  it('clicking "+ Clipboard" reads the clipboard and attaches it; the × detaches it and the next ask sends nothing', async () => {
    const api = fakeApi();
    const clip: AskbarClipboard = { text: 'x'.repeat(1200), chars: 1200, truncated: false };
    api.clipboard.mockResolvedValue(clip);
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req));
    await renderBar(api);

    await act(async () => buttonByText('+ Clipboard')!.click());
    expect(api.clipboard).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.askbar-clip')!.textContent).toBe('Clipboard · 1.2k chars');
    await ask(api, 'Summarise it');
    expect(api.ask.mock.calls[0][0].clipboard).toBe(clip.text);

    act(() => (host.querySelector('button[aria-label="Detach clipboard"]') as HTMLButtonElement).click());
    expect(host.querySelector('.askbar-clip-wrap')).toBeNull();
    expect(buttonByText('+ Clipboard')).toBeDefined();
    await ask(api, 'And the second part?');
    expect(api.ask.mock.calls[1][0].clipboard).toBeNull();
    expect(api.clipboard).toHaveBeenCalledTimes(1);
  });

  it('attaching by default (Settings) reads the clipboard on open and attaches it', async () => {
    const api = fakeApi();
    api.settingsGet.mockResolvedValue({ ...SETTINGS, attach_clipboard_default: true });
    api.clipboard.mockResolvedValue({ text: 'log line', chars: 8, truncated: false });
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req));
    await renderBar(api);

    expect(api.clipboard).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.askbar-clip')!.textContent).toBe('Clipboard · 8 chars');
    await ask(api, 'Why?');
    expect(api.ask.mock.calls[0][0].clipboard).toBe('log line');
  });

  it('the default-on attach does not persist across opens once the user detaches', async () => {
    const api = fakeApi();
    api.settingsGet.mockResolvedValue({ ...SETTINGS, attach_clipboard_default: true });
    api.clipboard.mockResolvedValue({ text: 'log line', chars: 8, truncated: false });
    await renderBar(api);

    act(() => (host.querySelector('button[aria-label="Detach clipboard"]') as HTMLButtonElement).click());
    expect(buttonByText('+ Clipboard')).toBeDefined();

    // The next open starts from the setting again (on), not from this detach.
    const onShown = api.onShown.mock.calls[0][0] as () => void;
    await act(async () => onShown());
    await flush();
    expect(host.querySelector('.askbar-clip')!.textContent).toBe('Clipboard · 8 chars');
  });

  it('an empty clipboard attaches nothing and the chip says so', async () => {
    const api = fakeApi();
    api.clipboard.mockResolvedValue(null);
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req));
    await renderBar(api);

    await act(async () => buttonByText('+ Clipboard')!.click());
    expect(buttonByText('Clipboard empty')).toBeDefined();
    expect(host.querySelector('.askbar-clip-wrap')).toBeNull();
    await ask(api, 'Anything?');
    expect(api.ask.mock.calls[0][0].clipboard).toBeNull();
  });

  it('labels a cut clip as "8k of 52k chars"', async () => {
    const api = fakeApi();
    api.clipboard.mockResolvedValue({ text: 'y'.repeat(8000), chars: 52000, truncated: true });
    await renderBar(api);
    await act(async () => buttonByText('+ Clipboard')!.click());
    expect(host.querySelector('.askbar-clip')!.textContent).toBe('Clipboard · 8k of 52k chars');
  });

  it('shows the working spinner while the ask runs and calls the api once', async () => {
    const api = fakeApi();
    api.ask.mockReturnValue(new Promise(() => {}));
    await renderBar(api);

    await ask(api, 'What changed in main?');
    expect(host.querySelector('.askbar')!.getAttribute('data-state')).toBe('running');
    expect(host.querySelector('[role="img"][aria-label="Working"]')).not.toBeNull();
    expect(host.textContent).toContain('Asking Claude Code…');
    expect(api.ask).toHaveBeenCalledTimes(1);
    expect(api.ask.mock.calls[0][0]).toMatchObject({ question: 'What changed in main?', agent: 'claude', clipboard: null });
    expect(api.resize).toHaveBeenLastCalledWith(true);
  });

  it('renders the answer with prose and a code block, and offers Copy, Continue in a pane and Close', async () => {
    const api = fakeApi();
    const answer = 'Run the check first:\n```ts\nconst ok = true;\n```\nThen commit.';
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req, answer));
    await renderBar(api);

    await ask(api, 'How do I verify?');
    expect(host.querySelector('.askbar')!.getAttribute('data-state')).toBe('answer');
    expect(Array.from(host.querySelectorAll('p.askbar-text')).map((p) => p.textContent)).toEqual(['Run the check first:', 'Then commit.']);
    expect(host.querySelector('pre.askbar-code')!.textContent).toBe('const ok = true;');
    expect(buttonByText('Copy')).toBeDefined();
    expect(buttonByText('Continue in a pane')).toBeDefined();
    expect(buttonByText('Close')).toBeDefined();
  });

  it('Continue in a pane hands the question to the pane and then hides the bar', async () => {
    const api = fakeApi();
    const clip: AskbarClipboard = { text: 'log line', chars: 8, truncated: false };
    api.clipboard.mockResolvedValue(clip);
    api.ask.mockImplementation(async (req: AskRequest) => okResult(req, 'Done.'));
    await renderBar(api);

    await act(async () => buttonByText('+ Clipboard')!.click());
    await ask(api, 'Why?');
    await act(async () => {
      buttonByText('Continue in a pane')!.click();
    });
    await flush();
    expect(api.continueInPane).toHaveBeenCalledWith('claude', 'Why?', 'log line');
    expect(api.hide).toHaveBeenCalled();
    expect(api.continueInPane.mock.invocationCallOrder[0]).toBeLessThan(api.hide.mock.invocationCallOrder[0]);
  });

  it('shows the error message and Try again when the ask fails, and Try again returns to the empty bar', async () => {
    const api = fakeApi();
    api.ask.mockImplementation(async (req: AskRequest) => ({
      ...okResult(req),
      ok: false,
      answer: null,
      error: { code: 'timeout', message: 'The agent did not answer in time.' },
    }));
    await renderBar(api);

    await ask(api, 'Anything there?');
    expect(host.querySelector('.askbar')!.getAttribute('data-state')).toBe('error');
    expect(host.querySelector('.askbar-error')!.textContent).toBe('The agent did not answer in time.');
    expect(buttonByText('Try again')).toBeDefined();

    act(() => buttonByText('Try again')!.click());
    expect(host.querySelector('.askbar')!.getAttribute('data-state')).toBe('empty');
    expect(input().value).toBe('Anything there?');
  });

  it('Esc closes the agent menu first and then hides the bar', async () => {
    const api = fakeApi();
    await renderBar(api);

    act(() => (host.querySelector('.askbar-chip') as HTMLButtonElement).click());
    expect(host.querySelector('.askbar-menu')).not.toBeNull();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(host.querySelector('.askbar-menu')).toBeNull();
    expect(api.hide).not.toHaveBeenCalled();

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(api.hide).toHaveBeenCalledTimes(1);
  });
});
