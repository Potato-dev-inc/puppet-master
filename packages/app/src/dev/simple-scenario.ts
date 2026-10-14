// Builds a Simple mode harness scenario: the cockpit route with the switch on, panes that work on a
// copy, and the mocked answers in `simple-fixtures.ts`.
import type { HarnessDom, HarnessScenario } from './types';
import { simplePaneInfo, type SimpleScenario } from './simple-fixtures';

export function simpleScenario(opts: {
  name: string;
  window: { width: number; height: number };
  simple: SimpleScenario;
  select?: string;
  theme?: 'dark' | 'light';
  after?: (dom: HarnessDom) => void | Promise<void>;
}): HarnessScenario {
  return {
    name: opts.name,
    route: 'cockpit',
    window: opts.window,
    theme: opts.theme,
    select: opts.select,
    simple: opts.simple,
    panes: opts.simple.panes.map(simplePaneInfo),
    after: opts.after,
  };
}
