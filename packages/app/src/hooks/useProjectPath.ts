import { useCallback, useEffect, useState } from 'react';
import { loadSettings, saveSettings } from '../lib/settings';
import { isValidProjectPath } from '../lib/project-path';
import { tauri } from '../lib/tauri';

export function useProjectPath() {
  const [projectPath, setProjectPathState] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const settings = await loadSettings();
        const fromRegistry = (await tauri.getProjectPath()) || null;
        const path = settings.project_path ?? fromRegistry;
        if (isValidProjectPath(path)) {
          await tauri.setProjectPath(path);
          const resolved = await tauri.getProjectPath();
          setProjectPathState(resolved);
          if (settings.project_path !== resolved) {
            await saveSettings({ ...settings, project_path: resolved });
          }
        }
      } finally {
        setReady(true);
      }
    })();
  }, []);

  const setProjectPath = useCallback(async (path: string) => {
    await tauri.setProjectPath(path);
    const resolved = await tauri.getProjectPath();
    const settings = await loadSettings();
    await saveSettings({ ...settings, project_path: resolved });
    setProjectPathState(resolved);
  }, []);

  return { projectPath, setProjectPath, ready };
}
