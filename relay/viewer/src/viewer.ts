// The viewer page: reads the link, opens the snapshot, then long-polls for frames until the room ends.
// Nothing here sends room data anywhere. The only requests are the three GETs in api.ts.

import { Board, type BoardElements, type BoardUi } from './board';
import { makeApi, RoomError, type Api, type FetchLike } from './api';
import { openFrame, importKey, toBase64Url } from './crypto';
import { parseLink, type RoomLink } from './link';
import { applyFrame, emptyModel, endModel, type Model, type Status } from './render';
import type { Frame, RoomFrame } from './types';

/** How long one frames call waits for new rows. */
export const POLL_WAIT_S = 25;
/** The pause between two good polls. */
export const MIN_GAP_MS = 1000;
/** After a failed call, the wait doubles from 1 s up to this. */
export const BACKOFF_MAX_MS = 15_000;

/** Wait after the n-th failure in a row: 1 s, 2 s, 4 s, ... up to 15 s. */
export function backoffMs(failures: number): number {
  return Math.min(BACKOFF_MAX_MS, 1000 * 2 ** Math.max(0, failures - 1));
}

export type Paint = (model: Model, ui: BoardUi) => string | null;
export type Sleep = (ms: number) => Promise<void>;

export interface ViewerOptions {
  link: RoomLink;
  key: CryptoKey;
  api: Api;
  paint: Paint;
  sleep: Sleep;
}

/** The viewer's state machine. The DOM is behind `paint`, so the rules are tested without a page. */
export class Viewer {
  readonly model: Model = emptyModel();
  ui: BoardUi = { active: null, follow: true };
  /** Where the next frames call starts (the relay's `next`). */
  private cursor = 0;
  /** The highest seq shown. Frames at or below it are dropped. */
  private lastSeq = 0;
  private snapshotDone = false;
  private stopped = false;

  constructor(private readonly options: ViewerOptions) {}

  /** Runs until the room ends, the link breaks, or `stop` is called. */
  async run(): Promise<void> {
    let failures = 0;
    while (!this.stopped) {
      try {
        if (!this.snapshotDone) {
          const snap = await this.options.api.snapshot();
          this.snapshotDone = true;
          if (snap) {
            this.cursor = Math.max(this.cursor, snap.seq);
            if (await this.receive([snap]) > 0) {
              this.broken();
              return;
            }
          }
        }
        const batch = await this.options.api.frames(this.cursor, POLL_WAIT_S);
        if (batch.ended !== null) {
          this.end();
          return;
        }
        this.cursor = Math.max(this.cursor, batch.next);
        await this.receive(batch.frames);
        this.linkWorks();
        failures = 0;
        await this.options.sleep(MIN_GAP_MS);
      } catch (error) {
        const code = error instanceof RoomError ? error.code : 'network';
        if (code === 'ended') {
          this.end();
          return;
        }
        if (code === 'missing' || code === 'bad') {
          this.broken();
          return;
        }
        if (code === 'full') {
          this.snapshotDone = false;
          this.setStatus('full');
        } else {
          this.setStatus(this.model.status === 'connecting' ? 'connecting' : 'reconnecting');
        }
        failures += 1;
        await this.options.sleep(backoffMs(failures));
      }
    }
  }

  stop(): void {
    this.stopped = true;
  }

  /**
   * Opens and applies frames in order. Returns how many could not be opened. A frame for another room, or
   * one at or below the last seq shown, is dropped without being opened.
   */
  async receive(frames: RoomFrame[]): Promise<number> {
    let failed = 0;
    for (const frame of frames) {
      if (frame.room !== this.options.link.room || frame.seq <= this.lastSeq) {
        continue;
      }
      let opened: Frame;
      try {
        opened = await openFrame(this.options.key, frame);
      } catch {
        failed += 1;
        continue;
      }
      applyFrame(this.model, opened);
      this.lastSeq = frame.seq;
    }
    this.draw();
    return failed;
  }

  select(key: string): void {
    this.ui = { ...this.ui, active: key };
    this.draw();
  }

  setFollow(on: boolean): void {
    this.ui = { ...this.ui, follow: on };
    this.draw();
  }

  private linkWorks(): void {
    if (this.model.status === 'reconnecting' || this.model.status === 'full') {
      this.setStatus('live');
    }
  }

  private setStatus(status: Status): void {
    if (this.model.status === 'ended' || this.model.status === 'broken') {
      return;
    }
    this.model.status = status;
    this.draw();
  }

  private end(): void {
    this.stopped = true;
    endModel(this.model);
    this.draw();
  }

  private broken(): void {
    this.stopped = true;
    this.model.items = [];
    this.model.panes = [];
    this.model.room = null;
    this.model.status = 'broken';
    this.draw();
  }

  private draw(): void {
    const active = this.options.paint(this.model, this.ui);
    this.ui = { ...this.ui, active };
  }
}

const ELEMENT_IDS = {
  title: 'lr-title',
  live: 'lr-live',
  notice: 'lr-notice',
  tabs: 'lr-tabs',
  panes: 'lr-panes',
  follow: 'lr-follow',
} as const;

/** Finds the page parts. Returns null when the page is missing one (a broken build). */
export function queryElements(doc: Document): BoardElements | null {
  const get = (id: string): HTMLElement | null => doc.getElementById(id);
  const title = get(ELEMENT_IDS.title);
  const live = get(ELEMENT_IDS.live);
  const notice = get(ELEMENT_IDS.notice);
  const tabs = get(ELEMENT_IDS.tabs);
  const panes = get(ELEMENT_IDS.panes);
  const follow = get(ELEMENT_IDS.follow);
  if (!title || !live || !notice || !tabs || !panes || !(follow instanceof HTMLButtonElement)) {
    return null;
  }
  return { title, live, notice, tabs, panes, follow };
}

/** Starts the page: reads the fragment, imports the key, then runs the viewer. */
export async function boot(doc: Document, win: Window): Promise<void> {
  const els = queryElements(doc);
  if (!els) {
    return;
  }
  let viewer: Viewer | null = null;
  const board = new Board(doc, els, {
    onSelect: (key) => viewer?.select(key),
    onFollow: (on) => viewer?.setFollow(on),
  });
  const paint: Paint = (model, ui) => board.draw(model, ui);

  const link = parseLink(win.location.hash);
  let key: CryptoKey | null = null;
  if (link) {
    try {
      key = await importKey(link.contentKey);
    } catch {
      key = null;
    }
  }
  if (!link || !key) {
    paint({ ...emptyModel(), status: 'broken' }, { active: null, follow: true });
    return;
  }

  const seat = toBase64Url(crypto.getRandomValues(new Uint8Array(16)));
  const fetchFn: FetchLike = (input, init) => win.fetch(input, init);
  const api = makeApi({ origin: win.location.origin, link, seat, fetch: fetchFn });
  viewer = new Viewer({
    link,
    key,
    api,
    paint,
    sleep: (ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
  });
  await viewer.run();
}

// The page starts itself only where the viewer markup is present, so the tests can import this file.
if (typeof document !== 'undefined' && document.getElementById(ELEMENT_IDS.panes)) {
  void boot(document, window);
}
