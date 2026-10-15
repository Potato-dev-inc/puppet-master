/** Nudge timing: wait this long after the last action (lets a size claim land first)... */
export const NUDGE_SETTLE_MS = 350;
/** ...and never redraw more often than this. */
export const NUDGE_MIN_GAP_MS = 1500;

/**
 * Coalesces "the user did something" signals into one redraw: it fires `NUDGE_SETTLE_MS` after the
 * last request, never sooner than `NUDGE_MIN_GAP_MS` after the previous redraw. `skipIfScrolledUp`
 * is true unless some request in the batch asked for the redraw even while the user reads history.
 * Shared by the phone (PwaApp) and the desktop windows.
 */
export class NudgeScheduler {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private lastAt = 0;
  private skip = true;

  constructor(
    private readonly fire: (skipIfScrolledUp: boolean) => void,
    private readonly now: () => number = Date.now,
  ) {}

  request(skipIfScrolledUp: boolean): void {
    this.skip = this.skip && skipIfScrolledUp;
    if (this.timer !== null) clearTimeout(this.timer);
    const wait = Math.max(NUDGE_SETTLE_MS, NUDGE_MIN_GAP_MS - (this.now() - this.lastAt));
    this.timer = setTimeout(() => {
      this.timer = null;
      this.lastAt = this.now();
      const skip = this.skip;
      this.skip = true;
      this.fire(skip);
    }, wait);
  }

  cancel(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}
