// Settings tab dispatcher: maps a SettingsTabId to its panel in ./tabs. Each tab owns its own file.
import type { SettingsTabId } from './settings-ui';
import { AgentsTab } from './tabs/AgentsTab';
import { AdvancedTab } from './tabs/AdvancedTab';
import { ApiTab } from './tabs/ApiTab';
import { AppearanceTab } from './tabs/AppearanceTab';
import { DeveloperTab } from './tabs/DeveloperTab';
import { GeneralTab } from './tabs/GeneralTab';
import { McpTab } from './tabs/McpTab';
import { MobileTab } from './tabs/MobileTab';
import { OrchestratorTab } from './tabs/OrchestratorTab';
import { PerformanceTab } from './tabs/PerformanceTab';
import {
  AutomationTab,
  BackupTab,
  CostsTab,
  MarketplaceTab,
  NotificationsTab,
  ObservabilityTab,
  PluginsTab,
  RulesTab,
  SecurityTab,
  TeamTab,
} from './tabs/PlaceholderTabs';
import { SessionTab } from './tabs/SessionTab';
import { StorageTab } from './tabs/StorageTab';
import type { SettingsTabContext } from './tabs/tab-context';

export { EMPTY_CUSTOM, type SettingsTabContext } from './tabs/tab-context';

export function SettingsTabPanel({ tab, ctx }: { tab: SettingsTabId; ctx: SettingsTabContext }) {
  switch (tab) {
    case 'general': return <GeneralTab ctx={ctx} />;
    case 'appearance': return <AppearanceTab ctx={ctx} />;
    case 'session': return <SessionTab ctx={ctx} />;
    case 'orchestrator': return <OrchestratorTab ctx={ctx} />;
    case 'api': return <ApiTab ctx={ctx} />;
    case 'mcp': return <McpTab ctx={ctx} />;
    case 'mobile': return <MobileTab ctx={ctx} />;
    case 'performance': return <PerformanceTab />;
    case 'agents': return <AgentsTab />;
    case 'security': return <SecurityTab />;
    case 'storage': return <StorageTab ctx={ctx} />;
    case 'notifications': return <NotificationsTab />;
    case 'backup': return <BackupTab />;
    case 'marketplace': return <MarketplaceTab />;
    case 'rules': return <RulesTab />;
    case 'costs': return <CostsTab />;
    case 'team': return <TeamTab />;
    case 'automation': return <AutomationTab />;
    case 'observability': return <ObservabilityTab />;
    case 'plugins': return <PluginsTab />;
    case 'developer': return <DeveloperTab ctx={ctx} />;
    case 'advanced': return <AdvancedTab ctx={ctx} />;
    default: { const _exhaustive: never = tab; return _exhaustive; }
  }
}
