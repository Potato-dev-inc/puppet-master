// @vitest-environment jsdom
// The clip sheet: it asks for the plan, shows the drawing progress, then the video; Save picks a path, stages the
// bytes and saves, in that order, with the plan's token; closing while it draws stops the drawing; a changed caption
// draws again; a refused path keeps Save enabled; the slower way and the hidden items are said in plain words.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RenderClipArgs, RenderClipResult } from '../../lib/clipEncode';
import type { ClipPlan } from '../../lib/clipTypes';
import { ClipSheet, FALLBACK_NOTE } from './ClipSheet';
import { NOTHING_TO_SHOW, type ClipApi } from './useClipMaker';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TOKEN = 'plan-token';
const PICKED = 'C:/Users/you/Videos/clip.mp4';

function planOf(over: Partial<ClipPlan> = {}): ClipPlan {
  return {
    token: TOKEN,
    agent: 'Test Agent',
    request: 'Add a dark mode switch',
    turnMs: 60_000,
    outcome: 'completed',
    replay: { kind: 'activity', lengthMs: 8_000 },
    steps: [{ atMs: 0, name: 'Read', summary: 'a.ts', paths: ['a.ts'], writes: false, helper: false, ok: true }],
    result: { files: { count: 2, added: 10, deleted: 3, names: ['a.ts', 'b.ts'] }, checks: null, sentence: 'Done.' },
    notes: [],
    hidden: 0,
    ...over,
  };
}

function resultOf(over: Partial<RenderClipResult> = {}): RenderClipResult {
  return { blob: new Blob([new Uint8Array([0, 0, 0, 8, 0x66, 0x74, 0x79, 0x70])], { type: 'video/mp4' }), ext: 'mp4', guardHits: 0, usedFallback: false, ...over };
}

interface Fake {
  api: ClipApi;
  calls: string[];
  renders: RenderClipArgs[];
}

/** `render` resolves at once unless `hold` is set; then it waits for the signal and rejects like a cancelled render. */
function fakeApi(over: Partial<ClipApi> = {}, opts: { plan?: ClipPlan; result?: RenderClipResult; hold?: boolean } = {}): Fake {
  const calls: string[] = [];
  const renders: RenderClipArgs[] = [];
  const api: ClipApi = {
    plan: vi.fn(async () => {
      calls.push('plan');
      return opts.plan ?? planOf();
    }),
    render: vi.fn(async (args: RenderClipArgs) => {
      calls.push('render');
      renders.push(args);
      args.onProgress(0.4);
      if (opts.hold) {
        await new Promise<void>((_resolve, reject) => {
          args.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
        });
      }
      return opts.result ?? resultOf();
    }),
    pickPath: vi.fn(async () => {
      calls.push('pickPath');
      return PICKED;
    }),
    stage: vi.fn(async () => {
      calls.push('stage');
      return 8;
    }),
    save: vi.fn(async (_token: string, path: string) => {
      calls.push('save');
      return path;
    }),
    ...over,
  };
  return { api, calls, renders };
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  URL.createObjectURL = vi.fn(() => 'blob:clip-1');
  URL.revokeObjectURL = vi.fn();
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

function open(api: ClipApi, onClose: () => void = () => {}) {
  act(() => {
    root.render(<ClipSheet paneId="pane-1" userItemId="u1" api={api} onClose={onClose} />);
  });
}

function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
  if (!found) throw new Error(`no button named ${label}`);
  return found;
}

const hasButton = (label: string) => [...document.querySelectorAll('button')].some((b) => b.textContent?.trim() === label);
const text = () => document.body.textContent ?? '';
const field = () => document.querySelector('input.ck-clip-input') as HTMLInputElement;

