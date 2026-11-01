// @vitest-environment jsdom
// The message list: the bot's words as Markdown, day and time lines, the "New" line and Jump to latest, the hover
// toolbar (Copy, Reply into the composer), and the folded tool-activity line that never shows raw JSON in a bubble.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type PotatoMessage } from '../../../lib/potato';
import { POTATO_QUOTE_EVENT } from '../../../lib/potato-thread';
import { Composer } from './Composer';
import { Transcript, type TranscriptProps } from './Transcript';
import { threadItems } from './model';
import { bot, buttonByText } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let writeText: ReturnType<typeof vi.fn>;
beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const NOW = new Date(2026, 10, 1, 15, 0).getTime();
const at = (h: number, mi = 0, day = 1) => new Date(2026, 10, day, h, mi).getTime();
let n = 0;
const bubble = (text: string, when: number, from: 'bot' | 'you' = 'bot'): PotatoMessage => ({
  id: `t-${++n}`,
  at: when,
  from: { kind: from, name: from === 'bot' ? 'Fish' : undefined },
  kind: 'text',
  text,
});

function show(messages: PotatoMessage[], over: Partial<TranscriptProps> = {}) {
  const props: TranscriptProps = {
    bot: bot(),
    messages,
    agentLabel: 'Claude Code',
    transport: fakePotatoTransport({ bots: [bot()] }),
    onChanged: () => {},
    now: NOW,
    openLink: () => {},
    ...over,
  };
  act(() => root.render(<Transcript {...props} />));
  return props;
}
const labels = (sel: string) => [...host.querySelectorAll(sel)].map((el) => el.textContent);
const click = (el: Element | null | undefined) => {
  act(() => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
};

describe('Transcript, Markdown', () => {
  it('draws the bot as Markdown (code with Copy, a checklist, bold) and yours as typed', () => {
    show([
      bubble('**Done.**\n\n- ✓ **Archived** → 26 newsletters\n\n```sh\nnpm test\n```', at(12)),
      bubble('**not bold** <b>x</b>', at(12, 1), 'you'),
    ]);
    const [botBubble, yourBubble] = [...host.querySelectorAll('.ck-potato-bubble')];
    expect(botBubble.querySelector('strong')?.textContent).toBe('Done.');
    expect(botBubble.querySelector('li.ck-chat-md-task')).not.toBeNull();
    expect(botBubble.querySelector('button.ck-chat-md-copy')).not.toBeNull();
    expect(yourBubble.textContent).toBe('**not bold** <b>x</b>');
    expect(yourBubble.querySelector('strong, b')).toBeNull();
  });

  it('opens a web link in the opener it was given, once', () => {
    const openLink = vi.fn();
    show([bubble('Read [this](https://example.com/x) now', at(12))], { openLink });
    click(host.querySelector('a'));
    expect(openLink).toHaveBeenCalledExactlyOnceWith('https://example.com/x');
  });

  it('shows no raw HTML from the bot as elements', () => {
    show([bubble('<img src=x onerror=alert(1)> <script>alert(2)</script>', at(12))]);
    expect(host.querySelector('img, script')).toBeNull();
    expect(host.textContent).toContain('<script>alert(2)</script>');
  });
});

describe('Transcript, separators', () => {
  it('puts a day line and a time line at the top, and a time line after a pause', () => {
    show([bubble('one', at(0, 11)), bubble('two', at(0, 12)), bubble('three', at(9, 30))]);
    expect(labels('.ck-potato-sep--day')).toEqual(['Today']);
    expect(labels('.ck-potato-sep--time')).toEqual(['12:11 AM', '9:30 AM']);
    // the order in the list: day, time, one, two, time, three
    const order = [...host.querySelectorAll('.ck-potato-rows > li')].map((li) => li.textContent?.replace(/(Copy|Reply)/g, '').trim());
    expect(order).toEqual(['Today', '12:11 AM', 'one', 'two', '9:30 AM', 'three']);
  });

  it('names yesterday when the first message is from yesterday', () => {
    const yesterday = new Date(2026, 9, 31, 22, 0).getTime();
    show([bubble('old', yesterday), bubble('new', at(8, 5))]);
    expect(labels('.ck-potato-sep--day')).toEqual(['Yesterday', 'Today']);
  });

  it('adds no line between messages that are close together', () => {
    show([bubble('a', at(10, 0)), bubble('b', at(10, 3)), bubble('c', at(10, 14))]);
    expect(host.querySelectorAll('.ck-potato-sep--time')).toHaveLength(1);
  });

  it('starts a new run of one sender after a line', () => {
    const items = threadItems([bubble('a', at(10, 0)), bubble('b', at(10, 1)), bubble('c', at(11, 30))], { now: NOW });
    const firsts = items.filter((i) => i.type === 'bubble').map((i) => (i.type === 'bubble' ? i.first : null));
    expect(firsts).toEqual([true, false, true]);
  });
});

describe('Transcript, New line and Jump to latest', () => {
  const thread = () => [bubble('old', at(10, 0)), bubble('your question', at(10, 1), 'you'), bubble('reply one', at(10, 2)), bubble('reply two', at(10, 3))];

  it('puts the New line above the first unread reply, and keeps it after the thread is read', () => {
    show(thread(), { bot: bot({ unread: 2 }) });
    const rows = [...host.querySelectorAll('.ck-potato-rows > li')];
    const at1 = rows.findIndex((li) => li.classList.contains('ck-potato-sep--unread'));
    expect(at1).toBeGreaterThan(-1);
    expect(rows[at1 + 1].textContent).toContain('reply one');
    // opening the thread read it: unread is 0 now, the line stays
    show(thread(), { bot: bot({ unread: 0 }) });
    expect(host.querySelectorAll('.ck-potato-sep--unread')).toHaveLength(1);
  });

  it('has no New line when nothing was unread, and takes it away once you send', () => {
    show(thread());
    expect(host.querySelector('.ck-potato-sep--unread')).toBeNull();
    act(() => root.render(null));
    show(thread(), { bot: bot({ unread: 1 }) });
    expect(host.querySelector('.ck-potato-sep--unread')).not.toBeNull();
    show(thread(), { bot: bot({ unread: 0 }), followToken: 1 });
    expect(host.querySelector('.ck-potato-sep--unread')).toBeNull();
  });

  it('shows Jump to latest when you have scrolled up, and a click brings the view to the end', () => {
    show(thread());
    const scroller = host.querySelector<HTMLElement>('.ck-potato-body')!;
    Object.defineProperty(scroller, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(scroller, 'clientHeight', { value: 400, configurable: true });
    expect(host.querySelector('.ck-potato-jump')).toBeNull();

    scroller.scrollTop = 100;
    act(() => {
      scroller.dispatchEvent(new Event('scroll', { bubbles: true }));
    });
    const jump = host.querySelector('.ck-potato-jump');
    expect(jump?.textContent).toBe('Jump to latest');

    click(jump);
    expect(host.querySelector('.ck-potato-jump')).toBeNull();
  });

  it('shows no Jump button while you are at the end', () => {
    show(thread());
    const scroller = host.querySelector<HTMLElement>('.ck-potato-body')!;
    Object.defineProperty(scroller, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(scroller, 'clientHeight', { value: 400, configurable: true });
    scroller.scrollTop = 1560;
    act(() => {
      scroller.dispatchEvent(new Event('scroll', { bubbles: true }));
    });
    expect(host.querySelector('.ck-potato-jump')).toBeNull();
  });
});

describe('Transcript, hover actions', () => {
  it('has Copy and Reply on every bubble', () => {
    show([bubble('hello', at(10)), bubble('hi', at(10, 1), 'you')]);
    expect(host.querySelectorAll('.ck-potato-actions')).toHaveLength(2);
    expect(host.querySelector('.ck-potato-actions')?.getAttribute('role')).toBe('toolbar');
  });

  it('Copy puts the message text (as written) on the clipboard and says Copied', async () => {
    vi.useFakeTimers();
    try {
      show([bubble('**bold** and `code`', at(10))]);
      const copy = host.querySelector<HTMLButtonElement>('button[aria-label="Copy message"]')!;
      await act(async () => {
        copy.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(writeText).toHaveBeenCalledExactlyOnceWith('**bold** and `code`');
      expect(copy.textContent).toBe('Copied');
      await act(async () => {
        vi.advanceTimersByTime(1600);
      });
      expect(copy.textContent).toBe('Copy');
    } finally {
      vi.useRealTimers();
    }
  });

  it('Copy stays Copy when the clipboard says no', async () => {
    writeText.mockRejectedValueOnce(new Error('denied'));
    show([bubble('hello', at(10))]);
    const copy = host.querySelector<HTMLButtonElement>('button[aria-label="Copy message"]')!;
    await act(async () => {
      copy.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(copy.textContent).toBe('Copy');
  });

  it('Reply sends a quote event for this bot', () => {
    const seen: Array<{ botId: string; text: string }> = [];
    const on = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener(POTATO_QUOTE_EVENT, on);
    try {
      show([bubble('line one\nline two', at(10))]);
      click(host.querySelector('button[aria-label="Reply to message"]'));
    } finally {
      window.removeEventListener(POTATO_QUOTE_EVENT, on);
    }
    expect(seen).toEqual([{ botId: 'fish', text: 'line one\nline two' }]);
  });

  it('the composer puts the quote under what is typed, and ignores another bot', () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    act(() => root.render(<Composer bot={bot()} transport={transport} onChanged={() => {}} onNotify={() => {}} attach={null} />));
    const field = host.querySelector('textarea')!;
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, 'my answer');
    act(() => {
      field.dispatchEvent(new Event('input', { bubbles: true }));
    });
    act(() => {
      window.dispatchEvent(new CustomEvent(POTATO_QUOTE_EVENT, { detail: { botId: 'other', text: 'no' } }));
    });
    expect(field.value).toBe('my answer');
    act(() => {
      window.dispatchEvent(new CustomEvent(POTATO_QUOTE_EVENT, { detail: { botId: 'fish', text: 'hi' } }));
    });
    expect(field.value).toBe('my answer\n\n> hi\n\n');
    expect(buttonByText(host, 'Send')).toBeDefined();
  });
});

describe('Transcript, tool activity', () => {
  const tools = () => [
    bubble('on it', at(10, 0)),
    bubble('Fish used computer_observe: ok, saw Fake desktop', at(10, 0)),
    bubble('Fish used computer_act: ok', at(10, 0)),
    bubble('Fish used ask_person_for_screen: ok, saw {"granted":true,"reason":"a code","lease":{"id":"x1","ttl":30}}', at(10, 0)),
    bubble('All done.', at(10, 1)),
  ];

  it('folds a run of tool lines into one line that starts closed', () => {
    show(tools());
    const lines = labels('.ck-potato-bubble');
    expect(lines).toEqual(['on it', 'All done.']);
    expect(host.textContent).not.toContain('Fish used');
    const head = host.querySelector('.ck-potato-activity-head')!;
    expect(head.textContent).toBe('Used the computer · 3 steps');
    expect(head.getAttribute('aria-expanded')).toBe('false');
    expect(host.querySelector('.ck-potato-steps')).toBeNull();
  });

  it('opens to compact rows with words, and closes again', () => {
    show(tools());
    click(host.querySelector('.ck-potato-activity-head'));
    const rows = [...host.querySelectorAll('.ck-potato-step')];
    expect(rows.map((r) => r.querySelector('.ck-potato-step-label')?.textContent)).toEqual(['Looked at the screen', 'Used the mouse and keyboard', 'Asked you for the screen']);
    expect(rows[0].querySelector('.ck-potato-step-sum')?.textContent).toBe('Fake desktop');
    expect(host.querySelector('.ck-potato-activity-head')?.getAttribute('aria-expanded')).toBe('true');
    click(host.querySelector('.ck-potato-activity-head'));
    expect(host.querySelector('.ck-potato-steps')).toBeNull();
  });

  it('never shows raw JSON in a row: it sits behind Details, and only after you open it', () => {
    show(tools());
    click(host.querySelector('.ck-potato-activity-head'));
    const rows = [...host.querySelectorAll('.ck-potato-step')];
    for (const row of rows) expect(row.textContent).not.toMatch(/[{}]/);
    expect(host.querySelector('.ck-potato-step-raw')).toBeNull();
    const details = buttonByText(rows[2] as HTMLElement, 'Details')!;
    expect(details).toBeDefined();
    expect(buttonByText(rows[0] as HTMLElement, 'Details')).toBeUndefined();
    click(details);
    expect(host.querySelector('.ck-potato-step-raw')?.textContent).toContain('"granted": true');
    click(buttonByText(rows[2] as HTMLElement, 'Hide details'));
    expect(host.querySelector('.ck-potato-step-raw')).toBeNull();
  });

  it('shows a bubble that is only JSON as a "Got a result" step, not as words', () => {
    show([bubble('{"ok":true,"result":{"url":"https://example.com"}}', at(10))]);
    expect(host.querySelector('.ck-potato-bubble')).toBeNull();
    expect(host.querySelector('.ck-potato-activity-head')?.textContent).toBe('Got a result');
    expect(host.textContent).not.toContain('{"ok"');
  });

  it('says Needs you and marks a failed step', () => {
    show([bubble('Fish used computer_act: failed LEASE_LOST (needs you)', at(10))]);
    expect(host.querySelector('.ck-potato-activity-head')?.textContent).toContain('Needs you');
    click(host.querySelector('.ck-potato-activity-head'));
    expect(host.querySelector('.ck-potato-step.is-failed')?.textContent).toContain('The screen changed hands.');
  });

  it('starts a second activity line after a message in between', () => {
    show([bubble('Fish used computer_observe: ok', at(10)), bubble('looking good', at(10)), bubble('Fish used computer_act: ok', at(10))]);
    expect(host.querySelectorAll('.ck-potato-activity')).toHaveLength(2);
  });

  it('does not fold your own message that looks like a tool line', () => {
    show([bubble('Fish used computer_observe: ok', at(10), 'you')]);
    expect(host.querySelector('.ck-potato-activity')).toBeNull();
    expect(host.querySelector('.ck-potato-bubble--you')).not.toBeNull();
  });
});

describe('Transcript, empty', () => {
  it('says hello to the bot when there is nothing yet', () => {
    show([]);
    expect(host.querySelector('.ck-potato-empty')?.textContent).toBe('Say hello to Fish.');
    expect(host.querySelector('.ck-potato-jump')).toBeNull();
  });
});
