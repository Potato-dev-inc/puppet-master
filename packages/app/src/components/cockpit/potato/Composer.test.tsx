// @vitest-environment jsdom
// The Potato Bot message box: send and newline, the @ and / pickers, attachments (chips, caps, paste, drop), drafts per
// bot, the error strip with Retry, the off state with its reason, the quote event, and the mic.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport } from '../../../lib/potato';
import type { AttachTransport } from '../../../lib/potato-attach';
import { LS_COMPOSER_DRAFTS } from '../../../lib/composerDraft';
import { Composer, type ComposerProps } from './Composer';
import { bot, typeInto } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  window.localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (globalThis as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition;
});

const sleep = (ms: number) => act(async () => void (await new Promise((r) => setTimeout(r, ms))));
const field = () => host.querySelector<HTMLTextAreaElement>('textarea')!;
const sendButton = () => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === 'Send')!;
const byLabel = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
const rows = () => [...host.querySelectorAll('[role="option"]')].map((el) => el.textContent ?? '');
const chips = () => [...host.querySelectorAll('.ck-potato-chip-name')].map((el) => el.textContent);
const strip = (role: 'alert' | 'status') => host.querySelector(`.ck-potato-strip[role="${role}"]`);
const draftsInStorage = (): Record<string, { text: string }> => JSON.parse(window.localStorage.getItem(LS_COMPOSER_DRAFTS) ?? '{}');

function key(name: string, init: KeyboardEventInit = {}) {
  const e = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init });
  act(() => {
    field().dispatchEvent(e);
  });
  return e;
}

const fileOf = (name: string, type: string, body = 'hello', size?: number) => {
  const f = new File([body], name, { type });
  if (size !== undefined) Object.defineProperty(f, 'size', { value: size });
  return f;
};

function event(type: string, props: Record<string, unknown>) {
  const e = new Event(type, { bubbles: true, cancelable: true });
  for (const [k, v] of Object.entries(props)) Object.defineProperty(e, k, { value: v });
  return e;
}

