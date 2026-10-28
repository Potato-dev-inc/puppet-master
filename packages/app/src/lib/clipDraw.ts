// Draws one frame of a clip on a 2D canvas (SPEC-n32 section 3.8). Pure of the DOM: the caller passes the context.
// Every string that reaches fillText goes through GuardCounter (the webview's second wall), text is drawn with
// fillText only (never HTML), and the colours are fixed greys so the video looks the same in any app theme.
import type { GuardCounter } from './clipGuard';
import type { Caption, Scene, Timeline } from './clipTimeline';
import type { ClipPlan, Frame, Run, Step, TerminalReplay } from './clipTypes';

/** The video size (SPEC 3.1). */
export const CLIP_WIDTH = 1280;
export const CLIP_HEIGHT = 720;

/** Fixed greys (SPEC 3.8). */
export const CLIP_COLORS = { bg: '#0a0a0a', text: '#e8e8e8', muted: '#969696', raised: '#191919' } as const;

/** Each activity line appears this long after the one before it, and at most this many lines stay on screen. */
export const ACTIVITY_STEP_MS = 900;
export const ACTIVITY_MAX_LINES = 14;

/** Fixed labels. Nothing else is drawn except plan strings, captions and numbers. */
const MARK_LABEL = 'Puppet Master';
const END_LABEL = 'Made with Puppet Master';
const FAILED_LABEL = 'failed';
const NO_FILES_LABEL = 'No files changed';

const TERM = { x: 40, y: 48, w: 1200, h: 540 };
/** The terminal font: as large as the area allows, never below 12 px (readable) and never above 26 px. */
export const TERM_MIN_FONT = 12;
export const TERM_MAX_FONT = 26;
const CELL_W = 0.6;
const LINE_H = 1.2;
const ACTIVITY_LINE_H = 44;
const CAPTION = { y: 616, h: 48, maxW: 1100, padX: 24 };
const END_FADE_MS = 500;

const FONT_MONO = '"JetBrains Mono", ui-monospace, Menlo, Consolas, monospace';
const FONT_SANS = 'Inter, system-ui, "Segoe UI", sans-serif';
const FONT_DISPLAY = 'Doto, Inter, system-ui, sans-serif';

/** The part of the 2D canvas API the drawing uses. A real context and a test fake both fit it. */
export type Ctx2D = Pick<
  CanvasRenderingContext2D,
  | 'fillStyle'
  | 'font'
  | 'textAlign'
  | 'textBaseline'
  | 'globalAlpha'
  | 'fillRect'
  | 'fillText'
  | 'measureText'
  | 'beginPath'
  | 'moveTo'
  | 'arcTo'
  | 'closePath'
  | 'fill'
  | 'rect'
  | 'clip'
  | 'save'
  | 'restore'
>;

export interface DrawOpts {
  /** The second wall: every string drawn goes through it. */
  guard: GuardCounter;
}

/** Where and how big the terminal is drawn. */
export interface TerminalLayout {
  fontPx: number;
  /** True when the lines are still wider than the area at the smallest font, so the right side is cut. */
  crop: boolean;
  /** Rows that fit in the area at this font. When `rowsShown` is more, the top ones are cut (see `firstRow`). */
  maxRows: number;
  /** Left edge of the first column: the text is centred when it is narrower than the area. */
  x: number;
}

/**
 * Font size for the terminal scene. The font grows until the text fills the area's width (1200 px) or its height
 * (540 px), at most 26 px; it never goes under 12 px, so a very tall or very wide terminal is cut instead of
 * becoming unreadable. A text narrower than the area is centred.
 */
export function layoutTerminal(cols: number, rowsShown: number): TerminalLayout {
  const c = Math.max(1, cols);
  const rows = Math.max(1, rowsShown);
  const fit = Math.min(TERM.w / (c * CELL_W), TERM.h / (rows * LINE_H));
  const fontPx = Math.min(TERM_MAX_FONT, Math.max(TERM_MIN_FONT, fit));
  const width = c * CELL_W * fontPx;
  return {
    fontPx,
    crop: width > TERM.w + 0.001,
    maxRows: Math.max(1, Math.floor(TERM.h / (fontPx * LINE_H) + 0.001)),
    x: width >= TERM.w - 0.01 ? TERM.x : TERM.x + (TERM.w - width) / 2,
  };
}

