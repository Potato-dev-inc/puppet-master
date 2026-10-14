// Settings inside the cockpit's main area (Orchestration mode). It is the same `SettingsPanel` the standalone
// Settings window renders (six sections, search, autosave, reload on `settings://changed`), not a copy; this
// file only supplies what that window's `SettingsApp` supplies (project folder, update check) and the inline
// way out: the Back button and Escape both call `onBack`.
import { useEffect, useRef } from 'react';
import { SettingsPanel } from '../SettingsPanel';
import { UpdateAvailableBanner } from '../UpdateAvailableBanner';
import { useAppUpdateCheck } from '../../hooks/useAppUpdateCheck';
import { useProjectPath } from '../../hooks/useProjectPath';
import type { SettingsPageId } from '../settings/model';
import type { SettingsApi } from '../SettingsPanel';

interface Props {
  /** Page to open on (a deep link such as Settings > Agents > Preload opens Agents). */
  initialTab?: SettingsPageId;
  onBack: () => void;
  onPageChange?: (page: SettingsPageId) => void;
  /** Tests pass fakes for the panel's load/patch/subscribe. */
  api?: SettingsApi;
}

/** True for a field where Escape means "stop editing", not "leave Settings". */
function isEditing(target: EventTarget | null): target is HTMLElement {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (!(target instanceof HTMLInputElement)) return false;
  // Search keeps its own Escape (clear the text); other inputs are typed into.
  return !['checkbox', 'radio', 'button', 'submit', 'range'].includes(target.type);
}

export function SettingsView({ initialTab, onBack, onPageChange, api }: Props) {
  const { projectPath, setProjectPath } = useProjectPath();
  const appUpdate = useAppUpdateCheck(true);
  const host = useRef<HTMLDivElement>(null);
  const backRef = useRef(onBack);
  backRef.current = onBack;

  // Escape goes back, unless something nearer owns it: the palette or a confirm sheet (they handle the key
  // first), a search box with text (Escape clears it), or a field being typed into (Escape leaves the field;
  // a second Escape leaves Settings).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented || e.isComposing) return;
      if (host.current?.querySelector('[role="dialog"]') || document.querySelector('.ck-scrim')) return;
      const target = e.target;
      if (target instanceof HTMLInputElement && target.type === 'search' && target.value !== '') return;
      if (isEditing(target)) {
        target.blur();
        return;
      }
      e.preventDefault();
      backRef.current();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <div className="ck-settings-host" ref={host}>
      <SettingsPanel
        open
        inline
        initialTab={initialTab}
        onPageChange={onPageChange}
        projectPath={projectPath}
        onProjectPathChange={setProjectPath}
        onCheckForUpdates={() => void appUpdate.refresh()}
        updateCheck={appUpdate.result}
        updateChecking={appUpdate.checking}
        onOpenRelease={() => void appUpdate.openRelease()}
        onClose={onBack}
        api={api}
      />
      <UpdateAvailableBanner update={appUpdate} />
    </div>
  );
}
