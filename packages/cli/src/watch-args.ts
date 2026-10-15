/** Flag validation for `puppet-master watch`. */

export const WATCH_USAGE =
  'usage: puppet-master watch <operation_id|handle> [--project-path P] [--timeout-ms N] [--stream]';

const WATCH_FLAGS = new Set(['project-path', 'project', 'timeout-ms', 'stream', 'help', 'h']);

/** `null` when the flags are acceptable, else the problem to print above the usage line. */
export function validateWatchFlags(flags: Record<string, string | boolean>): string | null {
  for (const key of Object.keys(flags)) {
    if (!WATCH_FLAGS.has(key)) {
      return `unknown flag --${key} (milliseconds are given as --timeout-ms N)`;
    }
  }
  if ('timeout-ms' in flags) {
    const value = flags['timeout-ms'];
    const parsed = typeof value === 'string' ? Number(value) : Number.NaN;
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return `--timeout-ms needs a positive number of milliseconds, got ${String(value)}`;
    }
  }
  return null;
}