/**
 * The first shown row when the rows do not all fit. The bottom is kept (the prompt and the newest output live
 * there), unless the cursor is above the window; then the window starts at the cursor.
 */
export function firstRow(rowsShown: number, maxRows: number, cursorRow: number | null): number {
  const bottom = Math.max(0, rowsShown - maxRows);
  if (cursorRow !== null && cursorRow >= 0 && cursorRow < bottom) return cursorRow;
  return bottom;
}

/** Draws the frame of the clip at `tMs` (clip time, from 0). A time past the end draws the end card. */
export function drawFrame(ctx: Ctx2D, plan: ClipPlan, tl: Timeline, tMs: number, opts: DrawOpts): void {
  const t = Math.min(tl.totalMs, Math.max(0, Number.isFinite(tMs) ? tMs : 0));
  const scene = sceneAt(tl.scenes, t);
  fill(ctx, CLIP_COLORS.bg, 0, 0, CLIP_WIDTH, CLIP_HEIGHT);
  if (scene) drawScene(ctx, plan, scene, t, opts);
  drawCaptions(ctx, tl.captions, t, opts);
  label(ctx, MARK_LABEL, CLIP_WIDTH - 24, CLIP_HEIGHT - 20, { font: css(14, FONT_SANS), color: CLIP_COLORS.text, align: 'right', alpha: 0.4 }, opts);
  ctx.globalAlpha = 1;
}

function drawScene(ctx: Ctx2D, plan: ClipPlan, scene: Scene, t: number, opts: DrawOpts): void {
  switch (scene.kind) {
    case 'title':
      drawTitle(ctx, scene.agent, scene.request, opts);
      return;
    case 'replay':
      if (scene.mode === 'terminal' && plan.replay.kind === 'terminal') {
        drawTerminal(ctx, plan.replay, t - scene.startMs, opts);
      } else {
        drawActivity(ctx, plan.steps, t - scene.startMs, opts);
      }
      return;
    case 'result':
      drawResult(ctx, plan, opts);
      return;
    case 'end':
      drawEnd(ctx, scene, t, opts);
      return;
  }
}

function drawTitle(ctx: Ctx2D, agent: string, request: string, opts: DrawOpts): void {
  label(ctx, agent, 80, 200, { font: css(22, FONT_SANS), color: CLIP_COLORS.muted }, opts);
  const block = fitLines(ctx, request, opts, { family: FONT_SANS, weight: 600, maxW: CLIP_WIDTH - 160, maxLines: 2, startPx: 56, minPx: 28 });
  block.lines.forEach((line, i) => {
    label(ctx, line, 80, 300 + i * block.px * 1.25, { font: css(block.px, FONT_SANS, 600), color: CLIP_COLORS.text }, opts);
  });
  fill(ctx, CLIP_COLORS.muted, 80, 420, 120, 2);
}

function drawTerminal(ctx: Ctx2D, replay: TerminalReplay, local: number, opts: DrawOpts): void {
  const frame = frameAt(replay.frames, local);
  const shown = Math.max(1, replay.rowTo - replay.rowFrom + 1);
  const layout = layoutTerminal(replay.cols, shown);
  const { fontPx } = layout;
  const cellW = fontPx * CELL_W;
  const lineH = fontPx * LINE_H;
  clipped(ctx, TERM.x, TERM.y, TERM.w, TERM.h, () => {
    if (!frame) return;
    const caret = caretOf(frame, replay.rowFrom, shown);
    const skip = firstRow(shown, layout.maxRows, caret ? caret.row : null);
    frame.rows.forEach((index, row) => {
      if (row < skip || row >= skip + layout.maxRows) return;
      drawRow(ctx, replay.rowTable[index] ?? [], layout.x, TERM.y + (row - skip) * lineH, lineH, fontPx, cellW, opts);
    });
    if (caret && caret.row >= skip && caret.row < skip + layout.maxRows) {
      fill(ctx, CLIP_COLORS.text, layout.x + caret.col * cellW, TERM.y + (caret.row - skip) * lineH, 2, lineH);
    }
  });
}

