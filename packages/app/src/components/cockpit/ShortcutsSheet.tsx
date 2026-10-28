// The keyboard shortcuts sheet: `?`, Ctrl+/ or "Keyboard shortcuts" in the palette. The list is lib/shortcuts.ts.
// Esc closes it (Sheet), and so does the Close button.
import { SHORTCUT_GROUPS, SHORTCUTS, isMacPlatform, shortcutKeys } from '../../lib/shortcuts';
import '../../styles/cockpit-shortcuts.css';
import { Sheet } from './Sheet';

export function ShortcutsSheet({ onClose, mac = isMacPlatform() }: { onClose: () => void; mac?: boolean }) {
  return (
    <Sheet labelledBy="ck-shortcuts-t" role="dialog" onClose={onClose} className="ck-sheet--shortcuts">
      <h2 className="ck-section" id="ck-shortcuts-t">
        Keyboard shortcuts
      </h2>
      {SHORTCUT_GROUPS.map((group) => (
        <section key={group} className="ck-shortcuts-group">
          <h3 className="ck-caption">{group}</h3>
          <ul className="ck-shortcuts-list">
            {SHORTCUTS.filter((s) => s.group === group).map((s) => (
              <li key={s.id} className="ck-shortcuts-row">
                <span className="ck-shortcuts-label">{s.label}</span>
                <span className="ck-shortcuts-keys">
                  {shortcutKeys(s, mac).map((key) => (
                    <kbd key={key} className="ck-kbd">
                      {key}
                    </kbd>
                  ))}
                </span>
              </li>
            ))}
          </ul>
        </section>
      ))}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={onClose}>
          Close
        </button>
      </div>
    </Sheet>
  );
}
