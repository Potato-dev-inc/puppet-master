// Sidebar title row: bridge status dot, new chat, theme toggle, settings.
interface Props {
  bridgeReady: boolean;
  busy: boolean;
  theme: 'dark' | 'light';
  onNewChat: () => void;
  onToggleTheme: () => void;
  onShowSettings: () => void;
}

export function SidebarHeader({ bridgeReady, busy, theme, onNewChat, onToggleTheme, onShowSettings }: Props) {
  return (
    <div className="px-3 py-2 border-b border-pm-border text-sm font-semibold flex items-center gap-2">
      <span>Puppet Master</span>
      <span
        className={`w-1.5 h-1.5 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`}
        title={bridgeReady ? 'Bridge connected' : 'Bridge not reachable'}
      />
      <div className="flex-1" />
      <button
        onClick={onNewChat}
        className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
        title="Start a new chat (clears history)"
        disabled={busy}
      >
        New chat
      </button>
      <button
        onClick={onToggleTheme}
        className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
        title="Toggle monochrome light/dark theme"
      >
        {theme === 'dark' ? 'Light' : 'Dark'}
      </button>
      <button
        onClick={onShowSettings}
        className="px-1.5 py-0.5 text-xs rounded text-pm-muted hover:bg-pm-border/40"
        title="Settings"
      >
        ⚙
      </button>
    </div>
  );
}
