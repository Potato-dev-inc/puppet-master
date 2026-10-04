// Settings > Mobile PWA: input buffer, input box visibility, pairing panel.
import { parseDevServerPort } from '../../../lib/public-bridge-url';
import { MobilePairingPanel } from '../../MobilePairingPanel';
import { FieldInput, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function MobileTab({ ctx }: { ctx: SettingsTabContext }) {
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
