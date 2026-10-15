// Example attention items for the phone harness (route `phone`): what the PC's `GET /attention` would
// list for the example panes. Used by the approval-card scenarios and by tests.
import type { AttentionItem, AttentionResolution } from '../lib/attention';
import type { PaneInfo } from '../lib/tauri-types';
import { PANE_CLAUDE, PANE_CODEX, PANE_OPENCODE, PANE_PWSH } from './fixtures';
import { PHONE_PANES } from './phone-fixtures';

const MIN = 60_000;
const now = Date.now();

const ALLOW = { id: 'allow_once', label: 'Allow once', broad: false };
const ALLOW_ALL = { id: 'allow_all', label: 'Allow all commands', broad: true };
const DENY = { id: 'deny', label: 'Deny', broad: false };

function pane(id: string, heading: string, agent: string, project: string) {
  return { id, heading, agent, project };
}

/** Low risk: run the tests. The agent's own order, so Allow once leads. */
export const ITEM_LOW: AttentionItem = {
  id: 'a7Kp2xQ9',
  key: `approval:${PANE_CODEX}`,
  kind: 'approval',
  lane: 'blocking',
  rank: 2,
  state: 'open',
  since_ms: now - 4 * MIN,
  expires_at_ms: null,
  pane: pane(PANE_CODEX, 'Billing tests', 'Codex', 'billing-service'),
  title: 'Run tests',
  sentence: 'Run command: npm test (in billing-service)',
  risk: { level: 'low', word: 'Low risk', headline: 'Run tests', reasons: ['It only runs the project’s own tests.'] },
  prompt: {
    prompt_id: 'pid-low',
    occurrence: 2,
    kind: 'command',
    summary: 'Run command: npm test (in billing-service)',
    choices: [ALLOW, DENY],
    fingerprint: 'f3c1a9b200d4e7a1',
    layout: { primary: 'allow_once', focus: null, more: [] },
    provenance: 'screen',
  },
  actions: [],
  resolution: null,
};

/** High risk: Deny leads and has the focus; the broad answer is not offered on a phone. */
export const ITEM_HIGH: AttentionItem = {
  id: 'h1ghR1sk',
  key: `approval:${PANE_CLAUDE}`,
  kind: 'approval',
  lane: 'blocking',
  rank: 0,
  state: 'open',
  since_ms: now - 2 * MIN,
  expires_at_ms: null,
  pane: pane(PANE_CLAUDE, 'Fix login redirect', 'Claude Code', 'puppet-master'),
  title: 'Delete a folder outside this project',
  sentence: 'Run command: rm -rf ../build-cache',
  risk: {
    level: 'high',
    word: 'High risk',
    headline: 'Delete a folder outside this project',
    reasons: ['It deletes ../build-cache and everything in it.', 'That folder is outside this project.', 'Deleted files cannot be brought back.'],
  },
  prompt: {
    prompt_id: 'pid-high',
    occurrence: 7,
    kind: 'command',
    summary: 'Run command: rm -rf ../build-cache',
    choices: [ALLOW, ALLOW_ALL, DENY],
    fingerprint: 'c0de5afe11aa22bb',
    layout: { primary: 'deny', focus: 'deny', more: ['allow_all'] },
    provenance: 'screen',
  },
  actions: [],
  resolution: null,
};

/** Check first. */
export const ITEM_MEDIUM: AttentionItem = {
  ...ITEM_LOW,
  id: 'm3d1um0x',
  key: `approval:${PANE_OPENCODE}`,
  rank: 1,
  pane: pane(PANE_OPENCODE, 'Docs site', 'OpenCode', 'docs-site'),
  title: 'Install packages',
  sentence: 'Run command: npm install left-pad',
  risk: { level: 'medium', word: 'Check first', headline: 'Install packages', reasons: ['It downloads and installs code from the internet.'] },
  prompt: {
    ...ITEM_LOW.prompt!,
    prompt_id: 'pid-medium',
    summary: 'Run command: npm install left-pad',
    fingerprint: 'beadbeadbeadbead',
  },
};

