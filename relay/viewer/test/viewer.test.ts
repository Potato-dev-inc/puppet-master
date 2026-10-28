// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { Viewer, backoffMs, BACKOFF_MAX_MS, MIN_GAP_MS, POLL_WAIT_S, type Paint } from '../src/viewer';
import { RoomError, type Api } from '../src/api';
import { importKey, toBase64Url } from '../src/crypto';
import { noticeFor, type Model } from '../src/render';
import type { RoomBatch, RoomFrame } from '../src/types';
import { contentKey, KEY_B64, ROOM, sealFrame } from './helpers';

const LINK = {
  room: ROOM,
  readToken: toBase64Url(new Uint8Array(16).fill(3)),
  contentKey: KEY_B64,
};

const snapBody = {
  room: { title: 'Live Room', endsAt: 1, paused: false, hidden: 0 },
  panes: [{ key: 'p1', label: 'Agent A', title: 'Fix login', state: 'working', line: null }],
  items: [{ t: 'user', id: 1, pane: 'p1', text: 'Fix the login bug' }],
};

interface Script {
  snapshot: Array<RoomFrame | null | Error>;
  frames: Array<RoomBatch | Error>;
}

/** An Api that answers from a script. When the frames script runs out, the room has ended. */
function scriptedApi(script: Script): { api: Api; calls: string[] } {
  const calls: string[] = [];
  const api: Api = {
    async snapshot() {
      calls.push('snapshot');
      const next = script.snapshot.shift();
      if (next instanceof Error) {
        throw next;
      }
      return next ?? null;
    },
    async frames(after, wait) {
      calls.push(`frames:${after}:${wait}`);
      const next = script.frames.shift();
      if (next instanceof Error) {
        throw next;
      }
      return next ?? { frames: [], next: after, ended: 'stopped' };
    },
  };
  return { api, calls };
}

/** A viewer with recording sleeps and a recording paint. */
function makeViewer(api: Api, key: CryptoKey, paintFn?: Paint) {
  const sleeps: number[] = [];
  const statuses: string[] = [];
  const paint: Paint =
    paintFn ??
    ((model: Model) => {
      statuses.push(model.status);
      return null;
    });
  const viewer = new Viewer({
    link: LINK,
    key,
    api,
    paint,
    sleep: async (ms) => {
      sleeps.push(ms);
    },
  });
  return { viewer, sleeps, statuses };
}

describe('backoffMs', () => {
  it('doubles from 1 s and stops at 15 s', () => {
    expect([1, 2, 3, 4, 5, 6].map(backoffMs)).toEqual([1000, 2000, 4000, 8000, 15000, 15000]);
    expect(BACKOFF_MAX_MS).toBe(15000);
    expect(backoffMs(0)).toBe(1000);
  });
});

describe('Viewer.receive', () => {
  it('shows a snapshot then a delta, in order', async () => {
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey());
    const snap = await sealFrame({ seq: 1, kind: 'snap', body: snapBody });
    const delta = await sealFrame({
      seq: 2,
      kind: 'delta',
      body: { items: [{ t: 'agent', id: 2, pane: 'p1', text: 'Looking at auth.ts' }] },
    });
    expect(await viewer.receive([snap, delta])).toBe(0);
    expect(viewer.model.items.map((item) => item.id)).toEqual([1, 2]);
    expect(viewer.model.status).toBe('live');
  });

  it('ignores a frame whose seq is not above the last one shown', async () => {
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey());
    const newer = await sealFrame({ seq: 5, kind: 'delta', body: { items: [{ t: 'note', id: 5, pane: 'p1', text: 'new' }] } });
    const older = await sealFrame({ seq: 4, kind: 'delta', body: { items: [{ t: 'note', id: 4, pane: 'p1', text: 'old' }] } });
    const same = await sealFrame({ seq: 5, kind: 'delta', body: { items: [{ t: 'note', id: 6, pane: 'p1', text: 'dup' }] } });
    await viewer.receive([newer, older, same]);
    expect(viewer.model.items.map((item) => item.id)).toEqual([5]);
  });

  it('drops frames for another room', async () => {
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey());
    const other = await sealFrame({ seq: 1, kind: 'snap', room: 'bbbbbbbbbbbbbbbbbbbbbbbbbb', body: snapBody });
    expect(await viewer.receive([other])).toBe(0);
    expect(viewer.model.items).toEqual([]);
    expect(viewer.model.status).toBe('connecting');
  });

  it('counts a frame that cannot be opened and carries on with the next one', async () => {
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey());
    const good = await sealFrame({ seq: 2, kind: 'delta', body: { items: [{ t: 'note', id: 2, pane: 'p1', text: 'ok' }] } });
    const broken = { ...(await sealFrame({ seq: 1, kind: 'delta', body: {} })), ct: 'AAAA' };
    expect(await viewer.receive([broken, good])).toBe(1);
    expect(viewer.model.items.map((item) => item.id)).toEqual([2]);
  });

  it('a paused room shows the paused sentence, and a resume shows the not-shared note', async () => {
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey());
    const snap = await sealFrame({ seq: 1, kind: 'snap', body: snapBody });
    const paused = await sealFrame({
      seq: 2,
      kind: 'state',
      body: { room: { title: 'Live Room', endsAt: 1, paused: true, hidden: 0 } },
    });
    const resumed = await sealFrame({
      seq: 3,
      kind: 'state',
      body: { room: { title: 'Live Room', endsAt: 1, paused: false, hidden: 2 } },
    });
    await viewer.receive([snap, paused]);
    expect(noticeFor(viewer.model)).toBe('Paused by the owner');
    await viewer.receive([resumed]);
    expect(noticeFor(viewer.model)).toBe('Some steps were not shared');
  });
});

