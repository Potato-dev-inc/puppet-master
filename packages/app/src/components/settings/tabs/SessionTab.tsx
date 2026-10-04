// Settings > Sessions: default orchestrator backend and session-history placeholders.
import { FieldSelect, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import { OrchestratorBackendSelect } from './OrchestratorBackendSelect';
import type { SettingsTabContext } from './tab-context';

export function SessionTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="Sessions" description="Workspace defaults, history, and orchestration behavior.">
      <SettingToggle label="Send initial prompt on start" description="Automatically submit the new-session prompt when a workspace opens." checked onChange={() => undefined} implemented={false} />
      <SettingBlock label="Default orchestrator" implemented description="Sidebar brain for tool routing and delegation.">
        <OrchestratorBackendSelect settings={settings} setSettings={setSettings} />
      </SettingBlock>
      <SettingBlock label="Keep session history" implemented={false} description="How long to retain closed session metadata.">
        <FieldSelect value="30" disabled><option value="7">7 days</option><option value="30">30 days</option><option value="90">90 days</option><option value="365">1 year</option></FieldSelect>
      </SettingBlock>
    </SettingsSection>
  );
}
