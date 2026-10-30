// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Terminal } from '@xterm/xterm';
import { NudgeScheduler, NUDGE_MIN_GAP_MS, NUDGE_SETTLE_MS } from './nudge-scheduler';
import { isNudgeAction, nudgeDesktop, onDesktopNudge } from './desktop-nudge';
import { OwnerGrid } from '../terminal/owner-follow';
import { PaneStreamManager, type PaneScreenReplay } from '../terminal/pane-stream';

describe('nudge scheduler', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('settles 350 ms after the last action and collapses actions close together', () => {
    const fire = vi.fn();
    const s = new NudgeScheduler(fire);
    s.request(true);
    vi.advanceTimersByTime(NUDGE_SETTLE_MS - 50);
    s.request(true);
    vi.advanceTimersByTime(NUDGE_SETTLE_MS - 50);
    expect(fire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(60);
    expect(fire).toHaveBeenCalledTimes(1);
  });

  it('two nudges within 1.5 s produce one redraw per window', () => {
    const fire = vi.fn();
    const s = new NudgeScheduler(fire);
    s.request(true);
    vi.advanceTimersByTime(NUDGE_SETTLE_MS);
    expect(fire).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(200);
    s.request(true); // 200 ms after the first redraw: held back until 1.5 s after it
    vi.advanceTimersByTime(NUDGE_MIN_GAP_MS - 200 - 10);
    expect(fire).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(20);
    expect(fire).toHaveBeenCalledTimes(2);
  });

  it('a request that allows scrolled-up redraws wins over those that skip', () => {
    const fire = vi.fn();
    const s = new NudgeScheduler(fire);
    s.request(true);
    s.request(false);
    vi.advanceTimersByTime(NUDGE_SETTLE_MS);
    expect(fire).toHaveBeenCalledWith(false);
  });

  it('the desktop bus reaches the open terminal once for a burst of actions', () => {
    const listener = vi.fn();
    const off = onDesktopNudge(listener);
    nudgeDesktop();
    nudgeDesktop();
    nudgeDesktop();
    vi.advanceTimersByTime(NUDGE_SETTLE_MS + 10);
    expect(listener).toHaveBeenCalledTimes(1);
    off();
  });
});

describe('which clicks nudge', () => {
  it('buttons and menu items do; the terminal and typing areas do not', () => {
    document.body.innerHTML =
      '<button id="b"><span id="in">x</span></button><div role="menuitem" id="m">m</div><div class="terminal-host"><button id="t">t</button></div><div id="plain">p</div>';
    const q = (id: string) => document.getElementById(id);
    expect(isNudgeAction(q('b'))).toBe(true);
    expect(isNudgeAction(q('in'))).toBe(true);
    expect(isNudgeAction(q('m'))).toBe(true);
    expect(isNudgeAction(q('t'))).toBe(false);
    expect(isNudgeAction(q('plain'))).toBe(false);
  });
});

const enc = new TextEncoder();
const flush = (t: Terminal) => new Promise<void>((r) => t.write('', r));
const lines = (t: Terminal) => {
  const b = t.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < b.length; i++) out.push(b.getLine(i)?.translateToString(true) ?? '');
  while (out.length && out[out.length - 1] === '') out.pop();
  return out;
};

/** A pane at 40x8 with 30 lines of history (a shell: no reprint, history kept). */
function pane() {
  const emu = new Terminal({ cols: 40, rows: 8, scrollback: 1000, allowProposedApi: true });
  const snapshot = (): PaneScreenReplay => {
    const b = emu.buffer.active;
    let out = '';
    for (let i = 0; i < b.baseY; i++) out += `${b.getLine(i)?.translateToString(true) ?? ''}\r\n`;
    if (b.baseY > 0) out += '\r\n'.repeat(emu.rows - 1);
    out += '\x1b[H\x1b[2J';
    for (let r = 0; r < emu.rows; r++) out += `\x1b[${r + 1};1H${b.getLine(b.baseY + r)?.translateToString(true) ?? ''}`;
    out += `\x1b[${b.cursorY + 1};${b.cursorX + 1}H`;
    return { data: enc.encode(out), cols: emu.cols, rows: emu.rows, offset: 0 };
  };
  return { emu, snapshot };
}

async function mount(snapshot: () => PaneScreenReplay, size = { cols: 40, rows: 8 }) {
  const term = new Terminal({ ...size, scrollback: 1000, allowProposedApi: true });
  new PaneStreamManager().subscribe(
    'p',
    (d) => term.write(d),
    async () => snapshot(),
    (c, r) => term.write(new Uint8Array(0), () => term.resize(c, r)),
  );
  await new Promise((r) => setTimeout(r, 10));
  await flush(term);
  return term;
}

describe('remount nudge', () => {
  it('restores a corrupted session to the fresh-mount state and keeps a shell\'s history', async () => {
    const { emu, snapshot } = pane();
    for (let i = 0; i < 30; i++) emu.write(`history ${i}\r\n`);
    await flush(emu);

    const fresh = await mount(snapshot);
    // a corrupted live session: rows larger than the PTY's, scrolled away from the bottom
    const corrupt = new Terminal({ cols: 40, rows: 12, scrollback: 1000, allowProposedApi: true });
    corrupt.write('garbage\r\n'.repeat(40));
    await flush(corrupt);
    corrupt.scrollLines(-5);
    expect(corrupt.rows).not.toBe(emu.rows);

    const remounted = await mount(snapshot, { cols: 40, rows: 12 });
    expect({ cols: remounted.cols, rows: remounted.rows }).toEqual({ cols: fresh.cols, rows: fresh.rows });
    expect(lines(remounted)).toEqual(lines(fresh));
    expect(remounted.buffer.active.viewportY).toBe(fresh.buffer.active.viewportY);
    expect(lines(remounted).filter((l) => l.startsWith('history')).length).toBe(30);
  });

  it('sends no resize when the fit matches the PTY grid it was told about', () => {
    const grid = new OwnerGrid();
    grid.lastSent = { cols: 100, rows: 30 }; // assumePtyGrid
    expect(grid.fitted(100, 30)).toBeNull();
    expect(grid.fitted(90, 30)).toEqual({ cols: 90, rows: 30 }); // really differs: exactly one
    expect(grid.fitted(90, 30)).toBeNull();
  });
});
