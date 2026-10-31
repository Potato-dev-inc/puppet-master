import type { ExtraTool } from "./loop.ts";

export interface HelperRequest {
  requestId: string;
  conversationId: string;
  /** at most 600 characters */
  task: string;
  /** a folder name only, never a path */
  project?: string;
}

export const TASK_MAX = 600;

/** What the model is told after it asks. It must not announce work that did not start. */
export const HELPER_ASKED_TEXT = "I asked them. Do not say it has started.";

const ALREADY_ASKED_TEXT = "You already asked once this turn. Do not ask again.";

/**
 * A project is a folder name. Anything that looks like a path (a separator,
 * a drive letter, a dot-dot) is dropped, so a path the model writes never
 * travels: Puppet Master matches names against its own recent folders.
 */
export function cleanProject(raw: unknown): string | undefined {
  if (typeof raw !== "string") return undefined;
  const name = raw.trim();
  if (name === "" || name.length > 80) return undefined;
  if (/[\\/:]/.test(name) || name.includes("..")) return undefined;
  if (/[\u0000-\u001f]/.test(name)) return undefined;
  return name;
}

export const ASK_HELPER_SPEC = {
  name: "ask_helper",
  description:
    "Ask your owner to start a helper (a coding agent) for a job that needs real work on their computer. " +
    "This only asks: they decide on their PC. After you call it, do not say the work has started. " +
    "Say you asked, and wait to be told what happened. Use it only when your owner asks for work to be done.",
  parameters: {
    type: "object",
    properties: {
      task: { type: "string", description: "What to do, in one or two plain sentences (600 characters at most)" },
      project: { type: "string", description: "The folder name to work in, if the owner named one. A name, not a path." },
    },
    required: ["task"],
  },
} as const;

/**
 * `ask_helper`, for one conversation. It is added to an Agent only by pm.ts.
 * It never starts anything: it emits `helper_request`, and Puppet Master decides.
 * One request per turn, so a model that loops cannot fill the owner's screen.
 */
export function helperTool(opts: {
  conversationId: string;
  emit: (req: HelperRequest) => void;
  newId: () => string;
}): ExtraTool {
  let askedInTurn = -1;
  return {
    spec: { ...ASK_HELPER_SPEC, parameters: { ...ASK_HELPER_SPEC.parameters } },
    run(args, ctx) {
      const task = typeof args["task"] === "string" ? args["task"].trim().slice(0, TASK_MAX) : "";
      if (task === "") return "Nothing to ask. Say what the job is.";
      if (askedInTurn === ctx.turn) return ALREADY_ASKED_TEXT;
      askedInTurn = ctx.turn;

      const project = cleanProject(args["project"]);
      opts.emit({
        requestId: opts.newId(),
        conversationId: opts.conversationId,
        task,
        ...(project === undefined ? {} : { project }),
      });
      return HELPER_ASKED_TEXT;
    },
  };
}
