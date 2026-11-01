// Fish's computer (the sandbox): from `potato_sandbox_status`. Checked only while its Labs switch is on.
import type { SandboxState } from '../potato-sandbox';
import type { CheckRow } from './types';

export function checkComputer(state: SandboxState): CheckRow[] {
  const id = 'sandbox';
  switch (state) {
    case 'running':
    case 'stopped':
      return [{ id, status: 'ok', sentence: "Fish's computer is set up", fix: null }];
    case 'starting':
    case 'installing':
    case 'incomplete':
      return [{ id, status: 'warn', sentence: "Fish's computer is still being set up", fix: null }];
    case 'failed':
      return [{ id, status: 'fail', sentence: "Fish's computer had a problem", fix: { kind: 'openSandbox', label: 'Open' } }];
    default:
      return [{ id, status: 'warn', sentence: "Fish's computer is not set up", fix: { kind: 'openSandbox', label: 'Set up' } }];
  }
}
