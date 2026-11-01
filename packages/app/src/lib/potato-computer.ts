// The words of the Computer tab and the full-window computer (P3). Plain English, short sentences.
import { STATE_WORD, formatSize, type SandboxState, type SandboxView } from './potato-sandbox';

export const COMPUTER_COPY = {
  screenOf: (name: string) => `${name}’s screen`,
  takeControl: 'Take control',
  /** The person hands the computer back. */
  imDone: "I'm done",
  skip: 'Skip',
  /** The hover pill on the preview, and its label. */
  open: 'Open',
  openLabel: 'Open in full window',
  inFullWindow: 'Open in full window',
  retry: 'Retry',
  asleep: 'Asleep',
  wake: 'Wake',
  waking: 'Waking up…',
  couldNotWake: 'Could not start',
  wakeFirst: 'Wake the computer first.',
  // the full window
  overlayTitle: (name: string) => `${name}’s computer`,
  close: 'Close',
  confirmTitle: (name: string) => `Hand the computer back to ${name}?`,
  confirmBody: (name: string) => `${name} goes on from where you stopped.`,
  keepControl: 'Keep control',
  handBack: 'Hand back',
  // the maintenance section
  sectionTitle: 'The computer',
  start: 'Start',
  stop: 'Stop',
  restart: 'Restart',
  settings: 'Open sandbox settings',
  setUpTitle: 'Set up the computer',
  setUpBody: (name: string) => `${name} can use a small private computer, with its own browser and files. It cannot see your files or accounts.`,
  setUpButton: 'Set up the computer',
  settingUp: (percent: number) => `Setting up the computer… ${Math.round(percent)}%`,
  showSetUp: 'Show',
  unfinishedTitle: "Setup didn't finish",
  unfinishedBody: 'Setting up the computer was cut off, for example because the app was closed. You can pick it up where it stopped, or start over.',
  unfinishedBodyNoResume: 'Setting up the computer was cut off, and it cannot be picked up from here. Start over to set it up again.',
  continueSetUp: 'Continue setup',
  continuing: 'Continuing…',
  startOver: 'Start over',
  startOverTitle: 'Start over?',
  startOverBody: 'The unfinished computer is deleted and set up begins again from the start.',
  labsOff: (name: string) => `${name} has no computer here. Turn on Sandbox in Settings > General > Labs.`,
};

/** `Running`, `Running. Uses 3.7 GB`. */
export function sandboxLine(view: SandboxView): string {
  const parts = [STATE_WORD[view.state]];
  if (view.diskBytes !== null && (view.state === 'running' || view.state === 'stopped')) parts.push(`Uses ${formatSize(view.diskBytes)}`);
  return parts.join('. ');
}

/** Whether the screen can be looked at: the sandbox runs, or nobody can say (no sandbox reading). */
export function screenReady(state: SandboxState | null): boolean {
  return state === null || state === 'running';
}
