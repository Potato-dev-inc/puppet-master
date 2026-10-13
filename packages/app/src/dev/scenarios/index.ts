// Registry: every file in this folder that default-exports a HarnessScenario is picked up.
import type { HarnessScenario } from '../types';

const modules = import.meta.glob<{ default: HarnessScenario }>('./*.ts', { eager: true });

export const scenarios: Record<string, HarnessScenario> = {};
for (const [path, mod] of Object.entries(modules)) {
  if (path === './index.ts' || !mod.default) continue;
  scenarios[mod.default.name] = mod.default;
}
