import type { HarnessScenario } from '../types';

// Settings > Agents > Preload with a 5 second idle timeout in the stored config (the field must show 5 seconds) and plain "held back" lines.
const s: HarnessScenario = { name: 'settings-preload-seconds', route: 'settings', settingsTab: 'agents', standbyIdleSecs: 5,
  window: { width: 1440, height: 1300 },
  standbyBlocked: [
    { key: 'claude C:/Users/dev/work/puppet-master', reason: 'idle_timeout', fails: 0, retry_in_ms: 0, cold: true },
    { key: 'codex (headless) C:/Users/dev/work/puppet-master', reason: 'needs_attention', fails: 3, retry_in_ms: 0, cold: true },
    { key: 'cursor_agent [work] C:/Users/dev/work/puppet-master', reason: 'spawn_failed: no such file', fails: 1, retry_in_ms: 15000, cold: false },
  ],
};
export default s;
