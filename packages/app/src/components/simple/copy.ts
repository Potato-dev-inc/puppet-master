// Every word Simple mode shows, in one place. A test (`language.test.ts`) reads `allSimpleStrings()`
// and fails any sentence over 15 words, any card over 3 sentences, any word that is not on the list
// in `word-list.ts`, and any of the words Simple never uses (IDEAS-R8-PLAIN-MODE §6).
//
// Rules for new strings:
//  - one idea per sentence, 15 words or fewer, 3 sentences at most per card;
//  - present tense, the agent as the subject: "Claude Code wants to ...";
//  - say if her work is safe in every stop and every problem;
//  - buttons: one verb, or a verb and an object, 3 words at most;
//  - no word from the "never" list; add a new word to `word-list.ts` only when it is simple.
//
// Functions take the pieces that change (an agent name, a count, a file name). Each has samples in
// `SAMPLES` so the test can check what it makes.

export const T = {
  // ---- window
  look: 'Look: Simple',
  changeLook: 'Change',
  projects: 'Your projects',
  makeSomething: 'Make something',
  noProjects: 'You have no projects yet. Press Make something to start.',
  needsYouCount: 'Needs you',
  back: 'Projects',

  // ---- states of a project (the glyph is never alone)
  stateWorking: 'Working',
  stateNeedsYou: 'Needs you',
  stateReady: 'Ready',
  stateStopped: 'Stopped',
  workingOnCopy: 'Working on a copy',
  copyNote: 'Your own files stay as they are until you press Keep.',
  notOnCopy: 'This project is not on a copy. Simple mode cannot show it.',

  // ---- tabs
  tabPreview: 'Preview',
  tabConversation: 'Conversation',
  tabChanged: 'What changed',
  tabGoBack: 'Go back',

  // ---- bottom bar
  keep: 'Keep',
  throwAway: 'Throw away',
  stop: 'Stop',
  keepTitle: 'Keep these changes?',
  keepWorking: 'Claude Code is still working. Wait, or press Stop.',
  keepNothing: 'Nothing changed yet.',
  keepConfirm: 'Keep',
  keepDone: 'Kept. Your folder has the new files.',
  throwTitle: 'Throw away all changes?',
  throwBody: 'Your own folder stays as it is.',
  throwConfirm: 'Throw away',
  throwDone: 'Thrown away. Your own folder was not changed.',
  cancel: 'Cancel',
  keepFailed: 'PM could not keep the changes. Your work is safe. Nothing was lost.',
  keepConflict: 'A file changed in your folder while Claude Code worked. Nothing was kept.',

  // ---- composer
  composerPlaceholder: 'Tell Claude Code what to change',
  composerSend: 'Send',
  composerQueued: 'Your message will be sent when Claude Code finishes.',
  composerSent: 'Sent.',

  // ---- details
  details: 'Details',
  agentScreen: "Agent's screen",
  agentScreenNote: "This is what Claude Code shows. You don't need it.",
  hideScreen: 'Hide the screen',
  closeProject: 'Close this project',
  more: 'More',
  cantRead: "Claude Code is showing something PM can't read.",
  showMe: 'Show me',

  // ---- preview
  previewEmpty: 'Claude Code is making your page. It shows here when it is ready.',
  previewNone: 'There is no page to show yet.',
  previewBig: 'This page is too big to show here. Open it in your browser.',
  openIt: 'Open it',
  previewOpenFailed: 'PM could not open the page. Your work is safe.',

  // ---- conversation
  you: 'You',
  summary: 'Summary',
  conversationEmpty: 'Nothing was said yet. Claude Code starts in a moment.',
  conversationUnavailable: 'The conversation is not ready. Your work is safe.',
  thinking: 'Working',

  // ---- what changed
  changedEmpty: 'Nothing changed yet.',
  changedNew: 'new',
  changedEdited: 'changed',
  changedRemoved: 'removed',
  changedIntro: 'These files are in your copy. Your own folder has not changed.',

  // ---- go back
  goBackIntro: 'PM saves your project after every change. Pick a version to go back to.',
  goBackNow: 'Now',
  goBackStart: 'The start',
  goBackHere: 'Go back to here',
  goBackConfirmBody: 'The newer changes are saved too. You can come back.',
  goBackConfirm: 'Go back',
  goBackWorking: 'Claude Code is working. Press Stop first, then go back.',
  goBackEmpty: 'No versions yet. PM saves one when Claude Code finishes a step.',
  goBackOff: 'Go back is off. Your work is safe, but PM cannot save versions.',
  goBackNeedsHelper: 'Go back needs a small helper on this computer. It is not there yet.',
  goBackUnavailable: 'PM cannot show your versions. Your work is safe.',
  goBackDone: 'You went back.',
  goBackUndo: 'Undo',
  goBackUndone: 'Done. Your files are as they were before.',

  // ---- approvals
  yes: 'Yes',
  no: 'No',
  willChange: 'What will change',
  hideChange: 'Hide this',
  blockedWhy: 'Why did PM stop this?',
  blockedWhyBody: 'This could harm your computer or your files. Simple mode does not let it happen.',
  blockedOk: 'OK',
  blockedTell: 'Tell Claude Code why',
  blockedTold: "Told Claude Code to stay in this folder.",
  blockedTellText: "Don't do that. Stay inside this folder.",
  checkFirst: 'Check first',
  blockedSafe: 'Your files are safe. Nothing was changed.',

  // ---- make something
  makeTitle: 'Make something',
  makeLead: 'Answer 3 short questions. Then you see what PM will ask.',
  makeChooseKind: 'What do you want to make?',
  makeNext: 'Next',
  makeBack: 'Back',
  makeStart: 'Start',
  makeOther: 'Something else',
  makeOtherPlaceholder: 'Write it here',
  makeSeeTitle: 'Here is what PM will ask',
  makeSeeLead: 'You can change the words.',
  makeRulesTitle: 'PM also asks it to',
  makeStarting: 'Starting',
  makeNoAgent: 'No AI is ready on this computer. Add Claude Code or Codex first.',
  makeAddAgent: 'Open settings',
  makeFolder: 'Use a folder I already have',
  makeFolderChoose: 'Choose a folder',
  makeFolderTooBroad: 'This folder is too big to use. Pick a smaller folder, or make a new one.',
  makeFolderMissing: 'PM cannot find that folder. Pick another one.',
  makeFolderNeedsSetup: 'PM cannot copy this folder yet. Make a new project instead.',
  makeFolderOk: 'PM will work on a copy of this folder.',
  makeNewFolder: 'Make a new project instead',
  makeFailed: 'PM could not start. Your files are safe. Nothing was changed.',
  makeAnswerFirst: 'Pick an answer to go on.',

  // ---- problems
  tryAgain: 'Start again',
  signInAgain: 'Sign in',
  askFix: 'Ask Claude Code to fix it',
  goBackButton: 'Go back',
  copyReport: 'Copy report',
  reportCopied: 'Copied.',
  somethingWrong: 'Something went wrong. Your work is saved.',

  // ---- settings row
  settingLabel: 'Simple mode: fewer buttons and plain words',
  settingHint: 'Shows only what you need. You can switch it off at any time.',
} as const;

