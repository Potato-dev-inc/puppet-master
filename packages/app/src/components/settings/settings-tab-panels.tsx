import { useEffect, useState } from 'react';
import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { LlmModel, LlmProvider, OrchestratorBackend, Settings } from '@puppet-master/shared';
import { ORCHESTRATOR_BACKEND_LABELS } from '@puppet-master/shared';
import { clampSidebarWidth, listModels, resolveSettingsFilePath } from '../../lib/settings';
import { PUPPET_MASTER_MCP_COMMAND } from '../../lib/mcp-config';
import { MobilePairingPanel } from '../MobilePairingPanel';
import { parseDevServerPort } from '../../lib/public-bridge-url';
import {
  CodeBlock,
  FieldInput,
  FieldLabel,
  FieldSelect,
  InfoCard,
  MockListCard,
  SettingBlock,
  SettingToggle,
  SettingsSection,
  StorageBarMock,
  type SettingsTabId,
} from './settings-ui';

export const EMPTY_CUSTOM: LlmModel = {
  provider: 'openrouter',
  model_id: '',
  label: '',
};

const CLAUDE_DESKTOP_MCP = JSON.stringify({ mcpServers: { 'puppet-master': PUPPET_MASTER_MCP_COMMAND } }, null, 2);
const CODEX_MCP = "[mcp_servers.puppet-master]\ncommand = \"npx\"\nargs = [\"-y\",\"@puppet-master/mcp\"]";

export interface SettingsTabContext {
  settings: Settings;
  setSettings: (next: Settings) => void;
  projectPath: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  bridgeUrl: string | null;
  draftCustom: LlmModel;
  setDraftCustom: (next: LlmModel) => void;
  onSidebarWidthChange?: (width: number) => void;
}

export function SettingsTabPanel({ tab, ctx }: { tab: SettingsTabId; ctx: SettingsTabContext }) {
  switch (tab) {
    case 'general': return <GeneralTab ctx={ctx} />;
    case 'appearance': return <AppearanceTab ctx={ctx} />;
    case 'session': return <SessionTab ctx={ctx} />;
    case 'orchestrator': return <OrchestratorTab ctx={ctx} />;
    case 'api': return <ApiTab ctx={ctx} />;
    case 'mcp': return <McpTab ctx={ctx} />;
    case 'mobile': return <MobileTab ctx={ctx} />;
    case 'security': return <SecurityTab />;
    case 'storage': return <StorageTab />;
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

function GeneralTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, projectPath, onProjectPathChange } = ctx;
  const [settingsPath, setSettingsPath] = useState('settings.json');
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  const pickPath = async () => {
    if (!onProjectPathChange) return;
    const result = await openDialog({ directory: true, multiple: false, defaultPath: (await homeDir()) ?? projectPath ?? undefined });
    if (typeof result === 'string') {
      await onProjectPathChange(result);
      setSettings({ ...settings, project_path: result });
    }
  };
  return (
    <SettingsSection title="General" description="Launch behavior, defaults, and product preferences.">
      <InfoCard title="Settings file" description={settingsPath} implemented />
      <SettingBlock label="Default project folder" implemented description="Workspace root for new panes and orchestrator tools.">
        <div className="flex gap-2">
          <FieldInput value={projectPath ?? settings.project_path ?? ''} onChange={(e) => setSettings({ ...settings, project_path: e.target.value })} className="font-mono" placeholder="~/work/my-project" />
          <button type="button" onClick={() => void pickPath()} className="shrink-0 rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Browse</button>
        </div>
      </SettingBlock>
      <SettingBlock label="Theme" implemented description="Desktop chrome theme (stored in settings.json).">
        <FieldSelect
          value={settings.theme ?? 'dark'}
          onChange={(e) => setSettings({ ...settings, theme: e.target.value as Settings['theme'] })}
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </FieldSelect>
      </SettingBlock>
      <SettingToggle label="Auto-save workspace state" description="Persist panes, scrollback, layout, and summaries after important events." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Semantic repository index" description="Build a local index for smarter cross-pane agent context." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Auto session summaries" description="Generate a concise summary when a session is closed or restored." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Reduce motion" description="Disable large transitions and decorative animations." checked={false} onChange={() => undefined} implemented={false} />
    </SettingsSection>
  );
}

function AppearanceTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, onSidebarWidthChange } = ctx;
  return (
    <SettingsSection title="Appearance" description="Tune interface density and terminal layout.">
      <SettingBlock label="Sidebar width" implemented description="Orchestrator sidebar width in the workspace (stored in settings.json).">
        <FieldSelect value={String(settings.sidebar_width ?? 360)} onChange={(e) => { const next = clampSidebarWidth(Number(e.target.value)); setSettings({ ...settings, sidebar_width: next }); onSidebarWidthChange?.(next); }}>
          <option value="300">Compact (300px)</option>
          <option value="360">Comfortable (360px)</option>
          <option value="480">Wide (480px)</option>
          <option value="640">Extra wide (640px)</option>
        </FieldSelect>
      </SettingBlock>
      <SettingBlock label="Default grid columns" implemented={false} description="Starting terminal grid when opening a workspace.">
        <FieldSelect value="2" disabled><option value="1">1 column</option><option value="2">2 columns</option><option value="3">3 columns</option></FieldSelect>
      </SettingBlock>
      <InfoCard title="Design language" description="Warm accents are reserved for primary actions and active states." implemented={false} />
    </SettingsSection>
  );
}

function SessionTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="Sessions" description="Workspace defaults, history, and orchestration behavior.">
      <SettingToggle label="Send initial prompt on start" description="Automatically submit the new-session prompt when a workspace opens." checked onChange={() => undefined} implemented={false} />
      <SettingBlock label="Default orchestrator" implemented description="Sidebar brain for tool routing and delegation.">
        <FieldSelect value={settings.orchestrator_backend ?? 'api'} onChange={(e) => setSettings({ ...settings, orchestrator_backend: e.target.value as OrchestratorBackend })}>
          {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((backend) => (
            <option key={backend} value={backend}>{ORCHESTRATOR_BACKEND_LABELS[backend]}</option>
          ))}
        </FieldSelect>
      </SettingBlock>
      <SettingBlock label="Keep session history" implemented={false} description="How long to retain closed session metadata.">
        <FieldSelect value="30" disabled><option value="7">7 days</option><option value="30">30 days</option><option value="90">90 days</option><option value="365">1 year</option></FieldSelect>
      </SettingBlock>
    </SettingsSection>
  );
}

function OrchestratorTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, draftCustom, setDraftCustom } = ctx;
  const models = listModels(settings).filter((m) => m.provider === settings.default_provider);
  const customForProvider = (settings.custom_models ?? []).filter((m) => m.provider === settings.default_provider);
  const addCustomModel = () => {
    const label = draftCustom.label.trim();
    const model_id = draftCustom.model_id.trim();
    if (!label || !model_id) return;
    const entry: LlmModel = { provider: draftCustom.provider, model_id, label };
    const next = [...(settings.custom_models ?? []).filter((m) => !(m.provider === entry.provider && m.model_id === entry.model_id)), entry];
    setSettings({ ...settings, custom_models: next, default_provider: entry.provider, default_model: entry.model_id });
    setDraftCustom({ ...EMPTY_CUSTOM, provider: draftCustom.provider });
  };
  const removeCustomModel = (target: LlmModel) => {
    const next = (settings.custom_models ?? []).filter((m) => !(m.provider === target.provider && m.model_id === target.model_id));
    setSettings({ ...settings, custom_models: next });
  };
  return (
    <SettingsSection title="Orchestrator" description="Configure the Puppet Master sidebar brain and delegation logic.">
      <SettingBlock label="Backend" implemented>
        <FieldSelect value={settings.orchestrator_backend ?? 'api'} onChange={(e) => setSettings({ ...settings, orchestrator_backend: e.target.value as OrchestratorBackend })}>
          {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((backend) => (
            <option key={backend} value={backend}>{ORCHESTRATOR_BACKEND_LABELS[backend]}</option>
          ))}
        </FieldSelect>
      </SettingBlock>
      <SettingToggle label="Cost-aware model router" description="Route tasks to cheaper or stronger models depending on complexity." checked onChange={() => undefined} implemented={false} />
      <SettingBlock label="Default provider & model" implemented description="Used by the API orchestrator backend.">
        <div className="space-y-3">
          <div>
            <FieldLabel>Provider</FieldLabel>
            <FieldSelect value={settings.default_provider} onChange={(e) => {
              const provider = e.target.value as LlmProvider;
              const first = listModels(settings).find((m) => m.provider === provider);
              setSettings({ ...settings, default_provider: provider, default_model: first?.model_id ?? settings.default_model });
              setDraftCustom({ ...draftCustom, provider });
            }}>
              <option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option>
            </FieldSelect>
          </div>
          <div>
            <FieldLabel>Model</FieldLabel>
            <FieldSelect value={settings.default_model} onChange={(e) => setSettings({ ...settings, default_model: e.target.value })}>
              {models.map((m) => (<option key={`${m.provider}::${m.model_id}`} value={m.model_id}>{m.label}</option>))}
            </FieldSelect>
          </div>
        </div>
      </SettingBlock>
      <SettingBlock label="Custom models" implemented description="Add vendor/model ids for the sidebar picker.">
        {customForProvider.length > 0 && (
          <ul className="mb-3 space-y-1">
            {customForProvider.map((m) => (
              <li key={`${m.provider}::${m.model_id}`} className="flex items-center gap-2 text-xs font-mono">
                <span className="min-w-0 flex-1 truncate">{m.label} — {m.model_id}</span>
                <button type="button" onClick={() => removeCustomModel(m)} className="rounded px-1.5 py-0.5 text-pm-err hover:bg-pm-err/10">Remove</button>
              </li>
            ))}
          </ul>
        )}
        <div className="grid grid-cols-2 gap-2">
          <div><FieldLabel>Provider</FieldLabel><FieldSelect value={draftCustom.provider} onChange={(e) => setDraftCustom({ ...draftCustom, provider: e.target.value as LlmProvider })}><option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option></FieldSelect></div>
          <div><FieldLabel>Display label</FieldLabel><FieldInput value={draftCustom.label} onChange={(e) => setDraftCustom({ ...draftCustom, label: e.target.value })} placeholder="My model" /></div>
        </div>
        <div className="mt-2 flex gap-2">
          <FieldInput value={draftCustom.model_id} onChange={(e) => setDraftCustom({ ...draftCustom, model_id: e.target.value })} className="font-mono" placeholder="model-id or vendor/model" />
          <button type="button" onClick={addCustomModel} disabled={!draftCustom.label.trim() || !draftCustom.model_id.trim()} className="shrink-0 rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm text-pm-accent hover:bg-pm-accent/20 disabled:opacity-50">Add</button>
        </div>
      </SettingBlock>
      <InfoCard title="Shared tool surface" description="Sidebar API loop and external MCP hosts use the same list, spawn, read, write, kill, snapshot, approve, and route bridge tools." />
    </SettingsSection>
  );
}

function ApiTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="API keys" description="Keys are stored in secure OS storage in the desktop build.">
      <SettingBlock label="Anthropic API key" implemented><FieldInput type="password" value={settings.anthropic_api_key ?? ''} onChange={(e) => setSettings({ ...settings, anthropic_api_key: e.target.value })} className="font-mono" placeholder="sk-ant-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenAI API key" implemented><FieldInput type="password" value={settings.openai_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openai_api_key: e.target.value })} className="font-mono" placeholder="sk-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenRouter API key" implemented><FieldInput type="password" value={settings.openrouter_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openrouter_api_key: e.target.value })} className="font-mono" placeholder="sk-or-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="GitHub token" implemented={false}><FieldInput type="password" className="font-mono" placeholder="ghp_…" disabled /></SettingBlock>
      <SettingBlock label="Linear API key" implemented={false}><FieldInput type="password" className="font-mono" placeholder="lin_api_…" disabled /></SettingBlock>
      <SettingToggle label="Validate keys on save" description="Probe provider APIs with a minimal request before storing." checked onChange={() => undefined} implemented={false} />
    </SettingsSection>
  );
}

function McpTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl } = ctx;
  return (
    <SettingsSection title="MCP & bridge" description="External host integration for Cursor, Claude Desktop, Codex, and automation scripts.">
      <SettingBlock label="Bridge endpoint" implemented description="Auto-discovered local HTTP bridge (port range 17321–17399).">
        <FieldInput value={bridgeUrl ?? 'Discovering…'} readOnly className="font-mono" />
      </SettingBlock>
      <SettingToggle label="Remote runners" description="Allow trusted remote machines to host long-running terminal panes." checked={false} onChange={() => undefined} implemented={false} />
      <CodeBlock title="Claude Desktop config" code={CLAUDE_DESKTOP_MCP} />
      <CodeBlock title="Codex config" code={CODEX_MCP} />
    </SettingsSection>
  );
}

function MobileTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  return (
    <SettingsSection title="Mobile PWA" description="Pair a phone to mirror panes, send input, and monitor agent progress.">
      <SettingBlock label="Mobile input buffer (ms)" implemented description="Delay before committing mobile keystrokes to the PTY.">
        <FieldInput type="number" min={0} max={1000} step={50} value={settings.mobile_input_delay_ms ?? 250} onChange={(e) => {
          const value = Number(e.target.value);
          setSettings({ ...settings, mobile_input_delay_ms: Number.isFinite(value) ? value <= 0 ? 0 : Math.min(1000, Math.max(50, Math.round(value))) : 250 });
        }} className="font-mono" />
      </SettingBlock>
      <SettingToggle label="Show mobile input box" description="When disabled, the tap target stays invisible on the phone." checked={settings.mobile_input_visible ?? true} onChange={(value) => setSettings({ ...settings, mobile_input_visible: value })} implemented />
      <SettingToggle label="Require pairing approval" description="Ask for confirmation on desktop before a phone can connect." checked onChange={() => undefined} implemented={false} />
      <MobilePairingPanel publicPwaUrl={settings.public_pwa_url ?? ''} devServerPort={parseDevServerPort(settings.dev_server_port)} onPublicPwaUrlChange={(value) => setSettings({ ...settings, public_pwa_url: value.trim() || undefined })} onDevServerPortChange={(port) => setSettings({ ...settings, dev_server_port: port })} />
    </SettingsSection>
  );
}

function SecurityTab() {
  return (
    <SettingsSection title="Security" description="Protect API keys, local bridge access, and paired devices.">
      <SettingToggle label="Encrypted local storage" description="Encrypt settings, session history, and sensitive metadata." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Require approval for external MCP clients" description="Prompt before Cursor, Claude Desktop, or Codex can control panes." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Mask secrets in terminal scrollback" description="Redact detected API keys and tokens from saved history." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Audit log" description="Record every pane write, MCP tool call, model route, and approval decision." checked implemented={false} onChange={() => undefined} />
      <InfoCard title="Security status" description="OS keychain is available. Bridge accepts local loopback clients unless mobile pairing is enabled." implemented={false} />
    </SettingsSection>
  );
}

function StorageTab() {
  return (
    <SettingsSection title="Storage" description="Manage pane scrollback, snapshots, caches, and session archives.">
      <StorageBarMock label="Session history" value="1.2 GB" percent="w-2/5" />
      <StorageBarMock label="Pane scrollback" value="684 MB" percent="w-1/3" />
      <StorageBarMock label="Semantic index" value="514 MB" percent="w-1/3" />
      <StorageBarMock label="Agent cache" value="312 MB" percent="w-1/4" />
      <StorageBarMock label="MCP logs" value="88 MB" percent="w-1/6" />
      <button type="button" disabled className="rounded-lg border border-pm-border px-3 py-2 text-sm opacity-50">Clear expired history</button>
    </SettingsSection>
  );
}

function NotificationsTab() {
  return (
    <SettingsSection title="Notifications" description="Choose when Puppet Master should interrupt you.">
      <SettingToggle label="Desktop notifications" description="Show system notifications for completed tasks and blocked panes." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when agent waits for input" description="Useful for Claude confirmation prompts and failed commands." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when tests fail" description="Send a notification when a Bash pane detects test failures." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify on cost threshold" description="Warn when a session approaches the configured daily cost limit." checked implemented={false} onChange={() => undefined} />
    </SettingsSection>
  );
}

