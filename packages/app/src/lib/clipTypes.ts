// The clip plan as the webview gets it from Rust (camelCase, the same shape as `clip/model.rs`). Types only, no code.
// The plan is already redacted in Rust; the webview draws its strings and never reads raw terminal bytes.

/** How the turn ended. */
export type ClipOutcome = 'completed' | 'interrupted' | 'failed';

/** One run of text in a terminal row. `s` holds the style bits: 1 bold, 2 dim, 4 inverse, 8 underline, 16 background. */
export interface Run {
  t: string;
  s: number;
}

/** One key frame of the terminal replay. `rows` are indexes into `rowTable`. */
export interface Frame {
  atMs: number;
  rows: number[];
  /** Position `[row, col]` of the text caret, or null when it is hidden. */
  cursor: [number, number] | null;
}

/** The terminal replay: frames of the pane's own screen, sped up. */
export interface TerminalReplay {
  kind: 'terminal';
  cols: number;
  rows: number;
  rowFrom: number;
  rowTo: number;
  /** How much faster than real time the replay runs (1 = real time). */
  speedX: number;
  lengthMs: number;
  rowTable: Run[][];
  frames: Frame[];
}

/** The steps of the turn, shown as lines, when there is no terminal record. */
export interface ActivityReplay {
  kind: 'activity';
  lengthMs: number;
}

/** Nothing to show (for example a shell pane without turns). */
export interface NoneReplay {
  kind: 'none';
}

export type Replay = TerminalReplay | ActivityReplay | NoneReplay;

/** One tool call of the turn. `atMs` is clip time (from the start of the replay). */
export interface Step {
  atMs: number;
  name: string;
  summary: string;
  paths: string[];
  writes: boolean;
  helper: boolean;
  /** The result of the call: true ok, false failed, null when no result was seen. */
  ok: boolean | null;
}

export interface FilesFact {
  count: number;
  added: number;
  deleted: number;
  /** Last names only, at most 6. */
  names: string[];
}

export interface ResultFacts {
  files: FilesFact | null;
  /** E.g. "all checks passed (2/2)", or null when none ran. */
  checks: string | null;
  sentence: string;
}

export interface ClipPlan {
  token: string;
  /** Display label of the agent (a name the person sees, never an id). */
  agent: string;
  /** The person's message, one line, at most 140 characters. */
  request: string;
  turnMs: number;
  outcome: ClipOutcome;
  replay: Replay;
  steps: Step[];
  result: ResultFacts;
  /** Plain sentences for the sheet, at most 3. */
  notes: string[];
  /** Distinct screen rows that redaction changed. */
  hidden: number;
}