/** One row of runs. Each run takes its cells in order, so a wide or styled run never shifts the rest. */
function drawRow(ctx: Ctx2D, runs: readonly Run[], x0: number, y: number, lineH: number, fontPx: number, cellW: number, opts: DrawOpts): void {
  let col = 0;
  for (const run of runs) {
    const cells = cellCount(run.t);
    const x = x0 + col * cellW;
    const w = cells * cellW;
    const style = run.s | 0;
    const inverse = (style & 4) !== 0;
    if ((style & 16) !== 0) fill(ctx, CLIP_COLORS.raised, x, y, w, lineH);
    if (inverse) fill(ctx, CLIP_COLORS.text, x, y, w, lineH);
    const color = inverse ? CLIP_COLORS.bg : (style & 2) !== 0 ? CLIP_COLORS.muted : CLIP_COLORS.text;
    label(ctx, run.t, x, y, { font: css(fontPx, FONT_MONO, (style & 1) !== 0 ? 700 : 400), color }, opts, 'top');
    if ((style & 8) !== 0) fill(ctx, color, x, y + fontPx * 1.1, w, 1);
    col += cells;
  }
}

/** The activity scene: one line per step, each appearing 0.9 s after the last. Only the newest 14 stay. */
function drawActivity(ctx: Ctx2D, steps: readonly Step[], local: number, opts: DrawOpts): void {
  const visible = steps.filter((_, i) => i * ACTIVITY_STEP_MS <= local).slice(-ACTIVITY_MAX_LINES);
  const nameFont = css(24, FONT_SANS, 600);
  const textFont = css(24, FONT_SANS);
  clipped(ctx, TERM.x, TERM.y, TERM.w, TERM.h, () => {
    visible.forEach((step, row) => {
      const y = TERM.y + row * ACTIVITY_LINE_H;
      let x = TERM.x;
      if (step.name !== '') {
        label(ctx, step.name, x, y, { font: nameFont, color: CLIP_COLORS.muted }, opts, 'top');
        x += measure(ctx, nameFont, step.name, opts) + 16;
      }
      label(ctx, step.summary, x, y, { font: textFont, color: CLIP_COLORS.text }, opts, 'top');
      if (step.ok === false) {
        x += measure(ctx, textFont, step.summary, opts) + 16;
        label(ctx, FAILED_LABEL, x, y, { font: textFont, color: CLIP_COLORS.muted }, opts, 'top');
      }
    });
  });
}

function drawCaptions(ctx: Ctx2D, captions: readonly Caption[], t: number, opts: DrawOpts): void {
  for (const caption of captions) {
    if (t < caption.startMs || t >= caption.endMs) continue;
    if (caption.role === 'badge') {
      label(ctx, caption.text, CLIP_WIDTH - 40, 32, { font: css(16, FONT_SANS), color: CLIP_COLORS.muted, align: 'right' }, opts);
    } else {
      drawPill(ctx, caption.text, opts);
    }
  }
}

/** The caption pill at the bottom: one line, shrunk to 16 px before it is cut with an ellipsis. */
function drawPill(ctx: Ctx2D, text: string, opts: DrawOpts): void {
  const fitted = fitLines(ctx, text, opts, { family: FONT_SANS, maxW: CAPTION.maxW - CAPTION.padX * 2, maxLines: 1, startPx: 24, minPx: 16 });
  const line = fitted.lines[0];
  if (line === undefined) return;
  const font = css(fitted.px, FONT_SANS);
  const w = measure(ctx, font, line, opts) + CAPTION.padX * 2;
  const x = (CLIP_WIDTH - w) / 2;
  fillRound(ctx, CLIP_COLORS.raised, x, CAPTION.y, w, CAPTION.h, CAPTION.h / 2);
  label(ctx, line, CLIP_WIDTH / 2, CAPTION.y + CAPTION.h / 2, { font, color: CLIP_COLORS.text, align: 'center' }, opts, 'middle');
}