describe('Viewer.run', () => {
  it('opens the snapshot, polls from its seq, and clears the page when the room ends', async () => {
    const snap = await sealFrame({ seq: 4, kind: 'snap', body: snapBody });
    const delta = await sealFrame({ seq: 5, kind: 'delta', body: { items: [{ t: 'note', id: 2, pane: 'p1', text: 'hi' }] } });
    const { api, calls } = scriptedApi({
      snapshot: [snap],
      frames: [
        { frames: [delta], next: 5, ended: null },
        { frames: [], next: 5, ended: 'stopped' },
      ],
    });
    const { viewer, sleeps } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(calls).toEqual(['snapshot', `frames:4:${POLL_WAIT_S}`, `frames:5:${POLL_WAIT_S}`]);
    expect(viewer.model.status).toBe('ended');
    expect(viewer.model.items).toEqual([]);
    expect(viewer.model.panes).toEqual([]);
    expect(sleeps).toEqual([MIN_GAP_MS]);
  });

  it('a 404 on the snapshot is a broken link, with no more calls', async () => {
    const { api, calls } = scriptedApi({ snapshot: [new RoomError('missing')], frames: [] });
    const { viewer } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(viewer.model.status).toBe('broken');
    expect(calls).toEqual(['snapshot']);
  });

  it('a 410 on the frames call ends the room', async () => {
    const { api } = scriptedApi({ snapshot: [null], frames: [new RoomError('ended')] });
    const { viewer } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(viewer.model.status).toBe('ended');
  });

  it('a full room shows the sentence, asks for the snapshot again, and joins when a seat frees up', async () => {
    const snap = await sealFrame({ seq: 1, kind: 'snap', body: snapBody });
    const { api, calls } = scriptedApi({ snapshot: [new RoomError('full'), snap], frames: [] });
    const { viewer, sleeps, statuses } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(statuses).toContain('full');
    expect(calls.filter((call) => call === 'snapshot')).toHaveLength(2);
    expect(sleeps[0]).toBe(1000);
    expect(viewer.model.status).toBe('ended');
  });

  it('network errors back off 1 s, 2 s, 4 s, then the call works again', async () => {
    const snap = await sealFrame({ seq: 1, kind: 'snap', body: snapBody });
    const { api } = scriptedApi({
      snapshot: [snap],
      frames: [
        new RoomError('network'),
        new RoomError('network'),
        new RoomError('network'),
        { frames: [], next: 1, ended: 'expired' },
      ],
    });
    const { viewer, sleeps, statuses } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(sleeps).toEqual([1000, 2000, 4000]);
    expect(statuses).toContain('reconnecting');
    expect(viewer.model.status).toBe('ended');
  });

  it('a bad answer from the relay is a broken link, not a retry', async () => {
    const { api, calls } = scriptedApi({ snapshot: [null], frames: [new RoomError('bad')] });
    const { viewer } = makeViewer(api, await contentKey());
    await viewer.run();
    expect(viewer.model.status).toBe('broken');
    expect(calls).toHaveLength(2);
  });

  it('a snapshot that cannot be opened (wrong key) is a broken link', async () => {
    const snap = await sealFrame({ seq: 1, kind: 'snap', body: snapBody });
    const wrongKey = await importKey(toBase64Url(new Uint8Array(32).fill(9)));
    const { api, calls } = scriptedApi({ snapshot: [snap], frames: [] });
    const { viewer } = makeViewer(api, wrongKey);
    await viewer.run();
    expect(viewer.model.status).toBe('broken');
    expect(calls).toEqual(['snapshot']);
  });

  it('select and setFollow redraw the page with the new choice', async () => {
    const seen: Array<{ active: string | null; follow: boolean }> = [];
    const { viewer } = makeViewer(scriptedApi({ snapshot: [], frames: [] }).api, await contentKey(), (_model, ui) => {
      seen.push({ active: ui.active, follow: ui.follow });
      return ui.active;
    });
    viewer.select('p2');
    viewer.setFollow(false);
    expect(seen.at(-2)).toEqual({ active: 'p2', follow: true });
    expect(seen.at(-1)).toEqual({ active: 'p2', follow: false });
  });
});
