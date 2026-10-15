// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AskbarApi, AskNotice } from '../../lib/askbar';
import { AskNotices } from './AskNotices';

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

const notice = (over: Partial<AskNotice> = {}): AskNotice => ({
  id: 'n1',
  agent: 'codex',
  agent_label: 'Codex',
  question: 'How do I share a cache between threads?',
  at_ms: 1,
  result: {
    ask_id: 'a1',
    agent: 'codex',
    agent_label: 'Codex',
    question: 'How do I share a cache between threads?',
    ok: true,
    answer: 'Wrap the map in an Arc<Mutex<...>>.',
    error: null,
  },
  ...over,
});

function fakeApi(initial: AskNotice[] = []) {
  let onNotice: (n: AskNotice) => void = () => {};
  const api = {
    notices: vi.fn(async () => initial),
    dismissNotice: vi.fn(async () => undefined),
    openNotice: vi.fn(async () => undefined),
    onNotice: vi.fn(async (cb: (n: AskNotice) => void) => {
      onNotice = cb;
      return () => {};
    }),
    onNoticeRemoved: vi.fn(async () => () => {}),
  } as unknown as AskbarApi & {
    notices: ReturnType<typeof vi.fn>;
    dismissNotice: ReturnType<typeof vi.fn>;
    openNotice: ReturnType<typeof vi.fn>;
  };
  return { api, deliver: (n: AskNotice) => onNotice(n) };
}

const mount = async (api: AskbarApi) => {
  await act(async () => {
    root.render(<AskNotices api={api} />);
  });
};
const button = (text: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;

describe('AskNotices', () => {
  it('renders nothing when there are no notices', async () => {
    const { api } = fakeApi([]);
    await mount(api);
    expect(host.querySelector('.ib-list')).toBeNull();
    expect(host.innerHTML).toBe('');
  });

  it('shows an answer card with the agent, the question and a preview', async () => {
    const { api } = fakeApi([notice()]);
    await mount(api);
    const card = host.querySelector('[data-ib-card][data-kind="answer"]')!;
    expect(host.querySelector('[aria-label="Answers from your agents"]')).not.toBeNull();
    expect(card.querySelector('.ib-title')!.textContent).toBe('Answer from Codex');
    expect(card.querySelector('.ib-reason')!.textContent).toBe('How do I share a cache between threads?');
    expect(card.querySelector('.ib-clamp')!.textContent).toBe('Wrap the map in an Arc<Mutex<...>>.');
    expect(Array.from(card.querySelectorAll('button')).map((b) => b.textContent)).toEqual(['Open', 'Dismiss']);
  });

  it('shows the error message instead of the answer when the ask failed', async () => {
    const failed = notice({
      result: {
        ask_id: 'a1',
        agent: 'codex',
        agent_label: 'Codex',
        question: 'q',
        ok: false,
        answer: null,
        error: { code: 'timeout', message: 'Codex did not answer in time.' },
      },
    });
    const { api } = fakeApi([failed]);
    await mount(api);
    expect(host.querySelector('.ib-clamp')!.textContent).toBe('Codex did not answer in time.');
  });

  it('Open calls openNotice with the notice id', async () => {
    const { api } = fakeApi([notice()]);
    await mount(api);
    await act(async () => button('Open')!.click());
    expect(api.openNotice).toHaveBeenCalledWith('n1');
  });

  it('Dismiss calls dismissNotice and removes the card', async () => {
    const { api } = fakeApi([notice()]);
    await mount(api);
    await act(async () => button('Dismiss')!.click());
    expect(api.dismissNotice).toHaveBeenCalledWith('n1');
    expect(host.querySelector('[data-ib-card]')).toBeNull();
    expect(host.innerHTML).toBe('');
  });

  it('shows a notice that is delivered later through onNotice', async () => {
    const { api, deliver } = fakeApi([]);
    await mount(api);
    expect(host.querySelector('[data-ib-card]')).toBeNull();
    await act(async () => deliver(notice({ id: 'n2', agent_label: 'Cursor Agent' })));
    expect(host.querySelector('.ib-title')!.textContent).toBe('Answer from Cursor Agent');
  });
});
