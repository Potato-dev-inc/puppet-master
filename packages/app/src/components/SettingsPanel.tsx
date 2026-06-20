import { useEffect, useMemo, useState } from 'react';
import type { LlmModel, Settings } from '@puppet-master/shared';
import { DEFAULT_DEV_SERVER_PORT } from '../lib/public-bridge-url';
import { findBridgeUrl } from '../lib/bridge';
import { loadSettings, saveSettings } from '../lib/settings';
import { EMPTY_CUSTOM, SettingsTabPanel, type SettingsTabContext } from './settings/settings-tab-panels';
import {
  PlannedBadge,
  SETTINGS_TABS,
  type SettingsTabId,
} from './settings/settings-ui';

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved?: () => void;
  projectPath?: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  onSidebarWidthChange?: (width: number) => void;
  currentSidebarWidth?: number;
}

export function SettingsPanel({
  open,
  onClose,
  onSaved,
  projectPath = null,
  onProjectPathChange,
  onSidebarWidthChange,
  currentSidebarWidth,
}: Props) {
  const [tab, setTab] = useState<SettingsTabId>('general');
  const [search, setSearch] = useState('');
  const [settings, setSettings] = useState<Settings>({
    default_provider: 'anthropic',
    default_model: 'claude-sonnet-4-6',
    custom_models: [],
    orchestrator_backend: 'api',
    mobile_input_delay_ms: 250,
    mobile_input_visible: true,
    dev_server_port: DEFAULT_DEV_SERVER_PORT,
    sidebar_width: 360,
    theme: 'dark',
  });
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [draftCustom, setDraftCustom] = useState<LlmModel>(EMPTY_CUSTOM);
  const [bridgeUrl, setBridgeUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!open) return;
    void loadSettings().then((loaded) => setSettings(loaded));
    setDraftCustom(EMPTY_CUSTOM);
    void findBridgeUrl().then((url) => setBridgeUrl(url));
  }, [open]);

  const filteredTabs = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return SETTINGS_TABS;
    return SETTINGS_TABS.filter((item) => item.label.toLowerCase().includes(query));
  }, [search]);

  const tabCtx: SettingsTabContext = {
    settings,
    setSettings,
    projectPath,
    onProjectPathChange,
    bridgeUrl,
    draftCustom,
    setDraftCustom,
    onSidebarWidthChange,
    currentSidebarWidth,
  };

  if (!open) return null;

  const handleSave = async () => {
    await saveSettings(settings);
    if (settings.project_path && onProjectPathChange) {
      await onProjectPathChange(settings.project_path);
    }
    setSavedAt(Date.now());
    onSaved?.();
  };

  return (
    <div className="fixed inset-0 z-50 flex bg-pm-bg text-pm-text">
      <aside className="flex w-72 shrink-0 flex-col border-r border-pm-border bg-pm-panel">
        <div className="border-b border-pm-border p-4">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-pm-border px-3 py-1.5 text-sm hover:bg-pm-border/40"
          >
            ← Back
          </button>
          <h1 className="mt-4 text-xl font-semibold tracking-tight">Settings</h1>
          <p className="mt-1 text-sm text-pm-muted">
            Configure desktop, bridge, agents, and policies. Red badges mark planned features.
          </p>
          <div className="mt-4 flex items-center gap-2 rounded-xl border border-pm-border bg-pm-bg px-3 py-2">
            <span className="text-pm-muted">⌕</span>
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search settings"
              className="min-w-0 flex-1 bg-transparent text-sm outline-none"
            />
          </div>
        </div>

        <nav className="flex-1 space-y-0.5 overflow-auto p-2">
          {filteredTabs.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => setTab(item.id)}
              className={[
                'flex w-full items-center gap-2 rounded-xl px-3 py-2.5 text-left text-sm transition',
                tab === item.id
                  ? 'bg-pm-accent/10 font-medium text-pm-accent'
                  : 'text-pm-muted hover:bg-pm-border/30 hover:text-pm-text',
              ].join(' ')}
            >
              <span className="min-w-0 flex-1 truncate">{item.label}</span>
              {item.planned && <PlannedBadge compact />}
            </button>
          ))}
        </nav>
      </aside>

      <section className="min-w-0 flex-1 overflow-auto">
        <div className="mx-auto max-w-3xl px-8 py-8">
          <SettingsTabPanel tab={tab} ctx={tabCtx} />

          <div className="mt-8 flex items-center justify-end gap-3 border-t border-pm-border pt-6">
            {savedAt && <span className="mr-auto text-xs text-pm-ok">Saved</span>}
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-pm-border px-4 py-2 text-sm hover:bg-pm-border/40"
            >
              Done
            </button>
            <button
              type="button"
              onClick={() => void handleSave()}
              className="rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-4 py-2 text-sm font-medium text-pm-accent hover:bg-pm-accent/20"
            >
              Save changes
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