/** The folder-trust question: Don't trust leads, like Deny on a high-risk prompt. */
export const ITEM_TRUST: AttentionItem = {
  id: 'T7rustAb',
  key: `approval:${PANE_CLAUDE}`,
  kind: 'trust_gate',
  lane: 'blocking',
  rank: 0,
  state: 'open',
  since_ms: now - 40_000,
  expires_at_ms: null,
  pane: pane(PANE_CLAUDE, 'Fix login redirect', 'Claude Code', 'puppet-master'),
  title: 'Trust this folder?',
  sentence: 'Claude Code wants to work in puppet-master. It can read and change files there.',
  risk: null,
  prompt: {
    prompt_id: 'pid-trust',
    occurrence: 1,
    kind: 'workspace_trust',
    summary: 'Trust this workspace?',
    choices: [
      { id: 'trust_workspace', label: 'Trust this folder', broad: false },
      { id: 'deny', label: 'Don’t trust', broad: false },
    ],
    fingerprint: '7a57e7a57e7a57e7',
    layout: { primary: 'deny', focus: 'deny', more: [] },
    provenance: 'screen',
  },
  actions: [],
  resolution: null,
};

/** A sign-in screen: nothing to choose, only the PC can pass it. */
export const ITEM_SIGNIN: AttentionItem = {
  ...ITEM_TRUST,
  id: 'S1gn1nXx',
  title: 'Claude Code needs you on your PC',
  sentence: 'It’s showing a sign-in screen. Puppet Master won’t answer it for you.',
  prompt: { ...ITEM_TRUST.prompt!, prompt_id: 'pid-signin', kind: 'startup_screen', choices: [], fingerprint: '51611151611151aa', layout: { primary: null, focus: null, more: [] } },
};

/** A prompt Puppet Master could not read. */
export const ITEM_NO_PROMPT: AttentionItem = {
  ...ITEM_LOW,
  id: 'n0Pr0mpt',
  risk: null,
  title: 'Codex is waiting for you',
  sentence: null,
  prompt: null,
};

/** An agent's question (the choices come from `GET /questions`). */
export const ITEM_QUESTION: AttentionItem = {
  id: 'Qk2x9aLm',
  key: 'question:q-1',
  kind: 'question',
  lane: 'blocking',
  rank: 2,
  state: 'open',
  since_ms: now - 6 * MIN,
  expires_at_ms: now + 54 * MIN,
  pane: pane(PANE_CLAUDE, 'Fix login redirect', 'Claude Code', 'puppet-master'),
  title: 'Keep the old /login route as a redirect, or remove it?',
  sentence: null,
  risk: null,
  prompt: null,
  actions: [],
  resolution: null,
};

export const ITEM_ERROR: AttentionItem = {
  id: 'Err0rPwS',
  key: `error:${PANE_PWSH}`,
  kind: 'error',
  lane: 'blocking',
  rank: 4,
  state: 'open',
  since_ms: now - 3 * MIN,
  expires_at_ms: null,
  pane: pane(PANE_PWSH, 'PowerShell', 'PowerShell', 'puppet-master'),
  title: 'PowerShell hit an error',
  sentence: null,
  risk: null,
  prompt: null,
  actions: [],
  resolution: null,
};

/** What the list keeps for 15 s after an item leaves. */
export function tombstone(item: AttentionItem, resolution: Partial<AttentionResolution> & { state: string }): AttentionItem {
  return {
    ...item,
    state: 'resolved',
    actions: [],
    resolution: { by: null, choice_label: null, reason: null, at_ms: Date.now() - 4_000, ...resolution },
  };
}

/** The example panes with these panes waiting: Claude and Codex and OpenCode ask, PowerShell errored. */
export const QUEUE_PANES: PaneInfo[] = PHONE_PANES.map((p) =>
  p.id === PANE_CODEX || p.id === PANE_OPENCODE ? ({ ...p, status: 'waiting_input' } as PaneInfo) : p,
);
