// Settings > General > About: what changed in this release, in a few plain lines. Each line jumps to the setting
// it talks about (the panel's `openRow`), so the list is also a short tour.
import type { SettingsEnv } from '../model';
import { SettingBlock } from '../settings-ui';

export const WHATS_NEW_LABEL = "What's new in 0.2.0";

const ITEMS: { text: string; to: string }[] = [
  { text: 'Budgets: limit the turns or minutes an agent may use.', to: 'budget-turns' },
  { text: 'Safety: choose whether an agent may control another agent, and see who allowed what.', to: 'security-ask-before-control' },
  { text: 'Add Puppet Master to your agents in one reviewed step.', to: 'agent-pack' },
  { text: 'Labs: Team, the Ask bar, Simple mode and live updates, all off until you turn them on.', to: 'labs-index' },
  { text: 'A diagnostic file you can save to ask for help. Nothing is sent anywhere.', to: 'diagnostic-file' },
];

export function WhatsNew({ env }: { env: SettingsEnv }) {
  return (
    <SettingBlock label={WHATS_NEW_LABEL} description="The short version. Tap a line to go to it.">
      <ul className="st-labs-list">
        {ITEMS.map((item) => (
          <li key={item.to}>
            <button type="button" className="st-labs-item" onClick={() => env.openRow?.(item.to)}>
              <span className="st-labs-about">{item.text}</span>
            </button>
          </li>
        ))}
      </ul>
    </SettingBlock>
  );
}