// ---- strings with pieces that change

export const F = {
  agentWants: (agent: string, what: string, safe: string) => `${agent} wants to ${what}. ${safe}`,
  askingAgent: (agent: string) => `Here is what PM will ask ${agent}.`,
  tellAgent: (agent: string) => `Tell ${agent} what to change`,
  willSendWhenDone: (agent: string) => `Your message will be sent when ${agent} finishes.`,
  agentMaking: (agent: string) => `${agent} is making your page.`,
  agentScreenCaption: (agent: string) => `This is what ${agent} shows. You do not need it.`,
  cantRead: (agent: string) => `${agent} is showing something PM cannot read.`,
  stoppedAgent: (agent: string) => `PM stopped ${agent}.`,
  blockedWanted: (agent: string, what: string) => `${agent} wanted to ${what}.`,
  workingOnCopy: (agent: string) => `Working on a copy · ${agent}`,
  filesCount: (n: number) => (n === 1 ? '1 file' : `${n} files`),
  changedSummary: (agent: string, n: number) =>
    n === 0 ? `${agent} did not change any files.` : `${agent} changed ${n === 1 ? '1 file' : `${n} files`}.`,
  keepBody: (n: number) => `These ${n === 1 ? '1 file goes' : `${n} files go`} to your own folder.`,
  goBackConfirmTitle: (time: string) => `Go back to ${time}?`,
  wentBackTo: (time: string) => `You went back to ${time}.`,
  minutesAgo: (m: number) => (m < 1 ? 'just now' : m === 1 ? '1 minute ago' : `${m} minutes ago`),
  hoursAgo: (h: number) => (h === 1 ? '1 hour ago' : `${h} hours ago`),
  daysAgo: (d: number) => (d === 1 ? '1 day ago' : `${d} days ago`),
  stepRead: (file: string) => `Read ${file}`,
  stepMade: (file: string) => `Made ${file}`,
  stepChanged: (file: string) => `Changed ${file}`,
  stepRan: () => 'Ran a step',
  stepSearched: () => 'Looked through your files',
  agentStopped: (agent: string) => `${agent} stopped by itself. Your work is saved. You can start it again.`,
  agentSignedOut: (agent: string) => `${agent} is signed out. Sign in again in the browser. Your work is saved.`,
  agentLoop: (agent: string) => `${agent} tried the same fix 3 times. It is not working. Try saying it another way, or go back.`,
  agentSlow: (agent: string) => `${agent} has not done anything for a while. Your work is saved.`,
  pageProblem: (agent: string) => `The page has a problem, so it cannot show. ${agent} can look at it.`,
  stepOf: (n: number) => `Question ${n} of 3`,
  folderMade: (name: string) => `PM made the folder ${name}.`,
  madeChanges: (agent: string) => `${agent} made changes.`,
  agentMissing: (agent: string) => `${agent} is not on this computer. Add it first. Your work is safe.`,
  messageLost: (agent: string) => `Your message did not reach ${agent}. Your work is safe.`,
  trustDeclined: (agent: string) => `${agent} closed because you said No to a question. Your work is saved.`,
};

