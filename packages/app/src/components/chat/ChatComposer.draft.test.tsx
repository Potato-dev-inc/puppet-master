// @vitest-environment jsdom
// The message box keeps the unsent text per pane (lib/composerDraft.ts): it comes back after the box is left and
// mounted again, a sent message clears it, another pane starts empty, and a storage that throws changes nothing.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatComposer } from './ChatComposer';
import { type ComposerDraft, DRAFT_SAVE_MS, LS_COMPOSER_DRAFTS, readDraft } from '../../lib/composerDraft';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A fake browser recognizer, as in ChatComposer.voice.test.tsx: `emit` speaks a list of [text, isFinal]. */
class FakeRecognition {
  static sessions: FakeRecognition[] = [];
  static available = async () => 'available';

  onresult: ((event: unknown) => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  onend: (() => void) | null = null;
  lang = '';
  continuous = false;
  interimResults = false;
  processLocally = false;

  start() {
    FakeRecognition.sessions.push(this);
  }
  stop() {}
  abort() {}
  emit(results: Array<[string, boolean]>) {
    const list = results.map(([transcript, isFinal]) => Object.assign([{ transcript }], { isFinal }));
    this.onresult?.({ resultIndex: 0, results: list });
  }
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  localStorage.clear();
  FakeRecognition.sessions = [];
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
/** Waits past the save delay, as a person pausing before they look away. */
const waitForSave = () => act(async () => { await new Promise((r) => setTimeout(r, DRAFT_SAVE_MS + 50)); });
const field = () => host.querySelector('textarea') as HTMLTextAreaElement;
const sendButton = () => host.querySelector('button[type="submit"]') as HTMLButtonElement;

function render(paneId: string, onSend = vi.fn(async (_text: string) => null as string | null), voice = false) {
  act(() => root.render(<ChatComposer onSend={onSend} blockedReason={null} voice={voice} draftKey={paneId} />));
  return onSend;
}

/** The box goes away (the chat is closed or the pane is left) and a new one is made. */
function leave() {
  act(() => root.unmount());
  root = createRoot(host);
}

function type(text: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field(), text);
    field().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function press(key: string) {
  act(() => {
    field().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
}

describe('ChatComposer draft', () => {
  it('the text comes back when the box is left and mounted again for the same pane', async () => {
    render('pane-a');
    type('half written');
    leave();
    render('pane-a');
    await flush();
    expect(field().value).toBe('half written');
  });

  it('the draft is written about 300 ms after the last change, not on every key', async () => {
    render('pane-a');
    type('half');
    expect(readDraft('pane-a')).toBe('');
    await waitForSave();
    expect(readDraft('pane-a')).toBe('half');
  });

  it('another pane has its own box: it starts empty, and the first pane keeps its draft', async () => {
    render('pane-a');
    type('draft for a');
    await waitForSave();
    leave();
    render('pane-b');
    await flush();
    expect(field().value).toBe('');
    leave();
    render('pane-a');
    await flush();
    expect(field().value).toBe('draft for a');
  });

  it('a sent message clears the draft, so the box is empty when it comes back', async () => {
    const onSend = render('pane-a');
    type('send this');
    await waitForSave();
    expect(readDraft('pane-a')).toBe('send this');
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('send this');
    expect(readDraft('pane-a')).toBe('');
    leave();
    render('pane-a');
    await flush();
    expect(field().value).toBe('');
  });

  it('a refused message keeps the text and the draft', async () => {
    const onSend = render('pane-a', vi.fn(async () => 'Too many waiting messages.'));
    type('one more');
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledTimes(1);
    await waitForSave();
    expect(field().value).toBe('one more');
    expect(readDraft('pane-a')).toBe('one more');
  });

  it('a storage that throws changes nothing: the box still types and sends', async () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    const onSend = render('pane-a');
    await flush();
    type('still works');
    expect(field().value).toBe('still works');
    await waitForSave();
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('still works');
    expect(field().value).toBe('');
  });

  it('a box with no pane keeps nothing', async () => {
    act(() => root.render(<ChatComposer onSend={vi.fn(async () => null)} blockedReason={null} />));
    type('not kept');
    await waitForSave();
    expect(localStorage.getItem(LS_COMPOSER_DRAFTS)).toBeNull();
  });

  it('voice: words still being heard are not saved; only the text in the box is', async () => {
    vi.stubGlobal('SpeechRecognition', FakeRecognition);
    render('pane-v', vi.fn(async () => null), true);
    await flush();
    type('Fix the test');
    await waitForSave();

    // A short tap on the mic starts listening.
    const mic = host.querySelector('[data-testid="chat-composer-mic"]') as HTMLButtonElement;
    const now = Date.now();
    vi.spyOn(Date, 'now').mockReturnValue(now);
    act(() => {
      mic.dispatchEvent(new Event('pointerdown', { bubbles: true, cancelable: true }));
    });
    vi.spyOn(Date, 'now').mockReturnValue(now + 50);
    act(() => {
      mic.dispatchEvent(new Event('pointerup', { bubbles: true, cancelable: true }));
    });
    vi.restoreAllMocks();
    await flush();
    const rec = FakeRecognition.sessions[0];
    expect(rec).toBeDefined();

    // Interim words show in the mirror, and the box text stays as typed.
    act(() => rec.emit([['login', false]]));
    expect(field().value).toBe('Fix the test');
    await waitForSave();
    expect(readDraft('pane-v')).toBe('Fix the test');

    // Final words go into the box, and then into the draft.
    act(() => rec.emit([['login', true]]));
    const heard = field().value;
    expect(heard).toContain('login');
    await waitForSave();
    expect(readDraft('pane-v')).toBe(heard);

    // Leaving while new words are still interim keeps only the text in the box.
    act(() => rec.emit([['more words', false]]));
    leave();
    expect(readDraft('pane-v')).toBe(heard);
    expect(readDraft('pane-v')).not.toContain('more words');
  });
});

// The Point and Fix draft ("Edit in Chat") goes into the box once per draft, after a blank line if the box has text.
describe('ChatComposer Point draft', () => {
  const point = (text: string, nonce: number): ComposerDraft => ({ paneId: 'pane-a', text, nonce });

  function renderWith(draft: ComposerDraft | null, onSend = vi.fn(async (_text: string) => null as string | null)) {
    act(() => root.render(<ChatComposer onSend={onSend} blockedReason={null} draftKey="pane-a" draft={draft} />));
    return onSend;
  }

  it('an empty box gets the draft, focused, with the cursor at the end', () => {
    renderWith(point('Change the heading', 1));
    expect(field().value).toBe('Change the heading');
    expect(document.activeElement).toBe(field());
    expect(field().selectionStart).toBe('Change the heading'.length);
  });

  it('a typed text stays, and the draft goes after a blank line', () => {
    renderWith(null);
    type('my notes');
    renderWith(point('Change the heading', 1));
    expect(field().value).toBe('my notes\n\nChange the heading');
  });

  it('the same draft (same nonce) is not put in twice', () => {
    renderWith(point('Change the heading', 1));
    type('edited');
    renderWith(point('Change the heading', 1));
    expect(field().value).toBe('edited');
  });

  it('a new draft with the same text is put in again', () => {
    renderWith(point('Hi', 1));
    renderWith(point('Hi', 2));
    expect(field().value).toBe('Hi\n\nHi');
  });

  it('no draft changes nothing', () => {
    renderWith(null);
    type('typed');
    renderWith(null);
    expect(field().value).toBe('typed');
  });

  it('the draft is sent like any message from the box', async () => {
    const onSend = renderWith(point('Change the heading', 1));
    press('Enter');
    await flush();
    expect(onSend).toHaveBeenCalledWith('Change the heading');
    expect(field().value).toBe('');
  });
});