function BackupTab() {
  return (
    <SettingsSection title="Backup & sync" description="Keep encrypted workspace metadata available across machines.">
      <SettingToggle label="Cloud backup" description="Upload encrypted session metadata and settings to your account." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Include terminal scrollback" description="Back up pane output. Secrets are redacted before upload." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Team sync preview" description="Sync approved workspace summaries to a shared team feed." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" disabled className="rounded-lg border border-pm-border px-3 py-2 text-sm opacity-50">Export local backup</button>
    </SettingsSection>
  );
}

function MarketplaceTab() {
  return (
    <SettingsSection title="Agent marketplace" description="Install curated agents, prompt packs, and workflow presets.">
      <MockListCard title="Available packs" description="Marketplace installs will connect to a curated registry." items={[
        { title: 'Security reviewer', meta: 'Policy-aware code review agent · 18K installs', status: 'Installed' },
        { title: 'PR summarizer', meta: 'Generates release notes and review summaries · 42K installs', status: 'Installed' },
        { title: 'DB migration planner', meta: 'Plans reversible schema migrations · 9K installs' },
        { title: 'Frontend polish agent', meta: 'UI cleanup, accessibility, and responsive QA · 31K installs' },
      ]} />
    </SettingsSection>
  );
}

function RulesTab() {
  return (
    <SettingsSection title="Rules & guardrails" description="Create approval rules for commands, files, tools, and remote execution.">
      <MockListCard title="Guardrail policies" description="Command and tool approval rules are planned." items={[
        { title: 'Block destructive shell commands', meta: 'Require approval for rm -rf, git reset --hard, force push.', status: 'Enabled' },
        { title: 'Protect secrets and env files', meta: 'Prevent agents from printing .env or API tokens.', status: 'Enabled' },
        { title: 'Require approval for dependency changes', meta: 'Prompt before package managers modify lockfiles.', status: 'Enabled' },
        { title: 'Sandbox remote runners', meta: 'Limit remote jobs to approved repos.', status: 'Draft' },
      ]} />
    </SettingsSection>
  );
}

function CostsTab() {
  return (
    <SettingsSection title="Cost controls" description="Estimate, route, and limit model usage across sessions.">
      <SettingBlock label="Daily cost limit" implemented={false}><FieldInput value="$25" disabled /></SettingBlock>
      <SettingToggle label="Cost-aware model routing" description="Use cheaper models for simple tasks and stronger models for complex reasoning." checked implemented={false} onChange={() => undefined} />
      <div className="space-y-2 opacity-70">
        <StorageBarMock label="Anthropic" value="$1.62" percent="w-2/5" />
        <StorageBarMock label="OpenAI" value="$0.54" percent="w-1/5" />
        <StorageBarMock label="OpenRouter" value="$0.25" percent="w-1/6" />
      </div>
    </SettingsSection>
  );
}

function TeamTab() {
  return (
    <SettingsSection title="Team workspace" description="Share approved summaries, templates, and agent recipes with teammates.">
      <SettingToggle label="Enable team sync" description="Publish approved session summaries to a shared workspace feed." checked={false} implemented={false} onChange={() => undefined} />
      <MockListCard title="Members" description="Team presence and roles are not connected yet." items={[
        { title: 'Ada Chen', meta: 'Owner', status: 'online' },
        { title: 'Marco Lee', meta: 'Reviewer', status: 'idle' },
        { title: 'Nina Patel', meta: 'Developer', status: 'offline' },
      ]} />
    </SettingsSection>
  );
}

function AutomationTab() {
  return (
    <SettingsSection title="Automation" description="Run recipes when terminal, git, test, or MCP events happen.">
      <MockListCard title="Recipes" description="Event-driven automation will hook into bridge SSE in a future release." items={[
        { title: 'When tests fail', meta: 'Send failing output to Claude, ask Codex for a patch, rerun Bash tests.', status: 'Enabled' },
        { title: 'When Claude waits for input', meta: 'Notify desktop and mobile, then summarize the pending decision.', status: 'Enabled' },
        { title: 'When PR changes', meta: 'Refresh semantic index and generate review plan.' },
        { title: 'Nightly workspace digest', meta: 'Summarize all active panes and export a Markdown report.' },
      ]} />
    </SettingsSection>
  );
}

