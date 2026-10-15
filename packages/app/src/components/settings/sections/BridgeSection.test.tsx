// @vitest-environment jsdom
// Settings > Bridge & MCP: every row renders, the runtime switch and installs run the right calls, and uninstall is confirmed.
import { unsearchableLabels } from '../test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { buttonByText, click, hasText, mountRows, settle, type Mounted } from '../test-utils';
import { bridgeRows } from './BridgeSection';

const mocks = vi.hoisted(() => ({
  status: {
    bridgeReachable: true,
    bridgeUrl: 'http://127.0.0.1:17321',
    bridgeVersion: '0.9.0',
    portFileExists: true,
    portFilePath: 'C:\\work\\demo\\.puppet-master\\port',
    nodeAvailable: true,
    npmAvailable: true,
    npmPackageVersion: '1.2.0',
    launchCommand: 'npx -y @puppet-master/mcp',
    backends: [
      { backend: 'claude', label: 'Claude Code', installed: true, usesNpm: true, configPath: 'C:\\work\\demo\\.mcp.json', message: 'Configured.' },
    ],
    overallReady: true,
    repairResults: [],
  },
  written: [{ backend: 'claude', installed: true, changed: true, message: 'Wrote .mcp.json.' }],
}));

vi.mock('../../../lib/mcp-config', () => ({
  PUPPET_MASTER_MCP_COMMAND: { command: 'npx', args: ['-y', '@puppet-master/mcp'] },
  getMcpStatus: vi.fn(async () => mocks.status),
  installNpmMcpConfigs: vi.fn(async () => mocks.written),
  installGlobalNpmMcpConfigs: vi.fn(async () => mocks.written),
  uninstallNpmMcpConfigs: vi.fn(async () => mocks.written),
  uninstallGlobalNpmMcpConfigs: vi.fn(async () => mocks.written),
}));

vi.mock('../toolbelt/ToolbeltGroup', async () => {
  const { createElement } = await import('react');
  return {
    ToolbeltGroup: ({ projectPath }: { projectPath?: string | null }) =>
      createElement('section', { 'aria-label': 'Toolbelt', 'data-project': projectPath ?? '' }, 'Toolbelt stub'),
  };
});

import {
  getMcpStatus,
  installGlobalNpmMcpConfigs,
  installNpmMcpConfigs,
  uninstallGlobalNpmMcpConfigs,
  uninstallNpmMcpConfigs,
} from '../../../lib/mcp-config';

let m: Mounted;

async function mount(overrides: Parameters<typeof mountRows>[1] = {}) {
  m = await mountRows(bridgeRows, overrides);
  await settle(5);
  return m;
}

beforeEach(() => vi.clearAllMocks());
afterEach(async () => {
  await m?.unmount();
});

