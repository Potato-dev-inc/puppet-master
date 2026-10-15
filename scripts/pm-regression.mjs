#!/usr/bin/env node
// Live regression suite for Puppet Master agent runs. Unlike test-mcp-operations.mjs (mock
// bridge), this drives the real MCP binary against the RUNNING app and real backends, so it
// spawns agents and costs a little. Repeat runs catch the intermittent bugs a single pass hides.
//
//   node scripts/pm-regression.mjs [--backends opencode_native,cursor_agent] [--repeat 3]
//        [--only pong,write,failing_check,visible_close] [--binary path] [--project dir] [--keep]
//
// Exit code 1 on any failed assertion, a verified:true result that is wrong, a leftover pane, or
// a handle left running. Cursor runs use trust_workspace on the suite's own temp folder only.
//
// BENCHMARK MODE (--bench): compares backends on small native-subagent-style tasks and multi-file
// tasks against a generated fixture project, with timing and machine-verified correctness.
//
//   node scripts/pm-regression.mjs --bench [--backends opencode_native,cursor_agent,codex,claude]
//        [--tasks small,multi | pong,fix_bug,...] [--repeat 5] [--bench-out results.json]
//        [--max-p95 120] [--task-timeout 600] [--project dir] [--dry-run]
//
//   --bench          run the benchmark instead of the regression scenarios
//   --backends       default in bench mode: the backends (opencode_native, cursor_agent, codex,
//                    claude) whose CLI binary is found on PATH
//   --tasks          groups (small, multi) and/or task ids; default small,multi
//   --repeat N       runs per (backend, task); small tasks also run one warm followup per cold run
//   --bench-out f    write the JSON report (default pm-bench-<timestamp>.json in the cwd)
//   --max-p95 S      optional budget: fail when any group's p95 total wall time exceeds S seconds
//   --task-timeout S per-run wall limit before the run is cancelled (default 180 small / 600 multi)
//   --dry-run        print the plan and self-check the fixtures and checks with reference solutions;
//                    never spawns the MCP binary or any backend
//
// Bench exit code 1 when a run did not complete, a machine check fails locally, PM reported
// verified:true while the local check disagrees, or --max-p95 is exceeded. Results are per-machine:
// see docs/orchestrator/benchmark.md.
//
// Layout: this file is the entry point (main loop, global invariants, exit codes). The pieces live
// in scripts/pm-regression/: context.mjs (args, sandbox, MCP process + RPC client), scenarios.mjs,
// bench.mjs (+ bench-fixture / bench-tasks / bench-stats).
import { rmSync } from 'node:fs';
import { createContext } from './pm-regression/context.mjs';
import { createScenarios } from './pm-regression/scenarios.mjs';
import { createBench } from './pm-regression/bench.mjs';

const ctx = createContext();
const { args, bench, dryRun, backends, repeat, only, proc, sandbox, tool, check, failures, results, handlesSeen } = ctx;
const scenarios = createScenarios(ctx);
const { benchMain, dryRunMain } = createBench(ctx);

async function main() {
  await ctx.rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'pm-regression', version: '1' } });
  proc.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  const health = await tool('bridge_health', {});
  if (health._error) {
    console.error(`bridge unreachable (is the Puppet Master app running?): ${health._error}`);
    process.exit(2);
  }
  console.log(`build ${health.build_id}  catalog ${health.catalog_version}  drift=${health.schema_drift ?? 'n/a'}`);
  check('health: no schema drift', health.schema_drift !== true, health.schema_drift_hint);
  const panesBefore = new Set((JSON.stringify(await tool('list_panes', {})).match(/[0-9a-f]{8}-[0-9a-f-]{27}/g) ?? []));

  if (bench) await benchMain(health);
  else for (const backend of backends) {
    const extra = backend === 'cursor_agent' ? { trust_workspace: true } : {};
    for (const [name, scenario] of Object.entries(scenarios)) {
      if (only && !only.includes(name)) continue;
      for (let round = 1; round <= repeat; round += 1) {
        const before = failures.length;
        const started = Date.now();
        try {
          await scenario(backend, name === 'visible_close' ? undefined : extra);
        } catch (error) {
          failures.push(`${name}: threw ${error.message}`);
        }
        const failed = failures.length > before;
        results.push({ backend, scenario: name, round, ok: !failed, seconds: ((Date.now() - started) / 1000).toFixed(1) });
        console.log(`${failed ? 'FAIL' : 'ok  '} ${backend} ${name} #${round} (${((Date.now() - started) / 1000).toFixed(1)}s)`);
        if (failed) failures.slice(before).forEach((line) => console.log(`       - ${line}`));
      }
    }
  }

  // Global invariants: nothing the suite started is still running, and no pane was leaked.
  for (const handle of new Set(handlesSeen)) {
    const view = await tool('inspect_agent', { handle, project_path: sandbox });
    check(`handle ${handle} settled`, view.status && view.status !== 'running', view.status);
  }
  const panesAfter = JSON.stringify(await tool('list_panes', {})).match(/[0-9a-f]{8}-[0-9a-f-]{27}/g) ?? [];
  const leaked = panesAfter.filter((id) => !panesBefore.has(id));
  check('no leaked panes', leaked.length === 0, leaked.join(', '));
}

try {
  if (dryRun) await dryRunMain();
  else await main();
} finally {
  proc?.kill();
  // A backend process can still hold the folder open for a moment; leaving it behind is harmless.
  if (ctx.ownSandbox && !args.keep) {
    try { rmSync(sandbox, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 }); } catch { /* best effort */ }
  }
}
if (bench) {
  if (failures.length) {
    console.log(`\n${failures.length} benchmark failure(s)`);
    failures.forEach((line) => console.log(` - ${line}`));
    process.exit(1);
  }
  process.exit(0);
}
const passed = results.filter((entry) => entry.ok).length;
console.log(`\n${passed}/${results.length} scenario runs passed; ${failures.length} failed assertion(s)`);
if (failures.length) {
  failures.forEach((line) => console.log(` - ${line}`));
  process.exit(1);
}
