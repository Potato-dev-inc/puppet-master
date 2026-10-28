// Plain words for the What broke it result card (SPEC n33 section 3.7). The backend sends the title and the
// subtitle of each finding; this file builds the lines the window writes itself, with the same wording as
// `checkpoints/bisect/words.rs`. Nothing here names an agent except through the labels it is given.
import { kindOf, type FindingView, type SuspectView } from '../../lib/what-broke-it';

/** The backend's "Not sure." reasons that allow "Keep looking" (SPEC section 3.5 step 8). */
const BUDGET_REASONS = ['Ran out of runs.', 'Ran out of time.'];

/** `It broke cart.test.ts` / `It broke cart.test.ts and 1 more`. Same rule as `words::broke_line`. */
export function brokeLine(failing: string[]): string {
  if (failing.length === 0) return 'No test name was printed.';
  if (failing.length === 1) return `It broke ${failing[0]}`;
  return `It broke ${failing[0]} and ${failing.length - 1} more`;
}

/** The label of a turn whose summary is gone reads as "the agent" inside a sentence. */
function named(agentLabel: string): string {
  return agentLabel === 'An agent' ? 'the agent' : agentLabel;
}

/** `Undo only turn 7?` */
export function undoTitle(turn: number): string {
  return `Undo only turn ${turn}?`;
}

/** `Undo turn 7` */
export function undoButton(turn: number): string {
  return `Undo turn ${turn}`;
}

/** The confirm text. `files` is null when the turn's file count is unknown. */
export function undoText(turn: number, files: number | null, agentLabel: string): string {
  const where = files === null ? 'its files' : countFiles(files);
  return `This puts back what turn ${turn} changed in ${where}. Work from later turns stays. It doesn't change what ${named(agentLabel)} remembers. A save point is made first, so you can undo this.`;
}

/** The refusal that replaces the confirm text. Names the first file and counts the rest. */
export function undoRefusedText(paths: string[]): string {
  if (paths.length === 0) {
    return "Can't undo only this turn. A later change touched the same lines. Nothing was changed.";
  }
  if (paths.length === 1) {
    return `Can't undo only this turn. A later change touched the same lines in ${paths[0]}. Nothing was changed.`;
  }
  return `Can't undo only this turn. A later change touched the same lines in ${paths[0]} and ${paths.length - 1} more. Nothing was changed.`;
}

/** `Undone at 14:31.` */
export function undoneLine(atMs: number): string {
  return `Undone at ${clock(atMs)}.`;
}

/** Local clock time as `14:31`. */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
}

/** `1 file` or `4 files`. */
export function countFiles(n: number): string {
  return n === 1 ? '1 file' : `${n} files`;
}

/** The "Keep looking" button shows only when the search ran out of runs or time. */
export function canKeepLooking(finding: Pick<FindingView, 'kind' | 'title'>): boolean {
  return kindOf(finding) === 'not_sure' && BUDGET_REASONS.some((reason) => finding.title.includes(reason));
}

/** One line after "Check it again": the tests pass, or the names that still fail. */
export function recheckLine(finding: Pick<FindingView, 'kind' | 'title' | 'failing'>): string {
  if (kindOf(finding) === 'tests_pass') return 'The tests pass now.';
  if (finding.failing.length > 0) return `They still fail: ${finding.failing.join(', ')}`;
  return finding.title;
}

/** One suspect row: `Codex · turn 7` and its sentence (already scrubbed by the backend). */
export function suspectTitle(s: Pick<SuspectView, 'agentLabel' | 'turn'>): string {
  return `${s.agentLabel} · turn ${s.turn}`;
}

/** Hides folder paths (Windows drive or home folders) in text that may be copied. */
export function maskPaths(text: string): string {
  return text
    .replace(/[A-Za-z]:[\\/][^\s'"<>|]*/g, '~')
    .replace(/\/(?:home|Users|root)\/[^\s'"<>|]*/g, '~');
}

/**
 * The text of "Copy details": the result in plain lines, with the redacted tail and no folder path.
 * The tail is already redacted by the backend; `maskPaths` is a second guard.
 */
export function detailsText(finding: FindingView): string {
  const lines: string[] = [finding.title];
  if (finding.subtitle) lines.push(finding.subtitle);
  if (kindOf(finding) === 'culprit') lines.push(brokeLine(finding.failing));
  for (const s of finding.suspects) {
    const sentence = s.sentence ? `: ${s.sentence}` : '';
    lines.push(`${suspectTitle(s)}${sentence}`);
  }
  if (finding.tail.trim()) {
    lines.push('', 'Last lines of the failing run:', finding.tail.trimEnd());
  }
  return maskPaths(lines.join('\n'));
}

