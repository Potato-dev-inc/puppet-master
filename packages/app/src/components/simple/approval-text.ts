// Approvals in Simple mode: one short question, Yes or No (IDEAS-R8-PLAIN-MODE §4.5).
//
// The Rust risk rules (`prompt_risk/rules.rs`) send a headline and a reason in the words a developer
// reads. Simple shows its own plain sentence, keyed by that headline. `approval-text.test.ts` checks
// that every key still exists in rules.rs, so a renamed rule cannot silently fall through to the
// generic sentence.
//
// High risk is never asked: Simple answers the agent's own No and says so (a "blocked" card).
// There is no "Always allow" here: the broad answers are not drawn at all.
import type { PanePrompt, PromptChoice, RiskLevel } from '../../lib/cockpit-api';
import { isBroadChoice, isDenyChoice } from '../../lib/approval-risk';

export interface ApprovalWords {
  /** The verb phrase after "<agent> wants to": "look at your files". */
  what: string;
  /** One sentence from a fixed set: is it safe, and what to check. */
  safe: string;
}

const NOTHING_CHANGES = 'It does not change your files.';
const CAN_GO_BACK = 'You can go back.';
const CHECK_FIRST = 'Say Yes only if you expect it.';
const STAYS_INSIDE = 'It stays inside your project.';

/** Headlines that mean "this step only reads". Low risk. */
const READ_ONLY = ['Look at files', 'Look at git', 'Look at git history', 'Look at system settings', 'Check a version', 'Look up package info', 'Preview files git would delete', 'Open a shell'];

const READ_WORDS: ApprovalWords = { what: 'look at your files', safe: NOTHING_CHANGES };

/** Headlines Simple cannot put in its own words: the agent's screen shows instead (see screen-model.ts). */
export const UNREADABLE = ['Answer the agent', 'Finish the startup screen', 'Allow without asking again'];

const MEDIUM: Record<string, ApprovalWords> = {
  'Trust this folder': { what: 'work in this folder', safe: 'It is a copy that Puppet Master made for you.' },
  'Change a file': { what: 'change a file', safe: CHECK_FIRST },
  'Run a command': { what: 'run a step that Puppet Master does not know', safe: CHECK_FIRST },
  'Change files in this project': { what: 'change files in your project', safe: CAN_GO_BACK },
  'Change files': { what: 'change files in your project', safe: CAN_GO_BACK },
  'Connect to another computer': { what: 'use the internet', safe: 'It may download something.' },
  'Download and run a package': { what: 'download a program and use it', safe: CHECK_FIRST },
  'Install packages for this project': { what: 'add programs to your project', safe: CHECK_FIRST },
  'Read a file that may hold secrets': { what: 'open a file with passwords in it', safe: CHECK_FIRST },
  'Work outside this project': { what: 'look outside your project folder', safe: CHECK_FIRST },
  'Look at files outside this project': { what: 'look outside your project folder', safe: CHECK_FIRST },
  'Use files outside this project': { what: 'use files outside your project folder', safe: CHECK_FIRST },
  'Copy files from outside this project': { what: 'copy files from outside your project', safe: CHECK_FIRST },
  'Delete files': { what: 'delete files in your project', safe: CAN_GO_BACK },
  'Delete a folder': { what: 'delete a folder in your project', safe: CAN_GO_BACK },
  'Start a program': { what: 'start another program', safe: CHECK_FIRST },
  'Stop a running program': { what: 'stop a program', safe: CHECK_FIRST },
};

const LOW: Record<string, ApprovalWords> = {
  'Change a file in this project': { what: 'change a file in your project', safe: CAN_GO_BACK },
  'Run a simple command': { what: 'run a small step', safe: 'Nothing changes.' },
  'Run tests': { what: 'check that your project works', safe: 'Nothing changes.' },
  'Check or format the code': { what: 'check your project', safe: 'Nothing changes.' },
  'Check the code': { what: 'check your project', safe: 'Nothing changes.' },
  'Build or start the project': { what: 'start your project to try it', safe: STAYS_INSIDE },
  'Start the project': { what: 'start your project to try it', safe: STAYS_INSIDE },
  'Build or check the project': { what: 'check your project', safe: STAYS_INSIDE },
  'Create files or folders': { what: 'make a new file', safe: CAN_GO_BACK },
  'Delete build files': { what: 'remove files it can make again', safe: STAYS_INSIDE },
};

