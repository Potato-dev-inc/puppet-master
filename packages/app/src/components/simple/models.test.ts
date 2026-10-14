// The pure parts of the Simple screens: when the agent's screen shows by itself, the conversation,
// the Go back rows, the templates and the agent choice.
import { describe, expect, it } from 'vitest';
import type { AgentRegistryListing } from '../../lib/agentRegistry';
import type { PanePrompt } from '../../lib/cockpit-api';
import { pickAgent } from './agent-pick';
import { conversationItems, stepText, summarySentence } from './conversation-model';
import { agoText, versionsFrom } from './go-back-model';
import { needsScreen } from './screen-model';
import { fullPrompt, HOUSE_RULES, projectName, TEMPLATES, visiblePrompt } from './templates';

const risky = (choices = 2): PanePrompt => ({
  promptId: 'p',
  kind: 'command_approval',
  text: 'x',
  choices: Array.from({ length: choices }, (_, i) => ({ id: `c${i}`, label: `c${i}` })),
  risk: { level: 'low', headline: 'Run tests', reasons: [] },
});

describe('the agent screen shows by itself when PM cannot read it', () => {
  it('stays hidden while the agent works or rests', () => {
    expect(needsScreen({ status: 'running', prompt: null })).toBe(false);
    expect(needsScreen({ status: 'idle', prompt: null })).toBe(false);
  });
  it('stays hidden for an approval PM understands', () => {
    expect(needsScreen({ status: 'waiting_input', prompt: risky() })).toBe(false);
  });
  it('shows for a wait with no prompt, a prompt with no risk, or no answers', () => {
    expect(needsScreen({ status: 'waiting_input', prompt: null })).toBe(true);
    expect(needsScreen({ status: 'waiting_input', prompt: undefined })).toBe(true);
    expect(needsScreen({ status: 'waiting_input', prompt: { ...risky(), risk: undefined } })).toBe(true);
    expect(needsScreen({ status: 'waiting_input', prompt: risky(0) })).toBe(true);
  });
  it('shows when the agent stopped with a problem, because the screen holds the reason', () => {
    expect(needsScreen({ status: 'error', prompt: null })).toBe(true);
  });
});

describe('conversation', () => {
  it('draws her words, the agent words and one plain line per step', () => {
    const items = conversationItems({
      chunks: [
        { stream: 'user_task', text: 'Make a quiz.' },
        { stream: 'delivery', text: 'Sent. The agent started.' },
        { stream: 'assistant_final', text: '● Write(C:\\work\\copy\\tool.html)\n  ⎿  Wrote 120 lines\nDone. It is ready.' },
        { stream: 'assistant_final', text: '● Bash(npm test)' },
        { stream: 'diagnostic', text: 'secret internals' },
        { stream: 'execution_failure', text: 'The agent exited before it answered.' },
      ],
    });
    expect(items).toEqual([
      { kind: 'you', text: 'Make a quiz.' },
      { kind: 'step', text: 'Made tool.html' },
      { kind: 'agent', text: 'Done. It is ready.' },
      { kind: 'step', text: 'Ran a step' },
      { kind: 'problem', text: 'Something went wrong. Your work is saved.' },
    ]);
  });
  it('shows her words without the house rules PM added', () => {
    const sent = fullPrompt('Make a quiz.');
    expect(conversationItems({ chunks: [{ stream: 'user_task', text: sent }, { stream: 'user_steer', text: 'Bigger buttons.' }] })).toEqual([
      { kind: 'you', text: 'Make a quiz.' },
      { kind: 'you', text: 'Bigger buttons.' },
    ]);
  });
  it('never shows a stream tag or a raw error text', () => {
    const text = JSON.stringify(conversationItems({ chunks: [{ stream: 'execution_failure', text: 'ENOENT exit code 1' }] }));
    expect(text).not.toMatch(/ENOENT|exit code|execution_failure/);
  });
  it('names tool steps by what they did', () => {
    expect(stepText('● Read(src/a.ts)')).toBe('Read a.ts');
    expect(stepText('● Edit(style.css)')).toBe('Changed style.css');
    expect(stepText('● Grep(foo)')).toBe('Looked through your files');
    expect(stepText('just words')).toBeNull();
  });
  it('the summary comes from the numbers, not the Full-mode sentence', () => {
    expect(summarySentence({ files: { count: 3, added: 48, deleted: 7, paths: [] } }, 'Claude Code')).toBe('Claude Code changed 3 files.');
    expect(summarySentence({ files: null }, 'Claude Code')).toBeNull();
  });
});

