// Settings > Agents: logins (accounts), keeping agents warm (preload) and agent definitions (TOML files,
// consent, review and approval). Most blocks save through their own command; the Live updates row calls env.patch.
import { AccountsGroup } from '../accounts/AccountsGroup';
import { AgentDefinitions } from '../blocks/AgentDefinitions';
import { AgentVersions } from '../blocks/AgentVersions';
import { PreloadSettings } from '../blocks/PreloadSettings';
import { Popup } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { SettingRow } from '../settings-ui';

function LiveUpdates({ env }: { env: SettingsEnv }) {
  // 'evidence' is a later step that has no control yet; it reads as watch only here.
  const value = env.settings.agent_hooks_mode === 'off' ? 'off' : 'shadow';
  return (
    <SettingRow
      label="Live updates"
      description="Claude Code and Codex tell Puppet Master when they start, ask and finish. For now Puppet Master only records this next to what it reads on screen; it never acts on it. Codex asks for your OK once per project."
    >
      <Popup
        aria-label="Live updates"
        value={value}
        onChange={(event) => void env.patch({ agent_hooks_mode: event.target.value as 'off' | 'shadow' })}
      >
        <option value="shadow">Watch only</option>
        <option value="off">Off</option>
      </Popup>
    </SettingRow>
  );
}

export const agentsRows: RowDef[] = [
  {
    id: 'accounts',
    label: 'Accounts',
    hint: 'Use the same agent with more than one login, and pick one per pane.',
    keywords: 'login profile work personal open login pane add account remove',
    group: 'Logins',
    render: () => <AccountsGroup />,
  },
  {
    id: 'preload',
    label: 'Keep agents ready',
    hint: 'Keep a warm agent ready so opening a pane feels instant.',
    keywords: 'preload warm standby idle timeout status agents to keep warm',
    group: 'Speed',
    render: (env) => <PreloadSettings onSaved={env.markSaved} />,
  },
  {
    id: 'versions',
    label: 'Agent versions',
    hint: 'Check that an agent still starts the way Puppet Master expects after it updates.',
    keywords: 'version update tested untested check canary strict unknown screens recognise capture folder fixture',
    group: 'Updates',
    render: () => <AgentVersions />,
  },
  {
    id: 'live-updates',
    label: 'Live updates',
    hint: 'Claude Code and Codex tell Puppet Master when they start, ask and finish.',
    keywords: 'hooks live updates claude codex watch report trust',
    group: 'Live updates',
    render: (env) => <LiveUpdates env={env} />,
  },
  {
    id: 'definitions',
    label: 'Agent definitions',
    hint: 'Agents defined by TOML files. A new or changed one stays inactive until you review and approve it.',
    keywords: 'toml consent project sources drafts import export fixtures revoke approve review extra directories generic agent reload',
    render: () => <AgentDefinitions />,
  },
];
