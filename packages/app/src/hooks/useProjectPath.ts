import { useCallback, useEffect, useState } from 'react';
import { loadSettings, patchSettings } from '../lib/settings';
import { isValidProjectPath } from '../lib/project-path';
import { subscribeSettingsRefresh } from '../lib/settings-sync';
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
            await patchSettings({ project_path: resolved });
          }
        }
      } finally {
        setReady(true);
      }
    })();
  }, []);

  // The folder can also change behind this window's back (`POST /project-path`, another window): the
  // backend saves it and tells every window with `settings://changed`. Follow the saved value, otherwise
  // `projectPath` (which wins over the settings object in Settings > General) shows the old folder forever.
  useEffect(() => {
    let disposed = false;
    const off = subscribeSettingsRefresh(() => {
      void loadSettings()
        .then((settings) => {
          const saved = settings.project_path;
          if (disposed || !saved || !isValidProjectPath(saved)) return;
          setProjectPathState((current) => (current === saved ? current : saved));
        })
        .catch(() => undefined);
    });
    return () => {
      disposed = true;
      off();
    };
  }, []);

  const setProjectPath = useCallback(async (path: string) => {
    await tauri.setProjectPath(path);
    const resolved = await tauri.getProjectPath();
    await patchSettings({ project_path: resolved });
    setProjectPathState(resolved);
  }, []);

  return { projectPath, setProjectPath, ready };
}
