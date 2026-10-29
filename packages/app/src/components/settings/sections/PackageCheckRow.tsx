// Settings > General > Safety: "Check packages before installs". On by default. Saves through the package-check
// settings command (not settings.json). Off = only the offline look-alike check runs on the approval card.
// Only the package name is ever sent to a public registry; nothing here names an agent.
import { useEffect, useState } from 'react';
import { packageCheckSettingsGet, packageCheckSettingsSet, type PackageCheckSettings } from '../../../lib/package-check';
import { Note, SettingRow, SettingToggle } from '../settings-ui';

export const PACKAGE_CHECK_LABEL = 'Check packages before installs';
export const PACKAGE_CHECK_NOTE =
  'Before an agent installs a package, look up its age, downloads and install scripts on its public registry (npm, PyPI, crates.io, RubyGems, Go). Only the package name is sent.';
export const PACKAGE_CHECK_OFF_NOTE = 'Look-alike names are still checked on this computer.';

/** The text a failed save shows: the server's plain sentence, without its `CODE:` prefix. */
export function packageCheckSaveError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  const text = raw.replace(/^[A-Z][A-Z0-9_]*:\s*/, '').trim();
  return text || "Couldn't save. Try again.";
}

export function PackageCheckRow({ onSaved }: { onSaved?: () => void }) {
  // undefined = still loading, null = not available here (a browser preview), otherwise the saved setting.
  const [settings, setSettings] = useState<PackageCheckSettings | null | undefined>(undefined);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    packageCheckSettingsGet()
      .then((saved) => alive && setSettings(saved))
      .catch(() => alive && setSettings(null));
    return () => {
      alive = false;
    };
  }, []);

  if (settings === undefined) return <SettingRow label={PACKAGE_CHECK_LABEL} description={PACKAGE_CHECK_NOTE} />;
  if (settings === null) {
    return (
      <SettingRow label={PACKAGE_CHECK_LABEL} description={PACKAGE_CHECK_NOTE}>
        <span className="st-value">Available in the desktop app.</span>
      </SettingRow>
    );
  }

  // The switch moves at once; a failed save puts it back to the value it had before.
  const save = async (lookups: boolean) => {
    const before = settings;
    setError(null);
    setSaving(true);
    setSettings({ lookups });
    try {
      setSettings(await packageCheckSettingsSet({ lookups }));
      onSaved?.();
    } catch (e) {
      setSettings(before);
      setError(packageCheckSaveError(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <SettingToggle
        label={PACKAGE_CHECK_LABEL}
        description={
          settings.lookups ? (
            PACKAGE_CHECK_NOTE
          ) : (
            <>
              {PACKAGE_CHECK_NOTE}
              <br />
              {PACKAGE_CHECK_OFF_NOTE}
            </>
          )
        }
        checked={settings.lookups}
        disabled={saving}
        onChange={(on) => void save(on)}
      />
      {error && <Note role="alert">{error}</Note>}
    </>
  );
}
