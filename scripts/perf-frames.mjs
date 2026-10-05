// Screen-frame analysis shared by perf-latency.mjs and perf-summarize.mjs.
// A frame is {t, text} (pane screen when it changed) or {t, status} (pane status change).

/** The reply is on screen when a line (other than the echoed prompt) is just the word PONG. */
export const REPLY_RE = /^[\s│┃|•●⬢▌>*-]*PONG[\s│┃|]*$/;

export function frameAnalysis(frames, t0, finishedAt) {
  const shown = frames.filter((f) => !f.status);
  const first = shown[0];
  const replyLines = (f) => f.text.split('\n').filter((l) => REPLY_RE.test(l)).length;
  // A follow-up starts on a screen that already holds the previous turn's PONG: count new ones only.
  const already = first ? replyLines(first) : 0;
  const reply = shown.find((f) => replyLines(f) > already);
  const settledFrames = finishedAt ? shown.filter((f) => f.t <= finishedAt) : shown;
  const lastChange = settledFrames[settledFrames.length - 1];
  return {
    first_frame_ms: first ? first.t - t0 : null,
    reply_visible_ms: reply ? reply.t - t0 : null,
    reply_visible_at: reply?.t ?? null,
    screen_final_ms: lastChange ? lastChange.t - t0 : null,
    screen_final_at: lastChange?.t ?? null,
    frames: shown.length,
    status_trace: frames.filter((f) => f.status).map((f) => `${f.status}@${f.t - t0}`).join(' '),
  };
}
