// Settings tabs that are visual placeholders only (no persisted settings behind them yet).
import {
  FieldInput,
  InfoCard,
  MockListCard,
  SettingBlock,
  SettingToggle,
  SettingsSection,
  StorageBarMock,
} from '../settings-ui';

export function SecurityTab() {
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

export function NotificationsTab() {
  return (
    <SettingsSection title="Notifications" description="Choose when Puppet Master should interrupt you.">
      <SettingToggle label="Desktop notifications" description="Show system notifications for completed tasks and blocked panes." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when agent waits for input" description="Useful for Claude confirmation prompts and failed commands." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify when tests fail" description="Send a notification when a Bash pane detects test failures." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Notify on cost threshold" description="Warn when a session approaches the configured daily cost limit." checked implemented={false} onChange={() => undefined} />
    </SettingsSection>
  );
}

export function BackupTab() {
  return (
    <SettingsSection title="Backup & sync" description="Keep encrypted workspace metadata available across machines.">
      <SettingToggle label="Cloud backup" description="Upload encrypted session metadata and settings to your account." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Include terminal scrollback" description="Back up pane output. Secrets are redacted before upload." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Team sync preview" description="Sync approved workspace summaries to a shared team feed." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" disabled className="rounded-lg border border-pm-border px-3 py-2 text-sm opacity-50">Export local backup</button>
    </SettingsSection>
  );
}

export function MarketplaceTab() {
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

export function RulesTab() {
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

export function CostsTab() {
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

export function TeamTab() {
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

export function AutomationTab() {
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

export function ObservabilityTab() {
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

export function PluginsTab() {
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
