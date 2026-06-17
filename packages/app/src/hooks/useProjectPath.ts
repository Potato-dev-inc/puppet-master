import { useCallback, useEffect, useState } from 'react';
import { tauri } from '../lib/tauri';
import { loadSettings, saveSettings } from '../lib/settings';

export function useProjectPath() {
  const [projectPath, setProjectPathState] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const settings = await loadSettings();
        const fromRegistry = (await tauri.getProjectPath()) || null;
        const path = settings.project_path ?? fromRegistry;
        if (path) {
          await tauri.setProjectPath(path);
          setProjectPathState(path);
        }
      } finally {
        setReady(true);
      }
    })();
  }, []);

  const setProjectPath = useCallback(async (path: string) => {
    await tauri.setProjectPath(path);
    const settings = await loadSettings();
    await saveSettings({ ...settings, project_path: path });
    setProjectPathState(path);
  }, []);

  return { projectPath, setProjectPath, ready };
}