/** What each high-risk headline would have done. Never asked, only told. */
const BLOCKED: Record<string, string> = {
  'Change a file outside this project': 'change a file outside your project',
  'Change files outside this project': 'change files outside your project',
  'Delete files outside this project': 'delete files outside your project',
  'Delete a folder outside this project': 'delete a folder outside your project',
  'Run with administrator rights': 'take full control of this computer',
  'Change system settings': 'change settings of this computer',
  'Change permissions outside this project': 'change who can use files outside your project',
  'Install a package for the whole computer': 'add programs to the whole computer',
  'Run hidden code': 'run code that nobody can read',
  'Run text as code': 'run code that nobody can read',
  'Send a file that may hold secrets to the internet': 'send a file with passwords to the internet',
  'Change a file that may hold secrets': 'change a file with passwords in it',
  'Overwrite history on the remote': 'change saved work on another computer',
  'Throw away your latest changes': 'throw away your last work',
  'Delete every untracked and ignored file': 'delete many files',
  'Delete untracked files': 'delete files',
  'Rewrite git history': 'change your saved versions',
  "Erase git's safety net": 'delete your saved versions',
  'Publish to a public registry': 'put something online for everyone',
  'Change your npm account': 'change how you sign in online',
};

const GENERIC: Record<RiskLevel, ApprovalWords> = {
  low: { what: 'do one small step', safe: STAYS_INSIDE },
  medium: { what: 'do a step that may change things', safe: CHECK_FIRST },
  high: { what: 'do something unsafe', safe: '' },
};

export interface SimpleApproval {
  /** `ask`: Yes or No. `blocked`: PM says No by itself. */
  kind: 'ask' | 'blocked';
  level: RiskLevel;
  what: string;
  safe: string;
  /** The headline Simple matched (a test reads this), or `null` for a generic sentence. */
  matched: string | null;
}

/** The plain version of a prompt with a risk, or `null` for a prompt without one. */
export function simpleApproval(prompt: Pick<PanePrompt, 'risk'>): SimpleApproval | null {
  const risk = prompt.risk;
  if (!risk || UNREADABLE.includes(risk.headline)) return null;
  if (risk.level === 'high') {
    const what = BLOCKED[risk.headline];
    return { kind: 'blocked', level: 'high', what: what ?? GENERIC.high.what, safe: '', matched: what ? risk.headline : null };
  }
  // A sentence is used only at the level it was written for: "Nothing changes" never sits on a medium step.
  const table =
    risk.level === 'low'
      ? READ_ONLY.includes(risk.headline)
        ? READ_WORDS
        : LOW[risk.headline]
      : MEDIUM[risk.headline];
  const words = table ?? GENERIC[risk.level];
  return { kind: 'ask', level: risk.level, what: words.what, safe: words.safe, matched: table ? risk.headline : null };
}

/** Yes and No only. The broad "always allow" answers are dropped; Yes is the first non-deny answer. */
export function yesNo(choices: PromptChoice[]): { yes: PromptChoice | null; no: PromptChoice | null } {
  const plain = choices.filter((c) => !isBroadChoice(c));
  const no = plain.find(isDenyChoice) ?? null;
  const yes = plain.find((c) => !isDenyChoice(c)) ?? null;
  return { yes, no };
}

/** The answer PM gives by itself to a high-risk question: the agent's own No. */
export function blockChoice(prompt: Pick<PanePrompt, 'choices'>): PromptChoice | null {
  return prompt.choices.find((c) => isDenyChoice(c) && !isBroadChoice(c)) ?? prompt.choices.find(isDenyChoice) ?? null;
}

/** Every headline this file knows, so the test can check them against `rules.rs`. */
export const KNOWN_HEADLINES: string[] = [
  ...UNREADABLE,
  ...READ_ONLY,
  ...Object.keys(MEDIUM),
  ...Object.keys(LOW),
  ...Object.keys(BLOCKED),
];

/** Words for the language test: every `what` and `safe` phrase, in a card sentence. */
export const STRINGS: string[] = [
  ...[...Object.values(MEDIUM), ...Object.values(LOW), ...Object.values(GENERIC)].map((w) => `Claude Code wants to ${w.what}. ${w.safe}`.trim()),
  `Claude Code wants to ${READ_WORDS.what}. ${READ_WORDS.safe}`,
  ...Object.values(BLOCKED).map((what) => `Claude Code wanted to ${what}.`),
  NOTHING_CHANGES,
  CAN_GO_BACK,
  CHECK_FIRST,
  STAYS_INSIDE,
];