const switchFor = (label: string) => m.host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`);
const rowText = (id: string) => m.host.querySelector(`[data-row="${id}"]`)?.textContent ?? '';

describe('Bridge & MCP: every row renders', () => {
  it('renders every row id and every live control', async () => {
    await mount();
    for (const row of bridgeRows) {
      expect(m.host.querySelector(`[data-row="${row.id}"]`), `missing row: ${row.id}`).not.toBeNull();
    }
    expect(m.host.querySelector('[aria-label="Toolbelt"]')).not.toBeNull();
    for (const label of ['Bridge address', 'Runtime status', 'Use bundled Rust MCP', 'Install MCP runtime', 'Claude Desktop config', 'Codex config', 'Uninstall MCP']) {
      expect(hasText(m.host, label), `missing: ${label}`).toBe(true);
    }
    for (const text of ['Recheck & repair', 'Install in project', 'Install globally', 'Uninstall from project…', 'Uninstall globally…', 'Copy']) {
      expect(buttonByText(m.host, text), `missing button: ${text}`).toBeTruthy();
    }
    expect(switchFor('Use bundled Rust MCP')).not.toBeNull();
  });

  it('shows the bridge endpoint from the environment', async () => {
    await mount();
    expect(rowText('bridge-endpoint')).toContain('http://127.0.0.1:17321');
  });

  it('has no dropped or planned controls and no Save button', async () => {
    await mount();
    for (const gone of ['Remote runners', 'Log MCP tool calls', 'Show protocol inspector', 'Bridge health response', 'Settings file', 'Planned', 'Not implemented']) {
      expect(hasText(m.host, gone), `should not show: ${gone}`).toBe(false);
    }
    expect(buttonByText(m.host, 'Save changes')).toBeUndefined();
  });

  it('puts the destructive row in its own last list', async () => {
    await mount();
    const lists = [...m.host.querySelectorAll('section.st-group')];
    const last = lists[lists.length - 1];
    expect(last.getAttribute('data-danger')).toBe('true');
    expect(last.textContent).toContain('Uninstall MCP');
    expect(lists.slice(0, -1).some((s) => s.textContent?.includes('Uninstall MCP'))).toBe(false);
  });

  it('shows the runtime status from the report', async () => {
    await mount();
    expect(hasText(m.host, 'Ready for orchestrators')).toBe(true);
    expect(hasText(m.host, 'Installed (npm)')).toBe(true);
    expect(getMcpStatus).toHaveBeenCalledWith('C:\\work\\demo', false);
  });
});

describe('Bridge & MCP: controls save and run the right calls', () => {
  it('Use bundled Rust MCP saves only {developer_use_rust_mcp} and the install hint follows the new runtime', async () => {
    await mount();
    await click(switchFor('Use bundled Rust MCP'));
    expect(m.env.patches).toEqual([{ developer_use_rust_mcp: true }]);
    await m.rerender();
    expect(rowText('mcp-install')).toContain('bundled Rust MCP binary');
    expect(switchFor('Use bundled Rust MCP')!.getAttribute('aria-checked')).toBe('true');
  });

  it('Install in project runs the project install, shows the per-agent result, and asks the status to re-check', async () => {
    await mount();
    vi.mocked(getMcpStatus).mockClear();
    await click(buttonByText(m.host, 'Install in project'));
    await settle(5);
    expect(installNpmMcpConfigs).toHaveBeenCalledWith('C:\\work\\demo');
    expect(rowText('mcp-install')).toContain('Wrote .mcp.json.');
    expect(rowText('mcp-install')).toContain('Updated');
    expect(getMcpStatus).toHaveBeenCalledTimes(1); // the status row re-checked on the event, once
    expect(getMcpStatus).toHaveBeenCalledWith('C:\\work\\demo', false);
  });

  it('a failed install shows its error on the row', async () => {
    await mount();
    vi.mocked(installNpmMcpConfigs).mockRejectedValueOnce(new Error('npm not found'));
    await click(buttonByText(m.host, 'Install in project'));
    await settle(5);
    expect(rowText('mcp-install')).toContain('npm not found');
  });

  it('Install globally runs the global install with no project path', async () => {
    await mount();
    await click(buttonByText(m.host, 'Install globally'));
    await settle(5);
    expect(installGlobalNpmMcpConfigs).toHaveBeenCalledTimes(1);
    expect(installNpmMcpConfigs).not.toHaveBeenCalled();
  });

  it('Install in project is disabled and says why when no project folder is chosen', async () => {
    await mount({ projectPath: null });
    expect(buttonByText(m.host, 'Install in project')!.disabled).toBe(true);
    expect(hasText(m.host, 'Choose a project folder in General first')).toBe(true);
    expect(buttonByText(m.host, 'Uninstall from project…')!.disabled).toBe(true);
    expect(buttonByText(m.host, 'Install globally')!.disabled).toBe(false);
  });

  it('Recheck & repair calls getMcpStatus with autoRepair on', async () => {
    await mount();
    await click(buttonByText(m.host, 'Recheck & repair'));
    await settle(5);
    expect(getMcpStatus).toHaveBeenLastCalledWith('C:\\work\\demo', true);
  });

  it('the Claude Desktop and Codex snippets carry the MCP command', async () => {
    await mount();
    expect(rowText('snippet-claude-desktop')).toContain('"puppet-master"');
    expect(rowText('snippet-claude-desktop')).toContain('@puppet-master/mcp');
    expect(rowText('snippet-codex')).toContain('[mcp_servers.puppet-master]');
  });
});

describe('Bridge & MCP: uninstall is always confirmed', () => {
  it('a declined project uninstall calls nothing', async () => {
    await mount();
    m.env.confirm.mockResolvedValueOnce(false);
    await click(buttonByText(m.host, 'Uninstall from project…'));
    await settle(5);
    expect(m.env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Uninstall MCP from project?', confirmLabel: 'Uninstall' }));
    expect(uninstallNpmMcpConfigs).not.toHaveBeenCalled();
  });

  it('a declined global uninstall calls nothing', async () => {
    await mount();
    m.env.confirm.mockResolvedValueOnce(false);
    await click(buttonByText(m.host, 'Uninstall globally…'));
    await settle(5);
    expect(m.env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Uninstall global MCP?', confirmLabel: 'Uninstall' }));
    expect(uninstallGlobalNpmMcpConfigs).not.toHaveBeenCalled();
  });

  it('an accepted project uninstall removes the project entries and announces the change', async () => {
    await mount();
    const onChanged = vi.fn();
    window.addEventListener('pm-mcp-changed', onChanged);
    await click(buttonByText(m.host, 'Uninstall from project…'));
    await settle(5);
    window.removeEventListener('pm-mcp-changed', onChanged);
    expect(uninstallNpmMcpConfigs).toHaveBeenCalledWith('C:\\work\\demo');
    expect(uninstallGlobalNpmMcpConfigs).not.toHaveBeenCalled();
    expect(rowText('mcp-uninstall')).toContain('Wrote .mcp.json.');
    expect(onChanged).toHaveBeenCalled();
  });

  it('an accepted global uninstall removes the global entries', async () => {
    await mount();
    await click(buttonByText(m.host, 'Uninstall globally…'));
    await settle(5);
    expect(uninstallGlobalNpmMcpConfigs).toHaveBeenCalledTimes(1);
    expect(uninstallNpmMcpConfigs).not.toHaveBeenCalled();
  });

  it('keeps the explanation of which files are touched', async () => {
    await mount();
    expect(rowText('mcp-uninstall')).toContain('.codex/config.toml');
    expect(rowText('mcp-uninstall')).toContain('Cursor and Claude Desktop configs are manual');
  });
});

describe('Bridge: search covers what the section shows', () => {
  it('every word of every label a row shows finds that row', async () => {
    await mount();
    expect(unsearchableLabels(m.host, bridgeRows)).toEqual([]);
  });
});
