import { describe, expect, it } from 'vitest';
import type { Settings } from '@puppet-master/shared';
import { clampMobileInputDelayMs, toPublicSettings } from './bridge-settings';

describe('clampMobileInputDelayMs', () => {
  it('allows zero to disable mobile input buffering', () => {
    expect(clampMobileInputDelayMs(0)).toBe(0);
    expect(clampMobileInputDelayMs(-10)).toBe(0);
  });

  it('clamps nonzero values to the supported custom range', () => {
    expect(clampMobileInputDelayMs(1)).toBe(50);
    expect(clampMobileInputDelayMs(1250)).toBe(1000);
    expect(clampMobileInputDelayMs(275.4)).toBe(275);
  });

  it('uses the default for nonnumeric values', () => {
    expect(clampMobileInputDelayMs(undefined)).toBe(250);
  });
});

describe('toPublicSettings live updates', () => {
  it('defaults agent_hooks_mode to shadow and agent_hooks_agents to {}', () => {
    const pub = toPublicSettings({} as Settings);
    expect(pub.agent_hooks_mode).toBe('shadow');
    expect(pub.agent_hooks_agents).toEqual({});
  });

  it('passes saved values through', () => {
    const pub = toPublicSettings({ agent_hooks_mode: 'off', agent_hooks_agents: { codex: 'off' } } as unknown as Settings);
    expect(pub.agent_hooks_mode).toBe('off');
    expect(pub.agent_hooks_agents).toEqual({ codex: 'off' });
  });
});
