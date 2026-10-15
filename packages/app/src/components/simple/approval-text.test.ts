import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import type { PanePrompt, PromptChoice, RiskLevel } from '../../lib/cockpit-api';
import { blockChoice, KNOWN_HEADLINES, simpleApproval, UNREADABLE, yesNo } from './approval-text';

const prompt = (level: RiskLevel, headline: string, choices: PromptChoice[] = []): PanePrompt => ({
  promptId: 'p1',
  kind: 'command_approval',
  text: 'npm test',
  choices,
  risk: { level, headline, reasons: ['x'] },
});

const CHOICES: PromptChoice[] = [
  { id: 'allow_once', label: 'Yes' },
  { id: 'allow_always', label: "Yes, and don't ask again", broad: true },
  { id: 'deny', label: 'No, and tell it what to do differently' },
];

describe('approvals in plain words', () => {
  it('the eight common rules each have their own sentence', () => {
    const common: [RiskLevel, string, string][] = [
      ['low', 'Look at files', 'look at your files'],
      ['low', 'Run tests', 'check that your project works'],
      ['low', 'Build or start the project', 'start your project to try it'],
      ['medium', 'Change files in this project', 'change files in your project'],
      ['low', 'Create files or folders', 'make a new file'],
      ['medium', 'Connect to another computer', 'use the internet'],
      ['medium', 'Read a file that may hold secrets', 'open a file with passwords in it'],
      ['medium', 'Look at files outside this project', 'look outside your project folder'],
    ];
    for (const [level, headline, what] of common) {
      const words = simpleApproval(prompt(level, headline));
      expect(words?.kind).toBe('ask');
      expect(words?.what).toBe(what);
      expect(words?.matched).toBe(headline);
    }
  });

  it('every headline it knows still exists in the Rust rules (a rename cannot slip through)', () => {
    const dir = join(__dirname, '../../../src-tauri/src/prompt_risk');
    const rules = ['rules.rs', 'mod.rs', 'command.rs'].map((f) => readFileSync(join(dir, f), 'utf8')).join('\n');
    const missing = KNOWN_HEADLINES.filter((h) => !rules.includes(`"${h}"`));
    expect(missing).toEqual([]);
  });

  it('a sentence is used only at the level it was written for', () => {
    // "Run tests" is a low-risk sentence: at medium it must not claim "Nothing changes".
    const odd = simpleApproval(prompt('medium', 'Run tests'));
    expect(odd?.matched).toBeNull();
    expect(odd?.safe).not.toBe('Nothing changes.');
  });

  it('high risk is blocked, never asked', () => {
    const words = simpleApproval(prompt('high', 'Delete files outside this project', CHOICES));
    expect(words?.kind).toBe('blocked');
    expect(words?.what).toBe('delete files outside your project');
    const unknown = simpleApproval(prompt('high', 'Something new'));
    expect(unknown?.kind).toBe('blocked');
    expect(unknown?.matched).toBeNull();
  });

  it('there is no "always allow": only Yes and No are offered', () => {
    const { yes, no } = yesNo(CHOICES);
    expect(yes?.id).toBe('allow_once');
    expect(no?.id).toBe('deny');
    expect(yesNo(CHOICES.filter((c) => !c.broad && c.id !== 'allow_once')).yes).toBeNull();
  });

  it('PM answers the agent own No on a blocked step, never a broad answer', () => {
    expect(blockChoice({ choices: CHOICES })?.id).toBe('deny');
    expect(blockChoice({ choices: [{ id: 'allow_once', label: 'Yes' }] })).toBeNull();
    // A broad "never ask" that happens to start with No is not used while a plain No exists.
    const tricky: PromptChoice[] = [{ id: 'never', label: "No, don't ask again", broad: true }, { id: 'deny', label: 'No' }];
    expect(blockChoice({ choices: tricky })?.id).toBe('deny');
  });

  it('a folder-trust question says what it asks, not "a step that may change things"', () => {
    const words = simpleApproval(prompt('medium', 'Trust this folder', [{ id: 'trust', label: 'Trust this folder' }, { id: 'deny', label: "Don't trust" }]));
    expect(words?.what).toBe('work in this folder');
    expect(words?.matched).toBe('Trust this folder');
    // Yes means trust, No means do not trust: the agent's own answers, never a broad one.
    const { yes, no } = yesNo([{ id: 'trust', label: 'Trust this folder' }, { id: 'deny', label: "Don't trust" }]);
    expect([yes?.id, no?.id]).toEqual(['trust', 'deny']);
  });

  it('what PM cannot word is left to the agent screen', () => {
    for (const headline of UNREADABLE) expect(simpleApproval(prompt('medium', headline, CHOICES))).toBeNull();
  });

  it('a prompt without a risk is not an approval', () => {
    expect(simpleApproval({ risk: undefined })).toBeNull();
  });
});