function ObservabilityTab() {
  return (
    <SettingsSection title="Observability" description="Monitor model routes, pane events, bridge latency, and MCP calls.">
      <MockListCard title="Metrics" description="Live observability dashboards are planned." items={[
        { title: 'Bridge p95 latency', meta: 'Last 24 hours', status: '24 ms' },
        { title: 'MCP tool calls today', meta: 'Last 24 hours', status: '186' },
        { title: 'Pane writes blocked', meta: 'Guardrails', status: '4' },
        { title: 'Average agent idle time', meta: 'Last 24 hours', status: '38 sec' },
      ]} />
      <InfoCard title="Protocol inspector" description="Enable developer mode to see raw SSE events, request IDs, and MCP payload timing." implemented={false} />
    </SettingsSection>
  );
}

function PluginsTab() {
  return (
    <SettingsSection title="Plugins" description="Extend Puppet Master with sandboxed local plugins.">
      <SettingToggle label="Plugin sandbox" description="Run plugins in a restricted worker with explicit file and network permissions." checked implemented={false} onChange={() => undefined} />
      <MockListCard title="Installed plugins" description="Plugin runtime is not available yet." items={[
        { title: 'GitHub Pull Request Connector', meta: 'Sandboxed local plugin', status: 'Enabled' },
        { title: 'Linear Issue Sync', meta: 'Sandboxed local plugin', status: 'Enabled' },
        { title: 'Slack Digest Publisher', meta: 'Sandboxed local plugin', status: 'Disabled' },
        { title: 'Docker Runner Provider', meta: 'Sandboxed local plugin', status: 'Experimental' },
      ]} />
    </SettingsSection>
  );
}

function DeveloperTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl } = ctx;
  const [settingsPath, setSettingsPath] = useState<string>('…');
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  return (
    <SettingsSection title="Developer" description="Diagnostics and integration details for plugin authors.">
      <InfoCard title="Bridge endpoint" description={bridgeUrl ?? 'Bridge not discovered yet. Start the desktop app.'} />
      <InfoCard title="Settings file" description={`All desktop preferences persist to ${settingsPath} under the "settings" key.`} />
      <SettingToggle label="Log MCP tool calls" description="Verbose bridge and orchestration debugging." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Show protocol inspector" description="Display raw SSE events and bridge payloads in workspace." checked={false} implemented={false} onChange={() => undefined} />
      <CodeBlock title="Bridge health response" code={"{\n  \"ok\": true,\n  \"version\": \"0.8.0\",\n  \"panes\": 3,\n  \"transport\": [\"http\", \"sse\"],\n  \"latency_ms\": 18,\n  \"features\": [\"guardrails\", \"routing\", \"audit_log\"]\n}"} />
    </SettingsSection>
  );
}

function AdvancedTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings } = ctx;
  const exportSettings = () => {
    const redacted = {
      ...settings,
      anthropic_api_key: settings.anthropic_api_key ? '***' : undefined,
      openai_api_key: settings.openai_api_key ? '***' : undefined,
      openrouter_api_key: settings.openrouter_api_key ? '***' : undefined,
    };
    const blob = new Blob([JSON.stringify(redacted, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'puppet-master-settings.json';
    anchor.click();
    URL.revokeObjectURL(url);
  };
  return (
    <SettingsSection title="Advanced" description="Power-user options. Change carefully.">
      <SettingBlock label="Scrollback limit" implemented={false}><FieldInput value="10000" disabled className="font-mono" /></SettingBlock>
      <SettingBlock label="Pane spawn timeout" implemented={false}><FieldInput value="30000" disabled className="font-mono" /></SettingBlock>
      <SettingToggle label="Telemetry" description="Send anonymous product diagnostics." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" onClick={exportSettings} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Export settings JSON</button>
    </SettingsSection>
  );
}
