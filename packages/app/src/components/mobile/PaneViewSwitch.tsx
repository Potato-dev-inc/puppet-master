// Terminal | Chat for the open agent tab. A plain two-button switch under the tab bar.
import type { PaneView } from '../../lib/mobile-pane-view';
import '../../styles/phone-chat.css';

const LABEL: Record<PaneView, string> = { terminal: 'Terminal', chat: 'Chat' };

export function PaneViewSwitch({ view, onChange }: { view: PaneView; onChange: (view: PaneView) => void }) {
  return (
    <div className="ph-viewbar">
      <div role="group" aria-label="Pane view" className="ph-viewswitch">
        {(['terminal', 'chat'] as const).map((value) => (
          <button key={value} type="button" aria-pressed={view === value} onClick={() => onChange(value)}>
            {LABEL[value]}
          </button>
        ))}
      </div>
    </div>
  );
}
