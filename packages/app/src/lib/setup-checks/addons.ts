// Chat add-ons: from `acp_addon_status`. Shown only while the Chat Labs switch is on.
import type { AddonList } from '../acpAddon';
import type { CheckRow } from './types';

export function checkAddons(list: AddonList): CheckRow[] {
  return list.addons.map((a): CheckRow => {
    const id = `addon-${a.agent}`;
    if (a.state === 'installed') {
      const old = a.pinned_version !== null && a.version !== a.pinned_version;
      return old
        ? { id, status: 'warn', sentence: `Chat add-on for ${a.label} is old`, fix: { kind: 'installAddon', label: 'Update', agentId: a.agent } }
        : { id, status: 'ok', sentence: `Chat add-on for ${a.label} is up to date`, fix: null };
    }
    if (a.state === 'installing') return { id, status: 'warn', sentence: `Chat add-on for ${a.label} is installing`, fix: null };
    if (a.state === 'failed') {
      return { id, status: 'fail', sentence: a.detail || `Chat add-on for ${a.label} failed`, fix: { kind: 'installAddon', label: 'Install', agentId: a.agent } };
    }
    return { id, status: 'warn', sentence: `Chat add-on for ${a.label} is not installed`, fix: { kind: 'installAddon', label: 'Install', agentId: a.agent } };
  });
}
