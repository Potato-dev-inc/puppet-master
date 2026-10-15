// "agent" / "tools" tab switcher of the sidebar.
export type SidebarTab = 'agent' | 'tools';

const TABS: SidebarTab[] = ['agent', 'tools'];

export function SidebarTabs({ active, onChange }: { active: SidebarTab; onChange: (tab: SidebarTab) => void }) {
  return (
    <div className="grid grid-cols-2 gap-1 p-2 border-b border-pm-border bg-pm-bg/30" role="tablist" aria-label="Sidebar views">
      {TABS.map((tab) => (
        <button
          key={tab}
          type="button"
          role="tab"
          aria-selected={active === tab}
          onClick={() => onChange(tab)}
          className={`rounded px-2 py-1 text-xs capitalize border ${
            active === tab
              ? 'border-pm-accent bg-pm-accent/15 text-pm-accent'
              : 'border-pm-border text-pm-muted hover:text-pm-text hover:bg-pm-border/30'
          }`}
        >
          {tab}
        </button>
      ))}
    </div>
  );
}
