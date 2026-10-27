// What kind of work a tool call does, for the chat's activity line ("Read 6 files · Ran 2 commands").
// This file is the one table of tool names. A name not listed counts as "other" (the line says "Used N tools").
// Names match without case, and a namespace prefix is dropped (`mcp__server__read` and `server.read` are `read`).
// Generic code only: no agent is named outside these lists.

export type ToolKind = 'read' | 'search' | 'edit' | 'write' | 'run' | 'web' | 'helper' | 'other';

/** The tool names of each kind. A new agent's tool goes in its kind's list. */
export const TOOL_KIND_NAMES: Record<Exclude<ToolKind, 'other'>, readonly string[]> = {
  read: ['read', 'read_file', 'view', 'cat', 'notebookread', 'open'],
  search: ['grep', 'glob', 'ls', 'search', 'find', 'list', 'list_dir', 'file_search'],
  edit: ['edit', 'multiedit', 'str_replace', 'str_replace_based_edit_tool', 'apply_patch', 'patch', 'notebookedit'],
  write: ['write', 'write_file', 'create', 'create_file'],
  run: ['bash', 'bashoutput', 'killshell', 'shell', 'exec', 'exec_command', 'local_shell', 'run', 'terminal'],
  web: ['webfetch', 'websearch', 'web_search', 'fetch'],
  helper: ['agent', 'task'],
};

const KIND_OF_NAME = new Map<string, ToolKind>(
  (Object.keys(TOOL_KIND_NAMES) as (keyof typeof TOOL_KIND_NAMES)[]).flatMap((kind) => TOOL_KIND_NAMES[kind].map((name) => [name, kind] as const)),
);

/** The name a tool is looked up by: lower case, without a namespace prefix. Empty when the tool has no name. */
export function toolKeyOf(name: string): string {
  return (name.split(/__|\./).pop() ?? name).trim().toLowerCase();
}

export interface ToolCallShape {
  name: string;
  summary?: string;
  input?: unknown;
  /** The call changes files (an edit or write). */
  writes?: boolean;
  /** The call starts a helper agent. */
  helper?: boolean;
}

/**
 * The kind of one tool call. A helper flag wins; then the name table; then what the input holds (a `command` is a
 * run, a `patch` or `edits` an edit); then a command line read off the screen (`$ npm test`); then `writes`.
 */
export function classifyTool(call: ToolCallShape): ToolKind {
  if (call.helper) return 'helper';
  const key = toolKeyOf(call.name ?? '');
  if (key) {
    const named = KIND_OF_NAME.get(key);
    if (named) return named;
  } else if (/^\$\s/.test(call.summary ?? '')) {
    return 'run';
  }
  if (call.input && typeof call.input === 'object') {
    const input = call.input as Record<string, unknown>;
    if (typeof input.command === 'string') return 'run';
    if (typeof input.patch === 'string' || Array.isArray(input.edits)) return 'edit';
  }
  return call.writes ? 'edit' : 'other';
}
