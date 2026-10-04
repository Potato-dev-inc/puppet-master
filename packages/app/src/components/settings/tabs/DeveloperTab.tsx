// Settings > Developer: diagnostics and the bundled-Rust-MCP switch.
import { useEffect, useState } from 'react';
import { resolveSettingsFilePath } from '../../../lib/settings';
import { CodeBlock, InfoCard, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function DeveloperTab({ ctx }: { ctx: SettingsTabContext }) {
  const { bridgeUrl, settings, setSettings } = ctx;
  const [settingsPath, setSettingsPath] = useState<string>('…');
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  return (
    <SettingsSection title="Developer" description="Diagnostics and integration details for plugin authors.">
      <InfoCard title="Bridge endpoint" description={bridgeUrl ?? 'Bridge not discovered yet. Start the desktop app.'} />
      <InfoCard title="Settings file" description={`All desktop preferences persist to ${settingsPath} under the "settings" key.`} />
      <SettingToggle
        label="Use bundled Rust MCP"
        description="When enabled, app-installed MCP configs launch the bundled puppet-master-mcp binary instead of npx."
        checked={settings.developer_use_rust_mcp ?? false}
        implemented
        onChange={(value) => setSettings({ ...settings, developer_use_rust_mcp: value })}
      />
      <SettingToggle label="Log MCP tool calls" description="Verbose bridge and orchestration debugging." checked={false} implemented={false} onChange={() => undefined} />
      <SettingToggle label="Show protocol inspector" description="Display raw SSE events and bridge payloads in workspace." checked={false} implemented={false} onChange={() => undefined} />
      <CodeBlock title="Bridge health response" code={"{\n  \"ok\": true,\n  \"version\": \"0.8.0\",\n  \"panes\": 3,\n  \"transport\": [\"http\", \"sse\"],\n  \"latency_ms\": 18,\n  \"features\": [\"guardrails\", \"routing\", \"audit_log\"]\n}"} />
    </SettingsSection>
  );
}
