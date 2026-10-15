// Settings > Phone: how the phone reaches this computer, typing on the phone, pairing and paired devices.
// The public address saves through its own command (TunnelSection), because saving restarts the tunnel.
import { useEffect, useState } from 'react';
import { clampMobileInputDelayMs } from '../../../lib/bridge-settings';
import { parseDevServerPort } from '../../../lib/public-bridge-url';
import { tauri } from '../../../lib/tauri';
import { DEVICES_CHANGED_EVENT, MobilePairingPanel } from '../../MobilePairingPanel';
import { NumberControl } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { Btn, InfoRow, SettingRow, SettingToggle } from '../settings-ui';
import { TunnelSection } from '../TunnelSection';
import { PairedDevices } from '../blocks/PairedDevices';

type PairedDevice = Awaited<ReturnType<typeof tauri.listPairedMobileDevices>>[number];

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// ---------- revoke all devices ----------

function RevokeAllRow({ env }: { env: SettingsEnv }) {
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void tauri.listPairedMobileDevices().then(setDevices).catch(() => undefined);
  }, []);

  const revokeAll = async () => {
    env.setError(null);
    const ok = await env.confirm({
      title: 'Revoke all devices?',
      body: 'Every paired phone will be signed out. They can pair again with a new QR code.',
      confirmLabel: 'Revoke all',
    });
    if (!ok) return;
    setBusy(true);
    try {
      const paired = await tauri.listPairedMobileDevices();
      for (const device of paired) await tauri.revokePairedMobileDevice(device.id);
      window.dispatchEvent(new Event(DEVICES_CHANGED_EVENT));
      env.markSaved();
    } catch (error) {
      env.setError(`Not revoked: ${errorText(error)}`);
    } finally {
      setBusy(false);
      setDevices(await tauri.listPairedMobileDevices());
    }
  };

  return (
    <SettingRow label="Revoke all devices" description="Every paired phone is signed out and must pair again.">
      <Btn disabled={busy || devices.length === 0} onClick={() => void revokeAll()}>
        {busy ? 'Revoking…' : 'Revoke…'}
      </Btn>
    </SettingRow>
  );
}

// ---------- the rows ----------

const NOTIFICATIONS =
  'Turn them on in the phone app after you pair, to hear when an agent needs you. A notification can show the command an agent wants to run on the lock screen; turn previews off in your phone settings if that is a concern.';

export const phoneRows: RowDef[] = [
  {
    id: 'tunnel',
    label: 'Public address',
    hint: 'A fixed address means you pair your phone once.',
    keywords: 'tunnel cloudflare hostname token custom address named restart test connection',
    render: () => <TunnelSection />,
  },
  {
    id: 'phone-notifications',
    label: 'Notifications',
    hint: NOTIFICATIONS,
    keywords: 'alerts push lock screen previews needs you',
    group: 'Phone app',
    render: () => <InfoRow label="Notifications" description={NOTIFICATIONS} value="On the phone" />,
  },
  {
    id: 'mobile-input-delay',
    label: 'Typing delay',
    hint: 'How long to wait before sending what you type on the phone to the agent.',
    keywords: 'mobile input buffer ms delay',
    group: 'Phone app',
    render: (env) => (
      <SettingRow label="Typing delay" description="How long to wait before sending what you type on the phone to the agent.">
        <NumberControl
          aria-label="Typing delay (ms)"
          min={0}
          max={1000}
          step={10}
          suffix="ms"
          value={env.settings.mobile_input_delay_ms ?? 60}
          normalize={clampMobileInputDelayMs}
          onCommit={(ms) => env.patch({ mobile_input_delay_ms: ms })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'mobile-keyboard-resizes',
    label: 'Keyboard resizes the agent',
    hint: 'Off: the agent keeps its size; the keyboard covers the top of the screen (swipe to see it). On: the agent redraws above the keyboard (Claude reprints each time).',
    keywords: 'keyboard resize redraw phone screen',
    group: 'Phone app',
    render: (env) => (
      <SettingToggle
        label="Keyboard resizes the agent"
        description="Off: the agent keeps its size; the keyboard covers the top of the screen (swipe to see it). On: the agent redraws above the keyboard (Claude reprints each time)."
        checked={env.settings.mobile_keyboard_resizes_agent ?? false}
        onChange={(value) => void env.patch({ mobile_keyboard_resizes_agent: value })}
      />
    ),
  },
  {
    id: 'mobile-input-visible',
    label: 'Show the typing box on the phone',
    hint: 'When off, the box under the terminal is hidden, but the terminal still takes your taps.',
    keywords: 'input box tap target mobile phone keyboard',
    group: 'Phone app',
    render: (env) => (
      <SettingToggle
        label="Show the typing box on the phone"
        description="When off, the box under the terminal is hidden, but the terminal still takes your taps."
        checked={env.settings.mobile_input_visible ?? true}
        onChange={(value) => void env.patch({ mobile_input_visible: value })}
      />
    ),
  },
  {
    id: 'pairing',
    label: 'Pair a phone',
    hint: 'Scan the QR code with your phone camera. It opens a link and signs you in.',
    keywords: 'qr code pairing code refresh public pwa url dev server port full control watch answer only',
    group: 'Pairing',
    render: (env) => (
      <MobilePairingPanel
        publicPwaUrl={env.settings.public_pwa_url ?? ''}
        devServerPort={parseDevServerPort(env.settings.dev_server_port)}
        onPublicPwaUrlChange={(value) => env.patch({ public_pwa_url: value.trim() || null })}
        onDevServerPortChange={(port) => env.patch({ dev_server_port: port })}
      />
    ),
  },
  {
    id: 'paired-devices',
    label: 'Paired phones',
    hint: 'What each phone may do (Watch, Answer, Use the terminal, Start tasks) and when it signs out if unused.',
    keywords: 'devices phones abilities permissions watch answer terminal start tasks expiry revoke sign out',
    group: 'Devices',
    render: () => <PairedDevices />,
  },
  {
    id: 'revoke-all-devices',
    label: 'Revoke all devices',
    hint: 'Every paired phone is signed out and must pair again.',
    keywords: 'revoke sign out phones devices unpair',
    group: 'Devices',
    danger: true,
    render: (env) => <RevokeAllRow env={env} />,
  },
];
