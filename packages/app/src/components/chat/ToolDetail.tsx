// The open part of a tool row: the files it touched as chips, then an edit's diff, a command, or the input as JSON,
// then the output. Agent text and tool output are data: shown as plain text only.
import type { ToolCallItem, ToolResultItem, ToolStep } from '../../lib/sessionChatModel';
import { kindOfCall } from './chatActivity';
import { editPairs, lineDiff, patchLines, patchOf, type DiffLine, type DiffOp } from './chatDiff';
import { baseName } from './chatFormat';

/** The diff of an edit or write call, or null when its input holds no old or new text. */
export function diffLinesOf(call: ToolCallItem): DiffLine[] | null {
  const kind = kindOfCall(call);
  if (kind !== 'edit' && kind !== 'write') return null;
  const patch = patchOf(call.input);
  if (patch !== null) return patchLines(patch);
  const pairs = editPairs(call.input);
  return pairs ? pairs.flatMap((pair) => lineDiff(pair.before, pair.after)) : null;
}

/** The command of a run call: its `command` text, or the words of a command list. */
export function commandOf(input: unknown): string | null {
  if (!input || typeof input !== 'object') return null;
  const command = (input as Record<string, unknown>).command;
  if (typeof command === 'string') return command;
  if (Array.isArray(command) && command.every((part) => typeof part === 'string')) return command.join(' ');
  return null;
}

export function jsonOf(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

const GUTTER: Record<DiffOp, string> = { same: ' ', add: '+', del: '-', head: '' };

export function DetailLabel({ children }: { children: string }) {
  return <p className="mt-1 text-[10px] uppercase tracking-wide text-pm-muted ck-chat-detail-label">{children}</p>;
}

/** Paths as chips: the file name shows, the full path is the tooltip. */
export function PathChips({ paths }: { paths: readonly string[] }) {
  return (
    <ul className="flex flex-wrap gap-1 ck-chat-chips" aria-label="Files">
      {paths.map((path) => (
        <li key={path} className="min-w-0">
          <span className="inline-block max-w-full truncate font-mono text-[11px] ck-chat-chip" title={path}>
            {baseName(path)}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** A unified-style diff: a `+` or `-` gutter, added lines filled and bold, removed lines dimmed and struck through. */
export function DiffBlock({ lines }: { lines: readonly DiffLine[] }) {
  return (
    <div className="max-h-64 overflow-auto rounded border border-pm-border bg-pm-panel py-1 font-mono text-[11px] ck-chat-diff" data-testid="tool-diff">
      {lines.map((line, index) => (
        <div key={index} className={`flex min-w-max ck-chat-diff-line ck-chat-diff-line--${line.op}`}>
          <span aria-hidden="true" className="w-4 shrink-0 select-none text-center ck-chat-diff-gutter">
            {GUTTER[line.op]}
          </span>
          <span className="whitespace-pre pr-2 ck-chat-diff-text">{line.text || ' '}</span>
        </div>
      ))}
    </div>
  );
}

/** A mono block with a scroll cap: the command, or the input as JSON. */
export function CodeBlock({ label, text }: { label: string; text: string }) {
  return (
    <>
      <DetailLabel>{label}</DetailLabel>
      <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-pm-border bg-pm-panel p-1.5 font-mono text-[11px] text-pm-muted ck-chat-output">
        {text}
      </pre>
    </>
  );
}

/** The tool's output (or that it has none yet), scrollable with a cap. */
export function OutputBlock({ result }: { result: ToolResultItem | null }) {
  return (
    <>
      <DetailLabel>Output</DetailLabel>
      <pre
        data-testid="tool-output"
        className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-pm-border bg-pm-panel p-1.5 font-mono text-[11px] text-pm-muted ck-chat-output"
      >
        {result ? result.output : 'No output yet.'}
      </pre>
    </>
  );
}

export function ToolDetail({ step }: { step: ToolStep }) {
  const { call, result } = step;
  const paths = [...new Set((call.paths ?? []).filter((path) => typeof path === 'string' && path !== ''))];
  const diff = diffLinesOf(call);
  const command = kindOfCall(call) === 'run' ? commandOf(call.input) : null;
  return (
    <div className="mt-0.5 space-y-1 pb-1 ck-chat-detail">
      {paths.length > 0 && <PathChips paths={paths} />}
      {diff ? (
        <>
          <DetailLabel>Changes</DetailLabel>
          <DiffBlock lines={diff} />
        </>
      ) : command !== null ? (
        <CodeBlock label="Command" text={command} />
      ) : call.input != null ? (
        <CodeBlock label="Input" text={jsonOf(call.input)} />
      ) : null}
      <OutputBlock result={result} />
    </div>
  );
}