async function choose(...files: File[]) {
  const input = host.querySelector<HTMLInputElement>('[data-testid="potato-file-input"]')!;
  Object.defineProperty(input, 'files', { value: files, configurable: true });
  await act(async () => {
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await sleep(40);
}

interface Setup {
  transport: FakePotatoTransport;
  attach: AttachTransport & { sendWithFiles: ReturnType<typeof vi.fn> };
  props: ComposerProps;
}

function setup(over: Partial<ComposerProps> = {}): Setup {
  const transport = fakePotatoTransport({ bots: [bot(), bot({ id: 'nimbus', name: 'Nimbus' })] });
  const attach = { sendWithFiles: vi.fn(async () => ({ id: 'm1', state: 'ready' as const, delivered: true })) };
  const props: ComposerProps = { bot: bot(), transport, onChanged: () => {}, onNotify: () => {}, attach, ...over };
  return { transport, attach, props };
}

const render = (props: ComposerProps) => act(async () => root.render(<Composer {...props} />));
const sends = (t: FakePotatoTransport) => t.calls.filter((c) => c.method === 'send').map((c) => c.args);

describe('send and newline', () => {
  it('sends on Enter, trims the text, clears the box and tells the thread', async () => {
    const { props, transport } = setup();
    const onChanged = vi.fn();
    const onSent = vi.fn();
    await render({ ...props, onChanged, onSent });
    expect(field().placeholder).toBe('Message Fish');
    typeInto(field(), '  hello there  ');
    const e = key('Enter');
    await sleep(0);
    expect(e.defaultPrevented).toBe(true);
    expect(sends(transport)).toEqual([['fish', 'hello there']]);
    expect(field().value).toBe('');
    expect(onChanged).toHaveBeenCalled();
    expect(onSent).toHaveBeenCalled();
  });

  it('adds a line on Shift+Enter and sends nothing', async () => {
    const { props, transport } = setup();
    await render(props);
    typeInto(field(), 'one');
    const e = key('Enter', { shiftKey: true });
    expect(e.defaultPrevented).toBe(false);
    expect(sends(transport)).toEqual([]);
  });

  it('does not send while an input method is still choosing a word', async () => {
    const { props, transport } = setup();
    await render(props);
    typeInto(field(), 'ni');
    key('Enter', { isComposing: true });
    expect(sends(transport)).toEqual([]);
  });

  it('keeps Send off for an empty or blank box and sends nothing on Enter', async () => {
    const { props, transport } = setup();
    await render(props);
    expect(sendButton().disabled).toBe(true);
    typeInto(field(), '   ');
    key('Enter');
    await sleep(0);
    expect(sends(transport)).toEqual([]);
    typeInto(field(), 'hi');
    expect(sendButton().disabled).toBe(false);
  });

  it('grows with the text up to eight lines, then scrolls', async () => {
    const { props } = setup();
    await render(props);
    const proto = HTMLTextAreaElement.prototype;
    let height = 20;
    Object.defineProperty(proto, 'scrollHeight', { configurable: true, get: () => height });
    try {
      height = 60;
      typeInto(field(), 'a\nb\nc');
      expect(field().style.height).toBe('60px');
      height = 900;
      typeInto(field(), Array.from({ length: 40 }, () => 'x').join('\n'));
      expect(field().style.height).toBe('160px');
    } finally {
      delete (proto as { scrollHeight?: number }).scrollHeight;
    }
  });
});

describe('the @ picker', () => {
  it('lists the other bots, never this one, and puts the picked name in with the keyboard', async () => {
    const { props } = setup({ bots: [{ id: 'fish', name: 'Fish' }, { id: 'nimbus', name: 'Nimbus' }, { id: 'mochi', name: 'Mochi' }] });
    await render(props);
    typeInto(field(), 'hey @');
    expect(rows().map((r) => r.replace(/\s.*/, ''))).toEqual(['@Nimbus', '@Mochi']);
    key('ArrowDown');
    expect(host.querySelector('[role="option"][aria-selected="true"]')?.textContent).toContain('@Mochi');
    key('Enter');
    expect(field().value).toBe('hey @Mochi ');
    expect(rows()).toEqual([]);
  });

  it('filters while typing, picks with the mouse, and closes on Escape', async () => {
    const { props, transport } = setup({ bots: [{ id: 'nimbus', name: 'Nimbus' }, { id: 'mochi', name: 'Mochi' }] });
    await render(props);
    typeInto(field(), '@mo');
    expect(rows()).toHaveLength(1);
    key('Escape');
    expect(rows()).toEqual([]);
    typeInto(field(), '@m');
    await act(async () => host.querySelector<HTMLElement>('[role="option"]')!.click());
    expect(field().value).toBe('@Mochi ');
    expect(sends(transport)).toEqual([]);
  });

  it('reads the app\'s bots the first time @ is typed, and says so when there is nobody else', async () => {
    const { props, transport } = setup();
    await render(props);
    expect(transport.calls.some((c) => c.method === 'state')).toBe(false);
    typeInto(field(), '@');
    await sleep(0);
    expect(transport.calls.filter((c) => c.method === 'state')).toHaveLength(1);
    expect(rows().map((r) => r.replace(/\s.*/, ''))).toEqual(['@Nimbus']);

    const alone = setup({ bot: bot({ id: 'solo', name: 'Solo' }) });
    alone.transport.calls.length = 0;
    const lone = fakePotatoTransport({ bots: [bot({ id: 'solo', name: 'Solo' })] });
    await render({ ...alone.props, transport: lone });
    typeInto(field(), '@');
    await sleep(0);
    expect(host.textContent).toContain('No other bots to mention yet.');
    key('Enter');
    expect(host.textContent).not.toContain('No other bots to mention yet.');
    expect(sends(lone)).toEqual([]);
  });
});

describe('the / picker', () => {
  it('lists the commands this page can run, filters them, and fills the box with the picked one', async () => {
    const { props } = setup();
    await render(props);
    typeInto(field(), '/');
    expect(rows().map((r) => r.split(/(?=[A-Z])/)[0])).toEqual(['/persona fish', '/persona potato', '/help']);
    typeInto(field(), '/per');
    expect(rows()).toHaveLength(2);
    key('ArrowDown');
    key('Tab');
    expect(field().value).toBe('/persona potato');
    expect(rows()).toEqual([]);
  });

  it('runs /persona instead of sending it as a message', async () => {
    const { props, transport } = setup();
    const onChanged = vi.fn();
    await render({ ...props, onChanged });
    typeInto(field(), '/persona potato');
    key('Enter');
    await sleep(0);
    expect(transport.calls.find((c) => c.method === 'setPersona')?.args).toEqual(['fish', 'potato']);
    expect(sends(transport)).toEqual([]);
    expect(field().value).toBe('');
    expect(onChanged).toHaveBeenCalled();
  });

  it('answers /help in a note and a wrong /persona with the right words, keeping what was typed', async () => {
    const { props, transport } = setup();
    await render(props);
    typeInto(field(), '/help');
    key('Enter');
    await sleep(0);
    expect(strip('status')?.textContent).toContain('Type @ to mention another bot.');
    typeInto(field(), '/persona cow');
    key('Enter');
    await sleep(0);
    expect(strip('status')?.textContent).toContain('Say /persona fish or /persona potato.');
    expect(field().value).toBe('/persona cow');
    expect(transport.calls.some((c) => c.method === 'setPersona' || c.method === 'send')).toBe(false);
  });

  it('offers /new and /computer only when the page gives their actions, and runs them', async () => {
    const { props } = setup();
    const onNewBot = vi.fn();
    const onOpenComputer = vi.fn();
    await render({ ...props, actions: { onNewBot, onOpenComputer } });
    typeInto(field(), '/');
    expect(rows().map((r) => r.split(/(?=[A-Z])/)[0])).toEqual(['/new', '/persona fish', '/persona potato', '/computer', '/help']);
    typeInto(field(), '/computer');
    key('Enter');
    await sleep(0);
    expect(onOpenComputer).toHaveBeenCalledTimes(1);
    typeInto(field(), '/new');
    key('Enter');
    await sleep(0);
    expect(onNewBot).toHaveBeenCalledTimes(1);
    expect(field().value).toBe('');
  });

  it('sends a command that does not exist here (/clear, a path) as a normal message', async () => {
    const { props, transport } = setup();
    await render(props);
    typeInto(field(), '/clear');
    key('Enter');
    await sleep(0);
    typeInto(field(), '/usr/bin/env is where?');
    key('Enter');
    await sleep(0);
    expect(sends(transport)).toEqual([['fish', '/clear'], ['fish', '/usr/bin/env is where?']]);
  });
});

describe('attachments', () => {
  it('shows no paperclip when there is no way to send files', async () => {
    const { props } = setup({ attach: null });
    await render(props);
    expect(byLabel('Attach a file')).toBeNull();
    expect(host.querySelector('[data-testid="potato-file-input"]')).toBeNull();
  });

  it('opens the file picker from the paperclip and shows each chosen file as a chip with its size, then removes it', async () => {
    const { props } = setup();
    await render(props);
    const click = vi.spyOn(host.querySelector<HTMLInputElement>('[data-testid="potato-file-input"]')!, 'click').mockImplementation(() => {});
    await act(async () => byLabel('Attach a file')!.click());
    expect(click).toHaveBeenCalled();
    await choose(fileOf('notes.txt', 'text/plain', 'hello'), fileOf('pic.png', 'image/png', 'x'.repeat(2000)));
    expect(chips()).toEqual(['notes.txt', 'pic.png']);
    expect(host.querySelector('.ck-potato-chip-size')?.textContent).toBe('5 B');
    expect(host.querySelector('img.ck-potato-chip-thumb')?.getAttribute('src')).toMatch(/^data:image\/png;base64,/);
    await act(async () => byLabel('Remove notes.txt')!.click());
    expect(chips()).toEqual(['pic.png']);
  });

  it('takes pictures pasted into the box and leaves a paste of plain text alone', async () => {
    const { props } = setup();
    await render(props);
    const plain = event('paste', { clipboardData: { files: [] } });
    act(() => void field().dispatchEvent(plain));
    expect(plain.defaultPrevented).toBe(false);
    const pasted = event('paste', { clipboardData: { files: [fileOf('image.png', 'image/png')] } });
    await act(async () => void field().dispatchEvent(pasted));
    await sleep(40);
    expect(pasted.defaultPrevented).toBe(true);
    expect(chips()).toEqual(['image.png']);
  });

  it('takes files dropped on the box and lights it while they hang over', async () => {
    const { props } = setup();
    await render(props);
    const form = host.querySelector('form')!;
    const over = event('dragover', { dataTransfer: { types: ['Files'], files: [] } });
    act(() => void form.dispatchEvent(over));
    expect(over.defaultPrevented).toBe(true);
    expect(form.className).toContain('ck-potato-composer--drop');
    const drop = event('drop', { dataTransfer: { types: ['Files'], files: [fileOf('todo.md', 'text/markdown')] } });
    await act(async () => void form.dispatchEvent(drop));
    await sleep(40);
    expect(form.className).not.toContain('ck-potato-composer--drop');
    expect(chips()).toEqual(['todo.md']);
    // A drag of plain text is not ours.
    const text = event('dragover', { dataTransfer: { types: ['text/plain'], files: [] } });
    act(() => void form.dispatchEvent(text));
    expect(text.defaultPrevented).toBe(false);
  });

  it('says why a file is refused: wrong kind, too big, too many, too much together', async () => {
    const { props } = setup();
    await render(props);
    await choose(fileOf('run.exe', 'application/x-msdownload'));
    expect(strip('status')?.textContent).toContain('run.exe: only pictures');
    expect(chips()).toEqual([]);
    await choose(fileOf('huge.png', 'image/png', 'x', 3 * 1024 * 1024));
    expect(strip('status')?.textContent).toContain('huge.png is bigger than 2.0 MB.');
    await choose(fileOf('a.txt', 'text/plain'), fileOf('b.txt', 'text/plain'), fileOf('c.txt', 'text/plain'), fileOf('d.txt', 'text/plain'));
    expect(chips()).toHaveLength(4);
    await choose(fileOf('e.txt', 'text/plain'));
    expect(strip('status')?.textContent).toContain('You can attach up to 4 files.');
    expect(chips()).toHaveLength(4);
    expect(byLabel('Attach a file')!.disabled).toBe(true);
    await act(async () => host.querySelector<HTMLButtonElement>('.ck-potato-strip-x')!.click());
    expect(strip('status')).toBeNull();
  });

  it('sends files with the text through the file send, never through the plain one, and empties the chips', async () => {
    const { props, transport, attach } = setup();
    await render(props);
    await choose(fileOf('notes.txt', 'text/plain', 'hello'));
    expect(sendButton().disabled).toBe(false);
    typeInto(field(), 'read this');
    key('Enter');
    await sleep(0);
    expect(attach.sendWithFiles).toHaveBeenCalledTimes(1);
    const [botId, text, files] = attach.sendWithFiles.mock.calls[0] as [string, string, { name: string; kind: string; data: string }[]];
    expect([botId, text]).toEqual(['fish', 'read this']);
    expect(files.map((f) => [f.name, f.kind, f.data])).toEqual([['notes.txt', 'text', btoa('hello')]]);
    expect(sends(transport)).toEqual([]);
    expect(chips()).toEqual([]);
  });

  it('sends files with no words at all', async () => {
    const { props, attach } = setup();
    await render(props);
    await choose(fileOf('pic.png', 'image/png'));
    await act(async () => sendButton().click());
    await sleep(0);
    expect(attach.sendWithFiles.mock.calls[0].slice(0, 2)).toEqual(['fish', '']);
  });

  it('does not run a command that has files with it', async () => {
    const { props, transport, attach } = setup();
    await render(props);
    await choose(fileOf('a.txt', 'text/plain'));
    typeInto(field(), '/help');
    key('Enter');
    await sleep(0);
    expect(attach.sendWithFiles).toHaveBeenCalledTimes(1);
    expect(transport.calls.some((c) => c.method === 'setPersona')).toBe(false);
  });
});

describe('drafts', () => {
  it('keeps the unsent text for each bot and shows it again', async () => {
    const { props } = setup();
    await render(props);
    typeInto(field(), 'half a thought');
    await sleep(350);
    expect(draftsInStorage()['potato:fish']?.text).toBe('half a thought');

    act(() => root.unmount());
    root = createRoot(host);
    await render(props);
    expect(field().value).toBe('half a thought');

    // Another bot has its own box.
    await render({ ...props, bot: bot({ id: 'nimbus', name: 'Nimbus' }) });
    expect(field().value).toBe('');
    typeInto(field(), 'for nimbus');
    await sleep(350);
    await render(props);
    expect(field().value).toBe('half a thought');
    expect(draftsInStorage()['potato:nimbus']?.text).toBe('for nimbus');
  });

  it('writes the text when the box goes away before the wait is over', async () => {
    const { props } = setup();
    await render(props);
    typeInto(field(), 'quick');
    act(() => root.unmount());
    expect(draftsInStorage()['potato:fish']?.text).toBe('quick');
    root = createRoot(host);
  });

  it('forgets the draft when the message is sent', async () => {
    const { props } = setup();
    await render(props);
    typeInto(field(), 'going out');
    await sleep(350);
    expect(draftsInStorage()['potato:fish']).toBeDefined();
    key('Enter');
    await sleep(350);
    expect(draftsInStorage()['potato:fish']).toBeUndefined();
  });
});

describe('a failed send', () => {
  it('gives the words back, shows the reason in a strip above the box with Retry, and tells the page', async () => {
    const { props, transport } = setup();
    const onNotify = vi.fn();
    const send = vi.spyOn(transport, 'send').mockRejectedValueOnce(new Error('POTATO_RATE: That is too fast. Wait a moment, then try again.'));
    await render({ ...props, onNotify });
    typeInto(field(), 'hello');
    key('Enter');
    await sleep(0);
    expect(field().value).toBe('hello');
    const alert = strip('alert')!;
    expect(alert.textContent).toContain('That is too fast. Wait a moment, then try again.');
    expect(onNotify).toHaveBeenCalledWith('That is too fast. Wait a moment, then try again.');
    // The strip sits above the box.
    expect(alert.compareDocumentPosition(host.querySelector('.ck-potato-pill')!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    const onChanged = vi.fn();
    await render({ ...props, onNotify, onChanged });
    const retry = [...alert.querySelectorAll('button')].find((b) => b.textContent === 'Retry')!;
    await act(async () => retry.click());
    await sleep(0);
    expect(send).toHaveBeenCalledTimes(2);
    expect(send.mock.calls[1]).toEqual(['fish', 'hello']);
    expect(strip('alert')).toBeNull();
    expect(field().value).toBe('');
    expect(onChanged).toHaveBeenCalled();
  });

  it('Retry sends the failed message again even when the person has started something new', async () => {
    const { props, transport } = setup();
    const send = vi.spyOn(transport, 'send').mockRejectedValueOnce(new Error('NETWORK: No connection.'));
    await render(props);
    typeInto(field(), 'first');
    key('Enter');
    await sleep(0);
    typeInto(field(), 'second');
    await act(async () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Retry')!.click());
    await sleep(0);
    expect(send.mock.calls.map((c) => c[1])).toEqual(['first', 'first']);
    expect(field().value).toBe('second');
  });

  it('keeps the files for Retry and sends them again', async () => {
    const { props, attach } = setup();
    attach.sendWithFiles.mockRejectedValueOnce(new Error('POTATO_ATTACHMENT: a.txt is empty.'));
    await render(props);
    await choose(fileOf('a.txt', 'text/plain'));
    typeInto(field(), 'see');
    key('Enter');
    await sleep(0);
    expect(strip('alert')?.textContent).toContain('a.txt is empty.');
    expect(chips()).toEqual(['a.txt']);
    await act(async () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Retry')!.click());
    await sleep(0);
    expect(attach.sendWithFiles).toHaveBeenCalledTimes(2);
    expect(attach.sendWithFiles.mock.calls[1][1]).toBe('see');
    expect(chips()).toEqual([]);
    expect(strip('alert')).toBeNull();
  });

  it('can be dismissed', async () => {
    const { props, transport } = setup();
    vi.spyOn(transport, 'send').mockRejectedValueOnce(new Error('boom'));
    await render(props);
    typeInto(field(), 'x');
    key('Enter');
    await sleep(0);
    expect(strip('alert')).not.toBeNull();
    await act(async () => byLabel('Dismiss')!.click());
    expect(strip('alert')).toBeNull();
  });

  it('refuses words that leave no room for the files, without sending', async () => {
    const { props, attach } = setup();
    await render(props);
    await choose(fileOf('a.txt', 'text/plain'));
    typeInto(field(), 'w'.repeat(3950));
    key('Enter');
    await sleep(0);
    expect(attach.sendWithFiles).not.toHaveBeenCalled();
    expect(strip('alert')?.textContent).toContain('Shorten the message a little so the files fit.');
    expect(strip('alert')?.textContent).not.toContain('Retry');
    expect(field().value).toHaveLength(3950);
  });
});

describe('off', () => {
  it('turns everything off and says why when Potato Bot is switched off', async () => {
    const { props } = setup({ bot: bot({ brainState: 'off' }) });
    await render(props);
    expect(field().disabled).toBe(true);
    expect(sendButton().disabled).toBe(true);
    expect(byLabel('Attach a file')!.disabled).toBe(true);
    expect(host.querySelector('[role="status"]')?.textContent).toContain('switched off');
    expect(field().getAttribute('aria-describedby')).toBe(host.querySelector('.ck-potato-reason')!.id);
  });

  it('turns off at the daily limit with its own reason, and on again when the limit moves', async () => {
    const { props } = setup({ bot: bot({ brainState: 'daily_limit' }) });
    await render(props);
    expect(field().disabled).toBe(true);
    expect(host.querySelector('.ck-potato-reason')?.textContent).toContain("Today's limit is reached");
    await render({ ...props, bot: bot({ brainState: 'ready' }) });
    expect(field().disabled).toBe(false);
    expect(host.querySelector('.ck-potato-reason')).toBeNull();
  });

  it('takes no quote while it is off', async () => {
    const { props } = setup({ bot: bot({ brainState: 'off' }) });
    await render(props);
    act(() => void window.dispatchEvent(new CustomEvent('potato:quote', { detail: { text: 'hi' } })));
    expect(field().value).toBe('');
  });
});

describe('quotes from the thread', () => {
  const quote = (detail: unknown) => act(() => void window.dispatchEvent(new CustomEvent('potato:quote', { detail })));

  it('puts the quoted text in as "> " lines and leaves the cursor below them', async () => {
    const { props } = setup();
    await render(props);
    quote({ text: 'line one\nline two' });
    expect(field().value).toBe('> line one\n> line two\n\n');
    expect(document.activeElement).toBe(field());
    expect(field().selectionStart).toBe(field().value.length);
  });

  it('adds it under what is already written, takes a plain string, and ignores a quote for another bot', async () => {
    const { props } = setup();
    await render(props);
    typeInto(field(), 'my answer');
    quote('quoted');
    expect(field().value).toBe('my answer\n\n> quoted\n\n');
    quote({ text: 'not for us', botId: 'nimbus' });
    expect(field().value).toBe('my answer\n\n> quoted\n\n');
    quote({ text: 'for us', botId: 'fish' });
    expect(field().value).toContain('> for us');
  });

  it('stops listening when the box is gone', async () => {
    const { props } = setup();
    await render(props);
    act(() => root.unmount());
    expect(() => window.dispatchEvent(new CustomEvent('potato:quote', { detail: 'x' }))).not.toThrow();
    root = createRoot(host);
  });
});

describe('hold to talk', () => {
  it('leaves the mic out where the browser cannot listen on the device', async () => {
    const { props } = setup();
    await render(props);
    await sleep(10);
    expect(host.querySelector('[data-testid="potato-mic"]')).toBeNull();
  });

  it('shows the mic where on-device listening works', async () => {
    class Fake {
      static available = async () => 'available';
      processLocally = false;
      lang = '';
      continuous = false;
      interimResults = false;
      onresult = null;
      onerror = null;
      onend = null;
      start() {}
      stop() {}
      abort() {}
    }
    (globalThis as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition = Fake;
    const { props } = setup();
    await render(props);
    await sleep(10);
    expect(host.querySelector('[data-testid="potato-mic"]')).not.toBeNull();
  });
});