function drawResult(ctx: Ctx2D, plan: ClipPlan, opts: DrawOpts): void {
  const X = 80;
  const { files, checks, sentence } = plan.result;
  let checksY = 340;
  if (files && files.count === 0) {
    // A read-only turn: say so in words instead of "0 files changed +0 -0".
    label(ctx, NO_FILES_LABEL, X, 220, { font: css(64, FONT_SANS, 600), color: CLIP_COLORS.text }, opts);
    const block = fitLines(ctx, sentence, opts, { family: FONT_SANS, maxW: CLIP_WIDTH - 160, maxLines: 2, startPx: 32, minPx: 22 });
    block.lines.forEach((line, i) => {
      label(ctx, line, X, 290 + i * block.px * 1.3, { font: css(block.px, FONT_SANS), color: CLIP_COLORS.muted }, opts);
    });
    checksY = 290 + block.lines.length * block.px * 1.3 + 50;
  } else if (files) {
    const head = `${files.count} ${files.count === 1 ? 'file' : 'files'} changed`;
    const big = fitLines(ctx, head, opts, { family: FONT_SANS, weight: 600, maxW: CLIP_WIDTH - 160, maxLines: 1, startPx: 64, minPx: 32 });
    label(ctx, big.lines[0] ?? '', X, 220, { font: css(big.px, FONT_SANS, 600), color: CLIP_COLORS.text }, opts);

    const numberFont = css(40, FONT_SANS, 600);
    label(ctx, `+${files.added}`, X, 290, { font: numberFont, color: CLIP_COLORS.text }, opts);
    const plusW = measure(ctx, numberFont, `+${files.added}`, opts);
    label(ctx, `-${files.deleted}`, X + plusW + 24, 290, { font: numberFont, color: CLIP_COLORS.muted }, opts);

    files.names.forEach((name, i) => {
      const line = fitLines(ctx, name, opts, { family: FONT_MONO, maxW: CLIP_WIDTH - 160, maxLines: 1, startPx: 24, minPx: 16 });
      label(ctx, line.lines[0] ?? '', X, 350 + i * 34, { font: css(line.px, FONT_MONO), color: CLIP_COLORS.muted }, opts);
    });
    checksY = 350 + files.names.length * 34 + 40;
  } else {
    const block = fitLines(ctx, sentence, opts, { family: FONT_SANS, weight: 600, maxW: CLIP_WIDTH - 160, maxLines: 2, startPx: 48, minPx: 28 });
    block.lines.forEach((line, i) => {
      label(ctx, line, X, 220 + i * block.px * 1.25, { font: css(block.px, FONT_SANS, 600), color: CLIP_COLORS.text }, opts);
    });
  }
  if (checks) label(ctx, checks, X, checksY, { font: css(24, FONT_SANS), color: CLIP_COLORS.text }, opts);
}

function drawEnd(ctx: Ctx2D, scene: Extract<Scene, { kind: 'end' }>, t: number, opts: DrawOpts): void {
  label(ctx, END_LABEL, CLIP_WIDTH / 2, CLIP_HEIGHT / 2 - 8, { font: css(64, FONT_DISPLAY), color: CLIP_COLORS.text, align: 'center' }, opts);
  const who = fitLines(ctx, scene.agent, opts, { family: FONT_SANS, maxW: CLIP_WIDTH - 240, maxLines: 1, startPx: 24, minPx: 16 });
  label(ctx, who.lines[0] ?? '', CLIP_WIDTH / 2, CLIP_HEIGHT / 2 + 48, { font: css(who.px, FONT_SANS), color: CLIP_COLORS.muted, align: 'center' }, opts);
  // The last half second fades the card to dark.
  const fadeStart = scene.endMs - END_FADE_MS;
  if (t > fadeStart) fill(ctx, CLIP_COLORS.bg, 0, 0, CLIP_WIDTH, CLIP_HEIGHT, Math.min(1, (t - fadeStart) / END_FADE_MS));
}

// ---- helpers ----

interface Style {
  font: string;
  color: string;
  align?: CanvasTextAlign;
  alpha?: number;
}

/** The one place text is drawn. The string is guarded first, so no caller can skip the second wall. */
function label(ctx: Ctx2D, value: string, x: number, y: number, style: Style, opts: DrawOpts, baseline: CanvasTextBaseline = 'alphabetic'): void {
  ctx.font = style.font;
  ctx.fillStyle = style.color;
  ctx.textAlign = style.align ?? 'left';
  ctx.textBaseline = baseline;
  ctx.globalAlpha = style.alpha ?? 1;
  ctx.fillText(opts.guard.guard(value), x, y);
}

function fill(ctx: Ctx2D, color: string, x: number, y: number, w: number, h: number, alpha = 1): void {
  ctx.globalAlpha = alpha;
  ctx.fillStyle = color;
  ctx.fillRect(x, y, w, h);
}

