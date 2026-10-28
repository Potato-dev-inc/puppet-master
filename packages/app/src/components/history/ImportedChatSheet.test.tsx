// @vitest-environment jsdom
// The old-chat viewer: the chat loads from the backend and shows read-only (no message box) with a banner that says
// it is old; a missing source says so plainly; "Continue in a pane" is off, with the reason, when the agent has no
// resume option; picking it hands the plan up; a search hit scrolls to its message; Escape closes it.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImportApi, ImportedChatRequest, OpenedImportedChat, ResumePlan } from '../../lib/chatImport';
import type { ChatItem } from '../../lib/sessionChatModel';
import { bannerText, chatDate, ImportedChatSheet } from './ImportedChatSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// The chat view measures itself with a ResizeObserver, which jsdom does not have.
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const UPDATED = new Date(2026, 7, 14, 10, 0).getTime();
const META: OpenedImportedChat['meta'] = { title: 'Stripe webhook fix', agent: 'Codex', folder: 'shop', updatedMs: UPDATED, cutShort: false };
const ITEMS: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '50:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '60:0', at: null, outcome: 'completed', durationMs: 1000 },
  { kind: 'user', id: '70:0', at: null, text: 'Now the webhook' },
  { kind: 'assistant', id: '90:0', at: null, text: 'Signature check added.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '99:0', at: null, outcome: 'completed', durationMs: 1000 },
];
const PLAN_OK: ResumePlan = { ok: true, agentType: 'codex', cwd: 'C:/Users/you/shop', extraArgs: ['resume', 'sess-0001-abcd'] };
const REQUEST = { agent: 'codex', session: 'sess-0001-abcd', nonce: 1 };

type Api = Pick<ImportApi, 'open' | 'resumePlan'>;

function fakeApi(over: Partial<Api> = {}): Api {
  return {
    open: vi.fn(async () => ({ meta: META, items: ITEMS })),
    resumePlan: vi.fn(async () => PLAN_OK),
    ...over,
  };
}

let host: HTMLDivElement;
let root: Root;
let scrolled: HTMLElement[];

beforeEach(() => {
  scrolled = [];
  Element.prototype.scrollIntoView = function (this: HTMLElement) {
    scrolled.push(this);
  };
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

function render(props: { api: Api; request?: ImportedChatRequest; onContinue?: (plan: ResumePlan) => void; onClose?: () => void }) {
  act(() =>
    root.render(
      <ImportedChatSheet
        request={props.request ?? REQUEST}
        api={props.api}
        onContinue={props.onContinue ?? vi.fn()}
        onClose={props.onClose ?? vi.fn()}
      />,
    ),
  );
}

function continueButton(): HTMLButtonElement {
  const found = [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Continue in a pane');
  if (!found) throw new Error('no Continue in a pane button');
  return found;
}

describe('ImportedChatSheet', () => {
  it('reads the chat by agent and session, shows its turns with a read-only banner, and has no message box', async () => {
    const api = fakeApi();
    render({ api });
    expect(host.textContent).toContain('Loading the chat…');
    await flush();
    expect(api.open).toHaveBeenCalledWith('codex', 'sess-0001-abcd');
    expect(host.querySelector('h2')?.textContent).toBe('Stripe webhook fix');
    expect(host.textContent).toContain('List the files');
    expect(host.textContent).toContain('Two entries.');
    expect(host.querySelector('[data-testid="session-chat"]')).not.toBeNull();
    expect(host.querySelector('textarea')).toBeNull();
    expect(host.querySelector('.history-viewer-banner')?.textContent).toMatch(/^Old Codex chat from 14 Aug( 2026)?\. Read-only\.$/);
  });

  it('shows the plain sentence when the original chat file is gone, and Continue stays off', async () => {
    const sentence = 'The original chat file is gone. Search still has its text.';
    const api = fakeApi({ open: vi.fn(async () => Promise.reject(new Error(sentence))) });
    render({ api });
    await flush();
    expect(host.textContent).toContain('Chat unavailable');
    expect(host.textContent).toContain(sentence);
    expect(host.querySelectorAll('[data-turn-id]')).toHaveLength(0);
    expect(continueButton().disabled).toBe(true);
  });

  it('disables Continue and gives the reason when the agent has no resume option', async () => {
    const reason = 'This agent has no resume option.';
    const api = fakeApi({ resumePlan: vi.fn(async () => ({ ok: false, reason, agentType: 'codex', cwd: '', extraArgs: [] })) });
    render({ api });
    await flush();
    expect(continueButton().disabled).toBe(true);
    expect(continueButton().title).toBe(reason);
    expect(host.querySelector('.history-viewer-reason')?.textContent).toBe(reason);
  });

  it('hands the plan to onContinue when Continue is picked', async () => {
    const onContinue = vi.fn();
    const api = fakeApi();
    render({ api, onContinue });
    await flush();
    expect(continueButton().disabled).toBe(false);
    act(() => continueButton().click());
    expect(onContinue).toHaveBeenCalledTimes(1);
    expect(onContinue).toHaveBeenCalledWith(PLAN_OK);
  });

  it('scrolls to the turn that holds the search hit message', async () => {
    const api = fakeApi();
    render({ api, request: { ...REQUEST, itemId: '90:0' } });
    await flush();
    const turns = host.querySelectorAll<HTMLElement>('[data-turn-id]');
    expect(scrolled).toEqual([turns[1]]);
  });

  it('closes on Escape', async () => {
    const onClose = vi.fn();
    render({ api: fakeApi(), onClose });
    await flush();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('bannerText and chatDate', () => {
  it('omits the year in the same year and adds it in another year', () => {
    const sameYear = new Date(2026, 9, 28).getTime();
    const nextYear = new Date(2027, 0, 5).getTime();
    expect(chatDate(UPDATED, sameYear)).toBe('14 Aug');
    expect(chatDate(UPDATED, nextYear)).toBe('14 Aug 2026');
    expect(bannerText('Codex', UPDATED, sameYear)).toBe('Old Codex chat from 14 Aug. Read-only.');
  });
});
