import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useCallback, useState } from 'react';
import { listPresets, type AgentType } from '@puppet-master/shared';
import logoUrl from '../assets/branding/logo.svg';

interface Props {
  projectPath: string | null;
  onProjectPathChange: (path: string) => Promise<void>;
  onKillAll: () => void;
  onClearBuffers: () => void;
  onRestart: () => void;
  onNewSession?: (agent: AgentType) => void | Promise<void>;
}

export function WorkspaceHeader({
  projectPath,
  onProjectPathChange,
  onKillAll,
  onClearBuffers,
  onRestart,
  onNewSession,
}: Props) {
  const [showNewSession, setShowNewSession] = useState(false);
  const presets = listPresets();

  const pickPath = useCallback(async () => {
    const result = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: (await homeDir()) ?? undefined,
    });
    if (typeof result === 'string') {
      await onProjectPathChange(result);
    }
  }, [onProjectPathChange]);

  return (
    <header className="flex items-center gap-2 px-3 py-2 border-b border-pm-border bg-pm-panel">
      <img
        src={logoUrl}
        alt="Puppet Master"
        className="h-9 w-auto shrink-0 rounded-sm"
        draggable={false}
      />
      <button
        onClick={pickPath}
        className="px-2 py-1 text-xs rounded border border-pm-border bg-pm-bg hover:bg-pm-border/40 truncate max-w-[40ch]"
        title={projectPath ?? 'Click to pick project folder'}
      >
        {projectPath ?? 'Pick project folder…'}
      </button>
      <div className="flex-1" />
      {onNewSession && (
        <div className="relative">
          <button
            onClick={() => setShowNewSession((s) => !s)}
            className="px-2 py-1 text-xs rounded border border-pm-accent/50 bg-pm-accent/10 hover:bg-pm-accent/20 text-pm-accent"
            title="Spawn a new agent session"
          >
            New session
          </button>
          {showNewSession && (
            <div className="absolute top-full right-0 mt-1 z-20 w-44 rounded-md border border-pm-border bg-pm-panel shadow-lg">
              {presets.map((p) => (
                <button
                  key={p.type}
                  onClick={() => {
                    setShowNewSession(false);
                    void onNewSession(p.type);
                  }}
                  className="w-full text-left px-3 py-2 text-xs hover:bg-pm-border/40"
                >
                  {p.label}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      <button
        onClick={onClearBuffers}
        className="px-2 py-1 text-xs rounded border border-pm-border bg-pm-bg hover:bg-pm-border/40"
        title="Clear scrollback (panes keep running)"
      >
        Clear
      </button>
      <button
        onClick={onRestart}
        className="px-2 py-1 text-xs rounded border border-pm-border bg-pm-bg hover:bg-pm-border/40"
        title="Kill all panes and respawn"
      >
        Restart
      </button>
      <button
        onClick={onKillAll}
        className="px-2 py-1 text-xs rounded border border-pm-err/50 bg-pm-err/10 hover:bg-pm-err/20 text-pm-err"
        title="Kill all panes"
      >
        Kill All
      </button>
    </header>
  );
}