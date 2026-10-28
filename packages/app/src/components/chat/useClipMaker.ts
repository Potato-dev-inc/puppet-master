// The state machine behind the clip sheet: planning -> drawing(p) -> ready -> saving -> saved, or error. It asks
// Rust for a plan (already redacted), draws and encodes it in the webview, and saves the file through the system
// save dialog. A save that is refused goes back to `ready` with the message, so the person can pick another place.
// Nothing here sends data anywhere. The desktop calls are behind `ClipApi` so tests pass a fake.
import { useCallback, useEffect, useRef, useState } from 'react';
import { clipFileName, type ClipExt } from '../../lib/clip';
import type { RenderClipArgs, RenderClipResult } from '../../lib/clipEncode';
import { buildTimeline, REQUEST_MAX_CHARS, type Timeline } from '../../lib/clipTimeline';
import type { ClipPlan } from '../../lib/clipTypes';

/** What the sheet needs from the desktop shell and the encoder (a fake in tests). */
export interface ClipApi {
  plan: (paneId: string, userItemId?: string) => Promise<ClipPlan>;
  render: (args: RenderClipArgs) => Promise<RenderClipResult>;
  stage: (token: string, ext: ClipExt, bytes: Uint8Array) => Promise<number>;
  save: (token: string, path: string) => Promise<string>;
  /** The path the person picked, or null when they cancelled. */
  pickPath: (defaultName: string, ext: ClipExt) => Promise<string | null>;
}

/** A finished video and what it was made from. */
export interface MadeClip {
  plan: ClipPlan;
  timeline: Timeline;
  blob: Blob;
  /** An address the preview plays from ('' when the page cannot make one). */
  url: string;
  ext: ClipExt;
  guardHits: number;
  usedFallback: boolean;
  /** The request text the video was drawn with. */
  caption: string;
}

export type ClipState =
  | { phase: 'planning' }
  | { phase: 'drawing'; progress: number }
  | { phase: 'ready'; made: MadeClip; saveError: string | null }
  | { phase: 'saving'; made: MadeClip }
  | { phase: 'saved'; made: MadeClip; path: string }
  | { phase: 'error'; message: string };

export const NOTHING_TO_SHOW = 'There is nothing to show for this turn.';

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** A plan with no replay, no steps and no result has nothing a video could show. */
function isEmpty(plan: ClipPlan): boolean {
  return plan.replay.kind === 'none' && plan.steps.length === 0 && !plan.result.files && !plan.result.sentence;
}

function makeUrl(blob: Blob): string {
  return typeof URL !== 'undefined' && typeof URL.createObjectURL === 'function' ? URL.createObjectURL(blob) : '';
}

function freeUrl(url: string): void {
  if (url && typeof URL !== 'undefined' && typeof URL.revokeObjectURL === 'function') URL.revokeObjectURL(url);
}

/** The bytes of a blob (older webviews and jsdom have no `Blob.arrayBuffer`). */
async function blobBytes(blob: Blob): Promise<Uint8Array> {
  if (typeof blob.arrayBuffer === 'function') return new Uint8Array(await blob.arrayBuffer());
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error('Could not read the video.'));
    reader.readAsArrayBuffer(blob);
  });
}

export interface ClipMaker {
  state: ClipState;
  /** The plan, once Rust made it. */
  plan: ClipPlan | null;
  /** The request text in the field. */
  caption: string;
  setCaption: (text: string) => void;
  /** Draw again with the text in the field. */
  redraw: () => void;
  /** Stop drawing (the sheet then closes). */
  cancel: () => void;
  save: () => Promise<void>;
}

export function useClipMaker(paneId: string, userItemId: string | undefined, api: ClipApi): ClipMaker {
  const [state, setState] = useState<ClipState>({ phase: 'planning' });
  const [plan, setPlan] = useState<ClipPlan | null>(null);
  const [caption, setCaptionState] = useState('');
  const stateRef = useRef(state);
  stateRef.current = state;
  const planRef = useRef<ClipPlan | null>(null);
  const captionRef = useRef('');
  const abortRef = useRef<AbortController | null>(null);
  const urlRef = useRef('');

  const dropUrl = useCallback(() => {
    freeUrl(urlRef.current);
    urlRef.current = '';
  }, []);

  const draw = useCallback(
    async (forPlan: ClipPlan, text: string) => {
      abortRef.current?.abort();
      const ctrl = new AbortController();
      abortRef.current = ctrl;
      dropUrl();
      setState({ phase: 'drawing', progress: 0 });
      try {
        const timeline = buildTimeline(forPlan, text);
        const result = await api.render({
          plan: forPlan,
          timeline,
          signal: ctrl.signal,
          onProgress: (p) => {
            if (!ctrl.signal.aborted) setState({ phase: 'drawing', progress: Math.min(1, Math.max(0, p)) });
          },
        });
        if (ctrl.signal.aborted) return;
        const url = makeUrl(result.blob);
        urlRef.current = url;
        const made: MadeClip = { plan: forPlan, timeline, blob: result.blob, url, ext: result.ext, guardHits: result.guardHits, usedFallback: result.usedFallback, caption: text };
        setState({ phase: 'ready', made, saveError: null });
      } catch (error) {
        if (ctrl.signal.aborted) return;
        setState({ phase: 'error', message: errorText(error) });
      }
    },
    [api, dropUrl],
  );

  // Ask Rust for the plan, then draw it. Leaving the sheet stops the drawing and frees the preview.
  useEffect(() => {
    let alive = true;
    setState({ phase: 'planning' });
    api
      .plan(paneId, userItemId)
      .then((next) => {
        if (!alive) return;
        if (isEmpty(next)) {
          setState({ phase: 'error', message: NOTHING_TO_SHOW });
          return;
        }
        planRef.current = next;
        captionRef.current = next.request;
        setPlan(next);
        setCaptionState(next.request);
        void draw(next, next.request);
      })
      .catch((error: unknown) => {
        if (alive) setState({ phase: 'error', message: errorText(error) });
      });
    return () => {
      alive = false;
      abortRef.current?.abort();
      dropUrl();
    };
  }, [api, paneId, userItemId, draw, dropUrl]);

  const setCaption = useCallback((text: string) => {
    const cut = text.slice(0, REQUEST_MAX_CHARS);
    captionRef.current = cut;
    setCaptionState(cut);
  }, []);

  const redraw = useCallback(() => {
    const current = stateRef.current;
    const forPlan = planRef.current;
    if (!forPlan || (current.phase !== 'ready' && current.phase !== 'saved' && current.phase !== 'drawing')) return;
    void draw(forPlan, captionRef.current);
  }, [draw]);

  const cancel = useCallback(() => abortRef.current?.abort(), []);

  const save = useCallback(async () => {
    const current = stateRef.current;
    if (current.phase !== 'ready' && current.phase !== 'saved') return;
    const made = current.made;
    let path: string | null;
    try {
      path = await api.pickPath(clipFileName(new Date(), made.ext), made.ext);
    } catch (error) {
      setState({ phase: 'ready', made, saveError: `Not saved: ${errorText(error)}` });
      return;
    }
    if (!path) return;
    setState({ phase: 'saving', made });
    try {
      await api.stage(made.plan.token, made.ext, await blobBytes(made.blob));
      const saved = await api.save(made.plan.token, path);
      setState({ phase: 'saved', made, path: saved });
    } catch (error) {
      setState({ phase: 'ready', made, saveError: `Not saved: ${errorText(error)}` });
    }
  }, [api]);

  return { state, plan, caption, setCaption, redraw, cancel, save };
}
