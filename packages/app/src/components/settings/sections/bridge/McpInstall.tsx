// Install MCP runtime: register the selected runtime in each agent's config, for this project or globally.
import { installGlobalNpmMcpConfigs, installNpmMcpConfigs } from '../../../../lib/mcp-config';
import type { SettingsEnv } from '../../model';
import { Btn, Note, SettingBlock } from '../../settings-ui';
import { RESTART_NOTE, ResultLines, installPathOf, runtimeName, useMcpAction } from './shared';

export function McpInstallBlock({ env }: { env: SettingsEnv }) {
  const installPath = installPathOf(env);
  const { busy, results, error, run } = useMcpAction();

  return (
    <SettingBlock
      label="Install MCP runtime"
      description={`Register Claude Code, Codex, and OpenCode to use the selected runtime: ${runtimeName(env)}.`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Btn primary disabled={!installPath || busy !== null} onClick={() => void run('project', () => installNpmMcpConfigs(installPath))}>
          {busy === 'project' ? 'Installing…' : 'Install in project'}
        </Btn>
        <Btn disabled={busy !== null} onClick={() => void run('global', () => installGlobalNpmMcpConfigs())}>
          {busy === 'global' ? 'Installing…' : 'Install globally'}
        </Btn>
      </div>
      <Note>Project: {installPath || 'Choose a project folder in General first'}</Note>
      {results && <ResultLines results={results} note={RESTART_NOTE} />}
      {error && <Note role="alert">{error}</Note>}
    </SettingBlock>
  );
}
