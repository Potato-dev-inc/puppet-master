import { describe, expect, it } from 'vitest';
import {
  AgentTypeSchema,
  getDefaultTerminalAgentType,
  getPreset,
  listLaunchPresets,
  listPresets,
} from './agents';

describe('agent presets', () => {
  it('includes a first-class command prompt preset', () => {
    expect(AgentTypeSchema.options).toContain('cmd');
    expect(listPresets('windows').map((preset) => preset.type)).toContain('cmd');
  });

  it('uses cmd.exe for the Windows command prompt preset', () => {
    const preset = getPreset('cmd', 'windows');
    expect(preset.label).toBe('Command Prompt');
    expect(preset.command).toBe('cmd.exe');
    expect(preset.baseArgs).toEqual(['/K']);
  });

  it('uses bare Windows agent commands so PowerShell resolves shims', () => {
    expect(getPreset('claude', 'windows').command).toBe('claude');
    expect(getPreset('codex', 'windows').command).toBe('codex');
    expect(getPreset('opencode', 'windows').command).toBe('opencode');
  });

  it('puts agent launchers first and the platform terminal last', () => {
    expect(listLaunchPresets('windows').map((preset) => preset.type)).toEqual([
      'claude',
      'codex',
      'opencode',
      'powershell',
    ]);
    expect(listLaunchPresets('windows').at(-1)?.label).toBe('Terminal (PowerShell)');
    expect(listLaunchPresets('linux').map((preset) => preset.type)).toEqual([
      'claude',
      'codex',
      'opencode',
      'bash',
    ]);
    expect(listLaunchPresets('linux').at(-1)?.label).toBe('Terminal (Shell)');
  });

  it('uses powershell as the Windows terminal and bash elsewhere', () => {
    expect(getDefaultTerminalAgentType('windows')).toBe('powershell');
    expect(getDefaultTerminalAgentType('macos')).toBe('bash');
    expect(getDefaultTerminalAgentType('linux')).toBe('bash');
  });
});