function typeCaption(value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field(), value);
    field().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('ClipSheet flow', () => {
  it('asks for the plan of the turn, draws it, then shows the video with the caption field', async () => {
    const { api } = fakeApi();
    open(api);
    expect(text()).toContain('Reading the turn');
    await flush();
    expect(api.plan).toHaveBeenCalledWith('pane-1', 'u1');
    const video = document.querySelector('video') as HTMLVideoElement;
    expect(video.getAttribute('src')).toBe('blob:clip-1');
    expect(video.muted).toBe(true);
    expect(video.loop).toBe(true);
    expect(video.controls).toBe(true);
    expect(field().value).toBe('Add a dark mode switch');
    expect(text()).toContain('Make a clip');
    expect(text()).toContain('Nothing is uploaded');
    expect(text()).toContain('Look at it before you share it');
    expect(button('Save video…').disabled).toBe(false);
  });

  it('shows the drawing progress with a Cancel while the video is made', async () => {
    const { api } = fakeApi({}, { hold: true });
    open(api);
    await flush();
    expect(text()).toContain('Drawing 40%');
    const bar = document.querySelector('[role="progressbar"]') as HTMLElement;
    expect(bar.getAttribute('aria-valuenow')).toBe('40');
    expect(hasButton('Cancel')).toBe(true);
    expect(hasButton('Save video…')).toBe(false);
    expect(document.querySelector('video')).toBeNull();
  });

  it('cancel while drawing stops the render and closes the sheet', async () => {
    const { api, renders } = fakeApi({}, { hold: true });
    const onClose = vi.fn();
    open(api, onClose);
    await flush();
    expect(renders[0].signal.aborted).toBe(false);
    act(() => button('Cancel').click());
    expect(renders[0].signal.aborted).toBe(true);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closing the sheet while it draws (unmount or Escape) stops the render too', async () => {
    const { api, renders } = fakeApi({}, { hold: true });
    const onClose = vi.fn();
    open(api, onClose);
    await flush();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    act(() => root.render(<div />));
    expect(renders[0].signal.aborted).toBe(true);
  });

  it('Save picks a path, then stages the bytes, then saves, with the plan token', async () => {
    const { api, calls } = fakeApi();
    open(api);
    await flush();
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    expect(calls.slice(-3)).toEqual(['pickPath', 'stage', 'save']);
    expect(api.pickPath).toHaveBeenCalledTimes(1);
    const [defaultName, ext] = (api.pickPath as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(defaultName).toMatch(/^clip-\d{4}-\d{2}-\d{2}-\d{4}\.mp4$/);
    expect(ext).toBe('mp4');
    const stage = (api.stage as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(stage[0]).toBe(TOKEN);
    expect(stage[1]).toBe('mp4');
    expect([...(stage[2] as Uint8Array)]).toEqual([0, 0, 0, 8, 0x66, 0x74, 0x79, 0x70]);
    expect(api.save).toHaveBeenCalledWith(TOKEN, PICKED);
    expect(text()).toContain(`Saved to ${PICKED}. Nothing was sent anywhere.`);
    // The video stays, and another copy can be saved (it stages again).
    expect(document.querySelector('video')).not.toBeNull();
    expect(button('Save another copy…').disabled).toBe(false);
  });

  it('a cancelled dialog stages and saves nothing and Save stays enabled', async () => {
    const { api } = fakeApi({ pickPath: vi.fn(async () => null) });
    open(api);
    await flush();
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    expect(api.stage).not.toHaveBeenCalled();
    expect(api.save).not.toHaveBeenCalled();
    expect(button('Save video…').disabled).toBe(false);
  });

  it('a refused path shows the message and keeps Save enabled', async () => {
    const { api } = fakeApi({
      save: vi.fn(async () => {
        throw new Error('The file name must end with .mp4 or .webm.');
      }),
    });
    open(api);
    await flush();
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    const alert = document.querySelector('[role="alert"]');
    expect(alert?.textContent).toBe('Not saved: The file name must end with .mp4 or .webm.');
    expect(button('Save video…').disabled).toBe(false);
    expect(document.querySelector('video')).not.toBeNull();
  });

  it('a failed stage shows the message and the video stays', async () => {
    const { api } = fakeApi({
      stage: vi.fn(async () => {
        throw new Error('The video is too big.');
      }),
    });
    open(api);
    await flush();
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    expect(text()).toContain('Not saved: The video is too big.');
    expect(api.save).not.toHaveBeenCalled();
    expect(button('Save video…').disabled).toBe(false);
  });

  it('Save is disabled while it saves and the sheet cannot be closed then', async () => {
    let release: (path: string) => void = () => {};
    const { api } = fakeApi({ save: vi.fn(() => new Promise<string>((resolve) => (release = resolve))) });
    const onClose = vi.fn();
    open(api, onClose);
    await flush();
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    expect(button('Save video…').disabled).toBe(true);
    expect(button('Close').disabled).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => release(PICKED));
    await flush();
    expect(text()).toContain('Saved to');
  });
});

describe('ClipSheet caption', () => {
  it('Enter on the caption draws again with the new text and the same plan', async () => {
    const { api, renders } = fakeApi();
    open(api);
    await flush();
    expect(renders).toHaveLength(1);
    typeCaption('Add a switch for dark mode');
    expect(hasButton('Draw again')).toBe(true);
    act(() => {
      field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    });
    await flush();
    expect(api.plan).toHaveBeenCalledTimes(1);
    expect(renders).toHaveLength(2);
    const title = renders[1].timeline.scenes[0];
    expect(title.kind === 'title' && title.request).toBe('Add a switch for dark mode');
    expect(renders[0].signal.aborted).toBe(true);
    expect(hasButton('Draw again')).toBe(false);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:clip-1');
  });

  it('the caption field takes at most 140 characters', async () => {
    const { api } = fakeApi();
    open(api);
    await flush();
    expect(field().maxLength).toBe(140);
    typeCaption('x'.repeat(200));
    expect(field().value.length).toBe(140);
  });

  it('the caption cannot be edited while the plan is made or after an error', async () => {
    const { api } = fakeApi({ plan: vi.fn(async () => Promise.reject(new Error('This turn is still running. Make the clip when it ends.'))) });
    open(api);
    expect(field().disabled).toBe(true);
    await flush();
    expect(field().disabled).toBe(true);
  });
});

describe('ClipSheet notes and errors', () => {
  it('says Using the slower way when the encoder fell back, and keeps the file extension of the result', async () => {
    const { api } = fakeApi({}, { result: resultOf({ ext: 'webm', usedFallback: true }) });
    open(api);
    await flush();
    const alert = [...document.querySelectorAll('[role="alert"]')].find((n) => n.textContent === FALLBACK_NOTE);
    expect(alert).toBeDefined();
    expect(FALLBACK_NOTE).toContain('Using the slower way');
    await act(async () => {
      button('Save video…').click();
    });
    await flush();
    expect((api.pickPath as ReturnType<typeof vi.fn>).mock.calls[0][1]).toBe('webm');
    expect((api.pickPath as ReturnType<typeof vi.fn>).mock.calls[0][0]).toMatch(/\.webm$/);
    expect((api.stage as ReturnType<typeof vi.fn>).mock.calls[0][1]).toBe('webm');
  });

  it('does not show the slower-way line for a normal render', async () => {
    const { api } = fakeApi();
    open(api);
    await flush();
    expect(text()).not.toContain('slower way');
  });

  it('says how many items were hidden at the last moment (guard hits)', async () => {
    const { api } = fakeApi({}, { result: resultOf({ guardHits: 3 }) });
    open(api);
    await flush();
    expect(text()).toContain('3 items were hidden at the last moment.');
  });

  it('says it in the singular for one item', async () => {
    const { api } = fakeApi({}, { result: resultOf({ guardHits: 1 }) });
    open(api);
    await flush();
    expect(text()).toContain('1 item was hidden at the last moment.');
  });

  it('no hidden line when nothing was hidden', async () => {
    const { api } = fakeApi();
    open(api);
    await flush();
    expect(text()).not.toContain('hidden at the last moment');
    expect(text()).not.toContain('looked private');
  });

  it('shows how many screen lines Rust hid and the notes of the plan', async () => {
    const plan = planOf({ hidden: 3, notes: ['The first part of this turn was not recorded.'] });
    const { api } = fakeApi({}, { plan });
    open(api);
    await flush();
    expect(text()).toContain('Hid 3 lines that looked private.');
    expect(text()).toContain('The first part of this turn was not recorded.');
  });

  it('shows the refusal of the plan as it is and offers only Close', async () => {
    const message = 'This turn is still running. Make the clip when it ends.';
    const { api } = fakeApi({ plan: vi.fn(async () => Promise.reject(new Error(message))) });
    open(api);
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(message);
    expect(api.render).not.toHaveBeenCalled();
    expect(hasButton('Save video…')).toBe(false);
    expect(hasButton('Close')).toBe(true);
  });

  it('says there is nothing to show when the plan has no scene, step or result', async () => {
    const empty = planOf({ replay: { kind: 'none' }, steps: [], result: { files: null, checks: null, sentence: '' } });
    const { api } = fakeApi({}, { plan: empty });
    open(api);
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(NOTHING_TO_SHOW);
    expect(api.render).not.toHaveBeenCalled();
  });

  it('a failed render shows its message and offers no save', async () => {
    const { api } = fakeApi({ render: vi.fn(async () => Promise.reject(new Error('Your system cannot make video here.'))) });
    open(api);
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe('Your system cannot make video here.');
    expect(hasButton('Save video…')).toBe(false);
  });

  it('frees the preview address when the sheet closes', async () => {
    const { api } = fakeApi();
    open(api);
    await flush();
    act(() => root.render(<div />));
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:clip-1');
  });
});
