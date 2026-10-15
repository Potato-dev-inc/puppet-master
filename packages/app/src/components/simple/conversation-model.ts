// The Conversation tab is drawn from the run transcript (the same chunks the Transcript tab shows)
// and from the last turn summary. This file only turns them into plain items; it fetches nothing.
import type { PaneTranscript, TurnSummary } from '../../lib/cockpit-api';
import { F, T } from './copy';
import { HOUSE_RULES } from './templates';

export type ConversationItem =
  | { kind: 'you'; text: string }
  | { kind: 'agent'; text: string }
  | { kind: 'step'; text: string }
  | { kind: 'note'; text: string }
  | { kind: 'problem'; text: string };

const YOU = new Set(['user_task', 'user_steer', 'user']);
const SAYS = new Set(['assistant_final', 'agent', 'result', 'success']);
/** Chunks about how the message travelled: nothing she needs. */
const HIDDEN = new Set(['delivery', 'delivery_trace', 'diagnostic', 'startup', 'stdout', 'stderr', 'permission']);

const TOOL_LINE = /^[●⏺•*]\s*([A-Za-z_]+)\((.*)\)\s*$/;
const TOOL_OUTPUT = /^\s*[⎿└|]/;

function fileOf(arg: string): string {
  const first = arg.split(',')[0].replace(/^["'`]|["'`]$/g, '').trim();
  const name = first.split(/[\\/]/).filter(Boolean).pop() ?? first;
  return name.length > 40 ? `${name.slice(0, 39)}…` : name;
}

/** One tool line ("● Write(tool.html)") in plain words. `null` when the line is not a tool line. */
export function stepText(line: string): string | null {
  const match = TOOL_LINE.exec(line.trim());
  if (!match) return null;
  const [, tool, arg] = match;
  switch (tool.toLowerCase()) {
    case 'read':
      return F.stepRead(fileOf(arg));
    case 'write':
      return F.stepMade(fileOf(arg));
    case 'edit':
    case 'multiedit':
    case 'update':
    case 'notebookedit':
      return F.stepChanged(fileOf(arg));
    case 'glob':
    case 'grep':
    case 'ls':
    case 'search':
      return F.stepSearched();
    default:
      return F.stepRan();
  }
}

/** Her message without the house rules PM added after it (they are shown in "PM also asks it to"). */
export function withoutRules(text: string): string {
  const rules = ` ${HOUSE_RULES.join(' ')}`;
  const trimmed = text.trim();
  return (trimmed.endsWith(rules) ? trimmed.slice(0, -rules.length) : trimmed).trim();
}

function pushText(items: ConversationItem[], raw: string) {
  const text: string[] = [];
  const flush = () => {
    const joined = text.join('\n').trim();
    if (joined) items.push({ kind: 'agent', text: joined });
    text.length = 0;
  };
  for (const line of raw.split(/\r?\n/)) {
    if (TOOL_OUTPUT.test(line)) continue;
    const step = stepText(line);
    if (step) {
      flush();
      items.push({ kind: 'step', text: step });
    } else {
      text.push(line.replace(/^[●⏺•]\s+/, ''));
    }
  }
  flush();
}

export function conversationItems(transcript: PaneTranscript | null | undefined): ConversationItem[] {
  const items: ConversationItem[] = [];
  for (const chunk of transcript?.chunks ?? []) {
    if (YOU.has(chunk.stream)) {
      const words = withoutRules(chunk.text);
      if (words) items.push({ kind: 'you', text: words });
    } else if (SAYS.has(chunk.stream)) {
      pushText(items, chunk.text);
    } else if (chunk.stream === 'interrupted') {
      items.push({ kind: 'note', text: T.stateStopped });
    } else if (chunk.stream === 'execution_failure') {
      items.push({ kind: 'problem', text: T.somethingWrong });
    } else if (!HIDDEN.has(chunk.stream) && chunk.text.trim()) {
      // An unknown stream: show its words (they are the agent's), never its tag.
      pushText(items, chunk.text);
    }
  }
  return items;
}

/** The Summary block at the end: what the last step did, from the numbers (not from the Full-mode sentence). */
export function summarySentence(summary: Pick<TurnSummary, 'files'> | null | undefined, agent: string): string | null {
  if (!summary || !summary.files) return null;
  return F.changedSummary(agent, summary.files.count);
}