// ---- samples, so the language test can check what the functions make

export const SAMPLES: string[] = [
  F.agentWants('Claude Code', 'look at your files', 'It does not change them.'),
  F.askingAgent('Claude Code'),
  F.tellAgent('Claude Code'),
  F.willSendWhenDone('Claude Code'),
  F.agentMaking('Claude Code'),
  F.agentScreenCaption('Claude Code'),
  F.cantRead('Claude Code'),
  F.stoppedAgent('Claude Code'),
  F.blockedWanted('Claude Code', 'delete files outside your project'),
  F.workingOnCopy('Claude Code'),
  F.filesCount(1),
  F.filesCount(3),
  F.changedSummary('Claude Code', 0),
  F.changedSummary('Claude Code', 1),
  F.changedSummary('Claude Code', 3),
  F.keepBody(1),
  F.keepBody(3),
  F.goBackConfirmTitle('14:05'),
  F.wentBackTo('14:05'),
  F.minutesAgo(0),
  F.minutesAgo(1),
  F.minutesAgo(5),
  F.hoursAgo(1),
  F.hoursAgo(3),
  F.daysAgo(1),
  F.daysAgo(2),
  F.stepRead('quiz.html'),
  F.stepMade('quiz.html'),
  F.stepChanged('quiz.html'),
  F.stepRan(),
  F.stepSearched(),
  F.agentStopped('Claude Code'),
  F.agentSignedOut('Claude Code'),
  F.agentLoop('Claude Code'),
  F.agentSlow('Claude Code'),
  F.pageProblem('Claude Code'),
  F.stepOf(1),
  F.folderMade('Class quiz'),
  F.madeChanges('Claude Code'),
  F.agentMissing('Claude Code'),
  F.messageLost('Claude Code'),
  F.trustDeclined('Claude Code'),
];

/** The strings in this file. The other Simple files export their own `STRINGS`; the language test
 *  puts them all together. */
export function ownStrings(): string[] {
  return [...Object.values(T), ...SAMPLES];
}
