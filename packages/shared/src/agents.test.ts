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

  it('keeps Windows PowerShell open after spawn', () => {
    const preset = getPreset('powershell', 'windows');
    expect(preset.command).toBe('powershell.exe');
    expect(preset.baseArgs).toEqual(['-NoLogo', '-NoExit']);
  });

  it('uses bare Windows agent commands so PowerShell resolves shims', () => {
    expect(getPreset('claude', 'windows').command).toBe('claude');
    expect(getPreset('codex', 'windows').command).toBe('codex');
    expect(getPreset('opencode', 'windows').command).toBe('opencode');
  });

  it('offers OpenCode API and PowerShell in the spawn list', () => {
    expect(listLaunchPresets('windows').map((preset) => preset.type)).toEqual([
      'opencode_native',
      'powershell',
    ]);
    expect(listLaunchPresets('linux').map((preset) => preset.type)).toEqual([
      'opencode_native',
      'powershell',
    ]);
    expect(listLaunchPresets('windows').map((preset) => preset.label)).toEqual([
      'OpenCode (API)',
      'PowerShell',
    ]);
  });

  it('defaults new workers to opencode_native', () => {
    expect(getDefaultTerminalAgentType('windows')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('macos')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('linux')).toBe('opencode_native');
  });

  it('keeps Cursor IDE and the headless Cursor Agent CLI distinct', () => {
    expect(getPreset('cursor', 'windows').command).toBe('cursor.cmd');
    expect(getPreset('cursor_agent', 'windows').command).toBe('cursor-agent');
    expect(getPreset('cursor_agent', 'windows').isTui).toBe(true);
  });
});
