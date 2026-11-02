// A Mac needs the person to turn on Accessibility before the mouse and keyboard can be shared. This says what to
// switch on and where, in plain words, and has one button to check again. Nothing shows when nothing is needed.
import { accessibilityHelp, type DeskPermissions } from '../../lib/desk-permissions';

export function PermissionBanner({ permissions, onRecheck }: { permissions: DeskPermissions | null; onRecheck: () => void }) {
  const help = accessibilityHelp(permissions);
  if (!help) return null;
  return (
    <div className="ck-desk-banner" role="alert" data-testid="permission-banner">
      <p className="ck-desk-row-label">{help.title}</p>
      <p className="ck-desk-hint">{help.steps}</p>
      <div className="ck-desk-actions">
        <button type="button" className="ck-desk-btn" onClick={onRecheck}>
          Check again
        </button>
      </div>
    </div>
  );
}
