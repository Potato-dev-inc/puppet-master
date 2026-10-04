// Orchestrator backend dropdown shared by the Sessions and Orchestrator settings tabs.
import type { OrchestratorBackend } from '@puppet-master/shared';
import { ORCHESTRATOR_BACKEND_LABELS } from '@puppet-master/shared';
import { FieldSelect } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function OrchestratorBackendSelect({
  settings,
  setSettings,
}: Pick<SettingsTabContext, 'settings' | 'setSettings'>) {
  return (
    <FieldSelect value={settings.orchestrator_backend ?? 'api'} onChange={(e) => setSettings({ ...settings, orchestrator_backend: e.target.value as OrchestratorBackend })}>
      {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((backend) => (
        <option key={backend} value={backend}>{ORCHESTRATOR_BACKEND_LABELS[backend]}</option>
      ))}
    </FieldSelect>
  );
}
