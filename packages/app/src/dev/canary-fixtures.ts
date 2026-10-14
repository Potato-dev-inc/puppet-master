// Version-check states for the evidence harness (`scenario.canary`). The sentences are the ones the
// Rust side writes (src-tauri/src/canary/state.rs).
import type { CanaryAgent, CanaryNotice, CanaryStatus } from '../lib/canary';

const MINUTES = 60_000;
const ago = (minutes: number) => Date.now() - minutes * MINUTES;

export const CLAUDE_TESTED: CanaryAgent = {
  agent: 'claude',
  label: 'Claude Code',
  installed: true,
  version: '2.1.299',
  tested: '>=2.1.299, <2.2',
  standing: 'tested',
  strict: false,
  strict_reason: null,
  chip: null,
  plain: 'Claude Code 2.1.299 is a version Puppet Master has been tested with.',
  checking: false,
  last: {
    outcome: 'ready',
    passed: true,
    summary: 'Claude Code started and reached its input box. Nothing was sent to it.',
    at_ms: ago(12),
    version: '2.1.299',
    screens: [],
  },
  captures: 0,
};

export const CODEX_UNTESTED: CanaryAgent = {
  agent: 'codex',
  label: 'Codex',
  installed: true,
  version: '0.171.0',
  tested: '>=0.159, <0.170',
  standing: 'untested',
  strict: true,
  strict_reason: 'untested_version',
  chip: 'Untested version',
  plain:
    'Puppet Master has not been tested with Codex 0.171.0 yet. It will ask you before closing a start-up notice, and it will not keep one ready in advance.',
  checking: false,
  last: {
    outcome: 'ready',
    passed: true,
    summary: 'Codex started and reached its input box. Nothing was sent to it.',
    at_ms: ago(3),
    version: '0.171.0',
    screens: [],
  },
  captures: 2,
};

export const CODEX_CHANGED: CanaryAgent = {
  ...CODEX_UNTESTED,
  version: '0.162.1',
  tested: '>=0.159, <0.170',
  standing: 'tested',
  strict_reason: 'changed_screens',
  plain: 'Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.',
  last: {
    outcome: 'unrecognised',
    passed: false,
    summary: 'Codex showed a screen Puppet Master does not recognise.',
    at_ms: ago(1),
    version: '0.162.1',
    screens: [],
  },
};

export const CODEX_CHANGED_NOTICE: CanaryNotice = {
  id: 'canary:codex:0.162.1',
  agent: 'codex',
  text: 'Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.',
  at_ms: ago(1),
  dismissed: false,
};

/** Settings > Agents > Agent versions: one agent flagged, one fine, two screens saved. */
export const VERSIONS_STATUS: CanaryStatus = {
  agents: [CLAUDE_TESTED, CODEX_UNTESTED],
  auto_check: true,
  notices: [],
  captures: 2,
};

/** The header chip on a Claude pane. */
export const CLAUDE_UNTESTED_STATUS: CanaryStatus = {
  agents: [
    {
      ...CLAUDE_TESTED,
      version: '2.4.0',
      standing: 'untested',
      strict: true,
      strict_reason: 'untested_version',
      chip: 'Untested version',
      plain:
        'Puppet Master has not been tested with Claude Code 2.4.0 yet. It will ask you before closing a start-up notice, and it will not keep one ready in advance.',
    },
  ],
  auto_check: true,
  notices: [],
  captures: 0,
};

/** The Inbox card after a check found changed screens. */
export const CHANGED_SCREENS_STATUS: CanaryStatus = {
  agents: [CLAUDE_TESTED, CODEX_CHANGED],
  auto_check: true,
  notices: [CODEX_CHANGED_NOTICE],
  captures: 0,
};