describe('go back', () => {
  const items = [
    { id: '6', turn: 2, kind: 'after', atMs: 6000 },
    { id: '5', turn: 2, kind: 'before', atMs: 5000 },
    { id: '4', turn: 1, kind: 'after', atMs: 4000 },
    { id: '3', turn: 1, kind: 'answer', atMs: 3500 },
    { id: '2', turn: 1, kind: 'undo', atMs: 3000 },
    { id: '1', turn: 1, kind: 'before', atMs: 1000 },
  ];
  const sums = [
    { turn: 1, files: { count: 2, added: 1, deleted: 0, paths: [] } },
    { turn: 2, files: { count: 1, added: 1, deleted: 0, paths: [] } },
  ];
  it('lists the version after each step, newest first, then the start', () => {
    const rows = versionsFrom(items, sums, 'Claude Code', false);
    expect(rows.map((r) => r.id)).toEqual(['6', '4', '1']);
    expect(rows[0].current).toBe(true);
    expect(rows[1].title).toBe('Claude Code changed 2 files.');
    expect(rows[2].title).toBe('The start');
  });
  it('has no current version while the agent works, and hides safety copies and answers', () => {
    const rows = versionsFrom(items, sums, 'Claude Code', true);
    expect(rows.some((r) => r.current)).toBe(false);
    expect(rows.map((r) => r.id)).not.toContain('2');
    expect(rows.map((r) => r.id)).not.toContain('3');
  });
  it('writes time in plain words', () => {
    expect(agoText(10_000)).toBe('just now');
    expect(agoText(5 * 60_000)).toBe('5 minutes ago');
    expect(agoText(3 * 3_600_000)).toBe('3 hours ago');
    expect(agoText(2 * 86_400_000)).toBe('2 days ago');
  });
});

describe('make something', () => {
  const tool = TEMPLATES[0];
  it('asks 3 tap questions per template', () => {
    for (const t of TEMPLATES) {
      expect(t.questions).toHaveLength(3);
      for (const q of t.questions) expect(q.choices.length).toBeGreaterThanOrEqual(3);
    }
  });
  it('shows her answers in a sentence, and sends them with the house rules', () => {
    const answers = { what: 'A class timer', who: 'My students or customers', look: 'Bright and fun' };
    const visible = visiblePrompt(tool, answers);
    expect(visible).toBe('Make a class timer for my students or customers. It should look bright and fun. Make it as ONE file called tool.html.');
    const sent = fullPrompt(visible);
    for (const rule of HOUSE_RULES) expect(sent).toContain(rule);
    expect(sent).toMatch(/Do not delete or move any files/);
  });
  it('names the project from the first answer', () => {
    expect(projectName(tool, { what: 'A quiz with 10 questions about plants' })).toBe('Quiz with 10 questions');
    expect(projectName(tool, {})).toBe('My tool');
  });
  it('needs no build step: one file, no other programs', () => {
    for (const t of TEMPLATES) {
      expect(t.prompt).toMatch(/ONE file/);
    }
    expect(HOUSE_RULES.join(' ')).toMatch(/without the internet/);
  });
});

describe('which agent starts', () => {
  const agent = (id: string, patch: Record<string, unknown> = {}) => ({
    id,
    label: id,
    kind: 'cli',
    dispatchable: true,
    installed: true,
    modes: { headless: true, visible: true },
    ...patch,
  });
  it('takes the first agent that can run a visible task, by its capabilities, not its name', () => {
    const listing = {
      agents: [agent('shell', { kind: 'shell' }), agent('a', { installed: false }), agent('b', { dispatchable: false }), agent('c'), agent('d')],
      dispatchable_ids: [],
      source: 'registry',
    } as unknown as AgentRegistryListing;
    expect(pickAgent(listing)?.id).toBe('c');
    expect(pickAgent({ ...listing, agents: [] })).toBeNull();
    expect(pickAgent(null)).toBeNull();
  });
});