function fillRound(ctx: Ctx2D, color: string, x: number, y: number, w: number, h: number, r: number): void {
  ctx.globalAlpha = 1;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
  ctx.fill();
}

function clipped(ctx: Ctx2D, x: number, y: number, w: number, h: number, draw: () => void): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  draw();
  ctx.restore();
}

function measure(ctx: Ctx2D, font: string, value: string, opts: DrawOpts): number {
  ctx.font = font;
  return ctx.measureText(opts.guard.guard(value)).width;
}

function css(px: number, family: string, weight = 400): string {
  return `${weight} ${px}px ${family}`;
}

interface FitSpec {
  family: string;
  weight?: number;
  maxW: number;
  maxLines: number;
  startPx: number;
  minPx: number;
}

/**
 * Word-wraps `raw` into at most `maxLines` lines, shrinking the font from `startPx` down to `minPx` first. The text is
 * guarded before it is measured, so a secret cannot be split across two lines to slip past the guard.
 */
function fitLines(ctx: Ctx2D, raw: string, opts: DrawOpts, spec: FitSpec): { lines: string[]; px: number } {
  const value = opts.guard.guard(raw.replace(/\s+/g, ' ').trim());
  let px = spec.startPx;
  for (;;) {
    ctx.font = css(px, spec.family, spec.weight);
    const lines = wrap(ctx, value, spec.maxW);
    if (lines.length <= spec.maxLines || px <= spec.minPx) {
      return { lines: keepLines(ctx, lines, spec.maxLines, spec.maxW), px };
    }
    px = Math.max(spec.minPx, px - 4);
  }
}

function wrap(ctx: Ctx2D, text: string, maxW: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(' ')) {
    if (word === '') continue;
    const joined = line === '' ? word : `${line} ${word}`;
    if (ctx.measureText(joined).width <= maxW) {
      line = joined;
      continue;
    }
    if (line !== '') lines.push(line);
    line = ctx.measureText(word).width <= maxW ? word : `${cutTo(ctx, word, maxW - ctx.measureText('…').width)}…`;
  }
  if (line !== '') lines.push(line);
  return lines;
}

/** Keeps the first `max` lines; a cut last line ends with an ellipsis. */
function keepLines(ctx: Ctx2D, lines: string[], max: number, maxW: number): string[] {
  if (max <= 0) return [];
  if (lines.length <= max) return lines;
  const kept = lines.slice(0, max);
  const last = kept[max - 1] ?? '';
  kept[max - 1] = `${cutTo(ctx, last, maxW - ctx.measureText('…').width)}…`;
  return kept;
}

/** The longest prefix of `text` (in characters) that is at most `maxW` wide. */
function cutTo(ctx: Ctx2D, text: string, maxW: number): string {
  const chars = Array.from(text);
  while (chars.length > 0 && ctx.measureText(chars.join('')).width > maxW) chars.pop();
  return chars.join('');
}

function sceneAt(scenes: readonly Scene[], t: number): Scene | undefined {
  return scenes.find((scene) => t >= scene.startMs && t < scene.endMs) ?? scenes[scenes.length - 1];
}

/** The last key frame at or before `t` (frames are in time order). Before the first one, the first. */
function frameAt(frames: readonly Frame[], t: number): Frame | undefined {
  let lo = 0;
  let hi = frames.length - 1;
  let found: Frame | undefined = frames[0];
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (frames[mid]!.atMs <= t) {
      found = frames[mid];
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/** The caret in the drawn rows. The cursor is a screen position, so its row is taken from the first shown row. */
function caretOf(frame: Frame, rowFrom: number, shown: number): { row: number; col: number } | null {
  if (!frame.cursor) return null;
  const row = frame.cursor[0] - rowFrom;
  if (row < 0 || row >= shown) return null;
  return { row, col: frame.cursor[1] };
}

/** Cells a piece of text takes on the screen: East Asian wide and emoji characters take two. */
function cellCount(text: string): number {
  let cells = 0;
  for (const ch of text) cells += isWide(ch.codePointAt(0) ?? 0) ? 2 : 1;
  return cells;
}

function isWide(cp: number): boolean {
  return (
    (cp >= 0x1100 && cp <= 0x115f) ||
    (cp >= 0x2e80 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1f64f) ||
    (cp >= 0x1f900 && cp <= 0x1f9ff)
  );
}
