// Settings > Agents: logins (accounts), keeping agents warm (preload) and agent definitions (TOML files,
// consent, review and approval). Most blocks save through their own command; the Live updates row calls env.patch.
import { AccountsGroup } from '../accounts/AccountsGroup';
import { AgentDefinitions } from '../blocks/AgentDefinitions';
import { AgentVersions } from '../blocks/AgentVersions';
import { COLLISION_LABEL, CollisionSettings } from '../blocks/CollisionSettings';
import { PreloadSettings } from '../blocks/PreloadSettings';
import { useEffect, useState } from 'react';
import { agentHealthApi, type AgentHealth } from '../../../lib/agent-health';
import { Popup } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { SettingRow, SettingToggle } from '../settings-ui';
import { KeepGoingRows } from './KeepGoingRows';

type HookMode = 'off' | 'shadow' | 'evidence';

/** Agents whose live updates are off until the user turns them on (the backend says which). */
function useOptInAgents(): AgentHealth[] {
  const [rows, setRows] = useState<AgentHealth[]>([]);
  useEffect(() => {
    let alive = true;
    agentHealthApi
      .check()
      .then((all) => alive && setRows(all.filter((r) => r.installed && r.hooks?.opt_in)))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  return rows;
}

function LiveUpdates({ env }: { env: SettingsEnv }) {
  // 'evidence' is a later step that has no control yet; it reads as watch only here.
  const value = env.settings.agent_hooks_mode === 'off' ? 'off' : 'shadow';
  const optIn = useOptInAgents();
  const chosen = env.settings.agent_hooks_agents ?? {};
  const isOn = (r: AgentHealth) => (chosen[r.id] ? chosen[r.id] !== 'off' : Boolean(r.hooks?.enabled));
  const setAgent = (id: string, on: boolean) => {
    const next: Record<string, HookMode> = { ...chosen, [id]: on ? 'shadow' : 'off' };
    void env.patch({ agent_hooks_agents: next });
  };
  return (
    <>
      <SettingRow
        label="Live updates"
        description="Claude Code and Codex tell Puppet Master when they start, ask and finish. For now it is only recorded; Puppet Master never acts on it."
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
      {optIn.map((r) => (
        <SettingToggle
          key={r.id}
          label={`Live updates from ${r.label}`}
          description={r.hooks?.opt_in_note ?? undefined}
          checked={value !== 'off' && isOn(r)}
          disabled={value === 'off'}
          onChange={(on) => setAgent(r.id, on)}
        />
      ))}
    </>
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
    hint: 'Keep an agent started and waiting, so opening a pane feels instant.',
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
    keywords: 'hooks hook lane live updates claude codex watch report trust',
    group: 'Live updates',
    labs: { name: 'Live updates', about: 'Claude Code and Codex report when they start, ask and finish.' },
    render: (env) => <LiveUpdates env={env} />,
  },
  {
    id: 'keep-going',
    label: 'When an agent runs out of usage',
    hint: 'Pick the agents a task can move to when the agent it runs on runs out of usage.',
    keywords: 'keep going continue move another agent limit usage run out reset back first resets offer order fallback backup hand over ask runs',
    group: 'Keep going',
    render: (env) => <KeepGoingRows onSaved={env.markSaved} />,
  },
  {
    id: 'collision',
    label: COLLISION_LABEL,
    hint: 'Puppet Master warns you when two agents change the same file. It never pauses or stops an agent.',
    keywords: 'collision clash conflict same file two agents overlap warn warning changed lines copies folder alert',
    group: 'Same file',
    render: (env) => <CollisionSettings onSaved={env.markSaved} />,
  },
  {
    id: 'definitions',
    label: 'Agent definitions',
    hint: 'Add your own agents with a small settings file. A new or changed one stays off until you review and approve it.',
    keywords: 'toml consent project sources drafts import export fixtures revoke approve review extra directories generic agent reload',
    render: (env) => <AgentDefinitions onOpenStore={env.openStore} />,
  },
];
