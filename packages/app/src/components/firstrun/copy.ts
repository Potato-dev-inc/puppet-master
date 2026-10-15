// Every word the first-run welcome shows. Plain English, the UX-PHASE1 section 0.3 word list: "separate
// copy", "Sign in", "Set up agents", "Undo", "History", "Receipt"; never worktree, account, quota, hooks,
// doctor, checkpoint. Sentences stay under 16 words and a card under 4 sentences (copy.test.ts checks).
import type { Look, StepId } from './model';

/**
 * The suggested first task, word for word what Rust sends (`first_run::TASK_PROMPT`; copy.test.ts reads
 * the Rust file and checks they match). It asks for words, never for a change.
 */
export const TASK_PROMPT =
  'Read this project and tell me, in a few plain sentences, what it does and how it is laid out. Change nothing: do not create, edit, move or delete any file.';

export const T = {
  // Shell
  title: 'Welcome to Puppet Master',
  stepOf: (n: number, total: number) => `Step ${n} of ${total}`,
  skipSetup: 'Skip setup',
  skipStep: 'Skip for now',
  later: 'Later',
  back: 'Back',
  next: 'Continue',
  reopenNote: 'You can open this again from Ctrl+K, or from Settings.',
  pausedBar: 'Setup is paused. Finish in the window, then come back.',
  pausedButton: 'Back to setup',

  // 1. Welcome
  welcomeLead: 'Run your AI agents in one place. See what they do. Undo what you do not want.',
  howDoYouWork: 'How do you work?',
  looks: {
    code: { label: 'I write code', hint: 'The full cockpit: panes, changes and a terminal.' },
    ai: { label: 'I build with AI, not much code', hint: 'Fewer buttons and plain words. Every task runs in a separate copy.' },
    none: { label: "I don't code", hint: 'Fewer buttons and plain words. Your own files stay safe.' },
  } satisfies Record<Look, { label: string; hint: string }>,
  noteCode: 'You get the full cockpit. Simple mode is in Settings, under General.',
  noteSimple: 'Simple mode is on. Switch it off any time in Settings, under General.',

  // 2. Agents
  agentsTitle: 'Your agents',
  agentsLead: 'Agents are the AI helpers that do the work. You need one that is ready.',
  agentsHowTo: 'Install one, sign in, then press Test. Test asks it one tiny question.',
  agentsReady: (n: number) => (n === 1 ? '1 agent is ready. You can go on.' : `${n} agents are ready. You can go on.`),
  agentsNone: 'No agent is ready yet. Get one to Ready, or skip for now.',

  // 3. Project
  projectTitle: 'Your first project',
  projectLead: 'Pick the folder of one project. An agent will work on a copy of it.',
  recent: 'Recent',
  noRecent: 'No projects yet. Choose a folder to start.',
  chooseFolder: 'Choose folder…',
  checking: 'Checking the folder…',
  tooBroad: 'That folder holds too much. Pick the folder of one project instead.',
  tooBig: 'This folder is too big to copy. Pick a smaller one.',
  missing: 'Puppet Master cannot find that folder. Pick another one.',
  failedFolder: 'That folder did not work. Pick another one.',
  needsGitTitle: 'This folder does not use Git yet.',
  needsGitBody: 'A separate copy and Undo both need it. Nothing is uploaded.',
  setUpGit: 'Set up Git here',
  gitWorking: 'Setting up Git…',
  gitFiles: (n: number) => `Saves ${n} ${n === 1 ? 'file' : 'files'} as the first snapshot.`,
  gitTooBig: 'This folder has too many files to save as one snapshot. Pick the project folder.',
  projectChosen: 'Ready for a separate copy.',
  remove: 'Remove',

  // 4. First task
  taskTitle: 'A first task',
  taskLead: 'Your agent reads the project and tells you what it does. It changes nothing.',
  taskAsk: 'Puppet Master will ask',
  taskAgent: 'Agent',
  taskSafe: 'It runs in a separate copy. Your folder is not touched.',
  taskReadOnly: 'It is read-only: the agent cannot change files.',
  taskSafeNot: 'This agent cannot be locked to read-only. It still works in a separate copy, so your folder stays as it is.',
  taskRun: 'Run the task',
  taskStarting: 'Starting…',
  taskWorking: (agent: string) => `${agent} is reading your project`,
  taskAnswerTitle: 'What it said',
  taskNoAnswer: 'It finished but wrote no answer.',
  taskFailed: 'The task did not finish. Your files were not touched.',
  taskTryAgain: 'Try again',
  taskNeedsAgent: 'A task needs an agent that is ready. Go back to Your agents.',
  taskNeedsProject: 'A task needs a project. Go back to Your first project.',
  taskAsking: (agent: string) => `${agent} is asking first. You decide.`,
  receiptTitle: 'Receipt',
  receiptTip: 'This is a receipt. It says what the agent did, and what it did not do.',
  undoTip: 'This is Undo. After a task that changes files, History has Undo this turn.',
  undoNone: 'This task changed nothing, so there is nothing to undo.',
  approvalTip: 'This is an approval card. Nothing risky runs until you choose.',
  openPane: 'Open it',

  // 5. Phone
  phoneTitle: 'Use your phone',
  phoneLead: 'Watch your agents and answer them from your phone.',
  phoneNote: 'Scan the code with your phone camera. You choose what the phone may do.',

  // 6. Pack
  packTitle: 'Add Puppet Master to your agents',
  packLead: 'Teach your agents to hand work to each other. You review every file first.',
  packFound: (names: string[]) => (names.length ? `Found: ${names.join(', ')}.` : 'No agent found yet.'),
  packReview: 'Review and install',
  looking: 'Looking for your agents…',

  // 7. Done
  doneTitle: 'You are set',
  doneLead: 'Three things worth knowing.',
  tipKeys: 'Ctrl+K',
  tipKeysBody: 'Find any agent or action from the keyboard.',
  tipInbox: 'Inbox',
  tipInboxBody: 'Agents that need you wait here. Answer, and they carry on.',
  tipHistory: 'History',
  tipHistoryBody: 'Every turn is saved. Undo this turn puts your files back.',
  doneButton: 'Start using Puppet Master',
  stepNames: {
    welcome: 'Welcome',
    agents: 'Your agents',
    project: 'Your first project',
    task: 'A first task',
    phone: 'Use your phone',
    pack: 'Add Puppet Master to your agents',
    done: 'Done',
  } satisfies Record<StepId, string>,

  // Palette and Settings
  paletteTitle: 'Welcome tour',
  paletteDesc: 'Set up agents, pick a project and try a first task',
  settingLabel: 'Welcome',
  settingHint: 'Go through the first-run welcome again. It never changes your settings unless you choose.',
  settingButton: 'Show the welcome',
} as const;

/** Every fixed sentence, for the language test. The three look labels are the founder's wording. */
export function ownStrings(): string[] {
  const out: string[] = [];
  const walk = (value: unknown, key = ''): void => {
    if (typeof value === 'string') {
      if (key !== 'label') out.push(value);
    } else if (value && typeof value === 'object') {
      for (const [k, v] of Object.entries(value)) walk(v, k);
    }
  };
  walk(T);
  out.push(T.stepOf(2, 7), T.agentsReady(1), T.agentsReady(3), T.gitFiles(1), T.gitFiles(12), T.taskWorking('Claude Code'), T.taskAsking('Codex'), T.packFound(['Claude Code', 'Codex']), T.packFound([]));
  return out;
}
