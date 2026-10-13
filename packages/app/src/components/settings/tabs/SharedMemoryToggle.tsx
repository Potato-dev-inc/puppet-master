// Settings > General: the app-wide "Shared project memory" switch (see components/cockpit/ProjectMemory.tsx).
import { useEffect, useState } from 'react';
import { memoryApi } from '../../../lib/memory-api';
import { SettingToggle } from '../settings-ui';

export function SharedMemoryToggle() {
  const [on, setOn] = useState(true);
  useEffect(() => {
    void memoryApi.globalEnabled().then(setOn).catch(() => undefined);
  }, []);
  return (
    <SettingToggle
      label="Shared project memory"
      description="Every agent in a project reads one memory, kept in .puppet-master/memory.md. Turn off to remove it from agent files."
      checked={on}
      onChange={(value) => {
        setOn(value);
        void memoryApi.setGlobalEnabled(value).catch(() => setOn(!value));
      }}
      implemented
    />
  );
}
