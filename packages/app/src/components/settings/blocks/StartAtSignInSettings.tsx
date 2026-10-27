// Settings > General: "Start Puppet Master when I sign in" (OS entry, off by default). Saves itself.
import { useStartAtSignIn } from '../../../hooks/useStartAtSignIn';
import { Note, SettingToggle } from '../settings-ui';

export const START_AT_SIGN_IN_LABEL = 'Start Puppet Master when I sign in';
export const START_AT_SIGN_IN_HINT = 'It starts quietly in the tray. Your companion wakes up too.';

export function StartAtSignInSettings({ onSaved }: { onSaved?: () => void }) {
  const start = useStartAtSignIn();
  return (
    <>
      <SettingToggle
        label={START_AT_SIGN_IN_LABEL}
        description={START_AT_SIGN_IN_HINT}
        checked={start.enabled}
        disabled={!start.ready}
        onChange={(value) => {
          void start.change(value).then((ok) => {
            if (ok) onSaved?.();
          });
        }}
      />
      {start.error && <Note role="alert">{start.error}</Note>}
    </>
  );
}
