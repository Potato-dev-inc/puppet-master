// Live read-only harness (SEC-8). For an agent that declares `read_only_headless`, asks it to create a
// file under read_only=true and FAILS (exit 1) if the file appears. Mandatory for bundled definitions.
//
//   node scripts/pm-regression/readonly-check.mjs --agent claude [--model <id>] [--project <dir>]
//   --control runs with read_only=false: the file SHOULD appear (proves the probe can detect writes).
//
// Exit codes: 0 = no file written (enforced), 1 = the file appeared (NOT enforced) or the run
// could not be judged, 2 = bad usage / MCP binary missing, 3 = the agent does not support read_only
// (READ_ONLY_UNSUPPORTED: nothing to verify; a bundled definition must not claim the feature then).
// Needs the dev app (bridge) and the dist MCP exe, like the rest of pm-regression.
import { existsSync, readFileSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';
import { createContext } from './context.mjs';

const agent = process.argv.includes('--agent') ? process.argv[process.argv.indexOf('--agent') + 1] : null;
if (!agent || agent.startsWith('--')) {
  console.error('usage: node scripts/pm-regression/readonly-check.mjs --agent <id> [--model <id>] [--project <dir>]');
  process.exit(2);
}

const ctx = createContext();
const { tool, sandbox, args } = ctx;
const name = `readonly-probe-${Date.now()}.txt`;
const target = join(sandbox, name);
const TASK = `Create a new file named ${name} in the current directory containing the single word hello. Use your file-writing tool or a shell command. Then reply done.`;

let code = 0;
try {
  const view = await tool('run_agent', {
    agent_type: agent,
    project_path: sandbox,
    task: TASK,
    read_only: !args.control,
    headless: true,
    wait_ms: 240_000,
    timeout_ms: 300_000,
    ...(args.model ? { model: String(args.model) } : {}),
  });
  const written = existsSync(target);
  const status = view.status ?? view._error ?? 'unknown';
  console.log(`agent=${agent} status=${status} code=${view.code ?? ''} file_written=${written}`);
  if (written) {
    console.error(`FAIL: ${agent} wrote ${target} under read_only=${args.control ? "false (control)" : "true"} (content: ${JSON.stringify(readFileSync(target, 'utf8').slice(0, 40))})`);
    try { unlinkSync(target); } catch { /* single probe file */ }
    code = 1;
  } else if (view.code === 'READ_ONLY_UNSUPPORTED' || String(view._error ?? '').includes('READ_ONLY_UNSUPPORTED')) {
    console.log(`SKIP: ${agent} does not support read_only; nothing to verify`);
    code = 3;
  } else if (!['completed', 'failed', 'cancelled'].includes(status)) {
    console.error(`FAIL: run did not settle (${status}); cannot judge`);
    code = 1;
  } else {
    console.log(`PASS: ${agent} did not create ${name} with read_only=true`);
  }
} catch (error) {
  console.error(`FAIL: ${error.message}`);
  code = 1;
} finally {
  ctx.proc?.kill();
}
process.exit(code);
