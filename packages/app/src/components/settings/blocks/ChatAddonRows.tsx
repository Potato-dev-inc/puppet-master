// Settings > Agents > Chat add-ons: one row per agent that can run as a chat. "Chat add-on: Installed 0.74.0 · Remove" or
// "Not installed · Install". Shown only while Chat (ACP) is on in Labs (the row is also hidden by its `labsFlag`).
// Remove moves the add-on aside (nothing is deleted); Install runs the pinned, checked install. Nothing here names an agent.
import { useAcpAddons } from '../../../hooks/useAcpAddons';
import { progressText, rowText, sizeText, tauriAcpAddonApi, type AcpAddonApi, type AddonStatus } from '../../../lib/acpAddon';
import { Btn, Note, SettingRow } from '../settings-ui';

export const CHAT_ADDONS_LABEL = 'Chat add-ons';
export const CHAT_ADDONS_NOTE = 'The small helper each agent needs to run as a chat. It uses the same sign-in as the agent’s terminal.';

function AddonRow({ status, error, onInstall, onRemove }: { status: AddonStatus; error?: string; onInstall: () => void; onRemove: () => void }) {
  const installing = status.state === 'installing';
  const installed = status.state === 'installed';
  const failed = status.state === 'failed';
  return (
    <SettingRow label={`${status.label} chat add-on`} description={installed || installing || failed ? undefined : `A small download${sizeText(status.size_mb)}.`}>
      <span className="st-value" data-testid={`addon-status-${status.agent}`} role={failed ? 'alert' : 'status'}>
        {installing ? `Installing… ${status.lines.length > 0 ? progressText(status) : ''}`.trim() : rowText(status)}
      </span>
      {installed ? (
        <Btn small onClick={onRemove} aria-label={`Remove the ${status.label} chat add-on`}>
          Remove
        </Btn>
      ) : (
        !installing && (
          <Btn small primary onClick={onInstall} aria-label={`${failed ? 'Try again: install' : 'Install'} the ${status.label} chat add-on`}>
            {failed ? 'Try again' : 'Install'}
          </Btn>
        )
      )}
      {error && <Note role="alert">{error}</Note>}
    </SettingRow>
  );
}

export function ChatAddonRows({ api = tauriAcpAddonApi }: { api?: AcpAddonApi }) {
  const addons = useAcpAddons(api);
  const list = Object.values(addons.byAgent);
  // Not in Labs, or outside the desktop app, or an older build: nothing to show.
  if (!addons.labsEnabled || list.length === 0) return null;
  return (
    <>
      <Note>{CHAT_ADDONS_NOTE}</Note>
      {list.map((status) => (
        <AddonRow key={status.agent} status={status} error={addons.removeErrors[status.agent]} onInstall={() => void addons.install(status.agent)} onRemove={() => void addons.remove(status.agent)} />
      ))}
    </>
  );
}
