// Settings > Agents: logins (accounts), keeping agents warm (preload) and agent definitions (TOML files,
// consent, review and approval). Each block saves through its own command, so no row here calls env.patch.
import { AccountsGroup } from '../accounts/AccountsGroup';
import { AgentDefinitions } from '../blocks/AgentDefinitions';
import { PreloadSettings } from '../blocks/PreloadSettings';
import type { RowDef } from '../model';

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
    id: 'definitions',
    label: 'Agent definitions',
    hint: 'Agents defined by TOML files. A new or changed one stays inactive until you review and approve it.',
    keywords: 'toml consent project sources drafts import export fixtures revoke approve review extra directories generic agent reload',
    render: () => <AgentDefinitions />,
  },
];
