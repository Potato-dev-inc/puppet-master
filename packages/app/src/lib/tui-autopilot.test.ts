import { describe, expect, it } from 'vitest';
import { isPermissionPrompt, wantsExplicitYes } from './tui-autopilot';

describe('tui autopilot prompt detection', () => {
  it('detects proceed prompts that require explicit yes', () => {
    const prompt = 'Do you want to proceed with these file edits? (y/n)';

    expect(isPermissionPrompt(prompt)).toBe(true);
    expect(wantsExplicitYes(prompt)).toBe(true);
  });

  it('does not type y for ordinary allow-once permission menus', () => {
    const prompt = 'Permission required: Allow once / Deny';

    expect(isPermissionPrompt(prompt)).toBe(true);
    expect(wantsExplicitYes(prompt)).toBe(false);
  });
});
