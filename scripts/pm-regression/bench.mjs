// Benchmark mode (--bench) and its --dry-run self-check: runs fixture tasks per backend with timing
// and local machine verification, writes the JSON report, and prints the summary table.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { BACKEND_BINARIES, onPath } from './context.mjs';
import { FIXTURE, allLocal, freshFixture, localCheck } from './bench-fixture.mjs';
import { TASKS, TASK_GROUPS } from './bench-tasks.mjs';
import { fmt, printTable, summarizeRuns } from './bench-stats.mjs';

const TERMINAL = ['completed', 'failed', 'cancelled'];
const seconds = (ms) => ms / 1000;
const stamp = (value) => { const t = Date.parse(value); return Number.isNaN(t) ? null : t; };

export function createBench(ctx) {
  const { args, backends, repeat, sandbox, tool, failures, handlesSeen } = ctx;

  function selectedTasks() {
    const wanted = String(args.tasks ?? 'small,multi').split(',').filter(Boolean).flatMap((name) => TASK_GROUPS[name] ?? [name]);
    const unknown = wanted.filter((id) => !TASKS.some((t) => t.id === id));
    if (unknown.length) { console.error(`unknown task(s): ${unknown.join(', ')} (known: ${TASKS.map((t) => t.id).join(', ')})`); process.exit(2); }
    return TASKS.filter((t) => wanted.includes(t.id));
  }

  /** Wait until the handle's turn is terminal or the deadline passes (then cancel it). */
  async function settleRun(view, handle, dir, limitMs) {
    const end = Date.now() + limitMs;
    while (!TERMINAL.includes(view.status) && handle && Date.now() < end) {
      const waited = await tool('wait_agents', { handles: [handle], timeout_ms: Math.min(60_000, Math.max(1000, end - Date.now())), project_path: dir });
      view = waited.runs?.[0] ?? waited.agents?.[0] ?? view;
      if (view._error) break;
    }
    if (!TERMINAL.includes(view.status)) {
      if (handle) await tool('cancel_agent', { handle, project_path: dir });
      view = { ...view, status: 'timeout' };
    }
    return view;
  }
  
  async function benchPhase({ backend, task, round, phase, dir, begin, handleOf, checks, limitMs }) {
    const t0 = Date.now();
    let view = await begin();
    const handle = view.handle ?? handleOf;
    const ttfh = handle ? seconds(Date.now() - t0) : null;
    if (handle) handlesSeen.push(handle);
    view = await settleRun(view, handle, dir, limitMs);
    const totalS = seconds(Date.now() - t0);
    const inspected = handle ? await tool('inspect_agent', { handle, project_path: dir }) : {};
    const pick = (key) => view[key] ?? inspected[key] ?? inspected.run?.[key] ?? inspected.turn?.[key] ?? null;
    const startedAt = pick('started_at');
    const completedAt = pick('completed_at');
    // Prefer the run view's own duration_ms (prompt written -> completion); started/completed stamps
    // are not exposed on every build.
    const stampedMs = stamp(startedAt) !== null && stamp(completedAt) !== null ? stamp(completedAt) - stamp(startedAt) : null;
    const serverMs = typeof view.duration_ms === 'number' ? view.duration_ms : stampedMs;
    const result = view.result ?? inspected.result ?? '';
    const completed = view.status === 'completed';
    const localOk = completed && allLocal(dir, checks, result);
    const pmVerified = view.verified === true || view.acceptance_status === 'passed';
    const row = {
      backend, task: task.id, size: task.size, phase, round, handle: handle ?? null, status: view.status,
      total_s: totalS, ttfh_s: ttfh, started_at: startedAt, completed_at: completedAt, server_s: serverMs === null ? null : seconds(serverMs),
      acceptance: view.acceptance_status ?? null, pm_verified: pmVerified, local_ok: localOk,
      pass: localOk, verified: localOk && pmVerified, error: view.error?.code ?? view._error ?? null,
    };
    const tag = `${backend}/${task.id}/${phase}#${round}`;
    if (!completed) failures.push(`${tag}: did not complete (${view.status} ${row.error ?? ''}${view.error?.context ? ` ${JSON.stringify(view.error.context)}` : ''})`);
    else if (!localOk) failures.push(`${tag}: local machine check failed (PM acceptance ${row.acceptance ?? 'n/a'})`);
    if (pmVerified && completed && !localOk) failures.push(`${tag}: PM reported verified but the local check disagrees`);
    console.log(`${row.pass ? 'ok  ' : 'FAIL'} ${tag} total ${fmt(totalS)}s ttfh ${fmt(ttfh)}s server ${fmt(row.server_s)}s verified=${row.verified}`);
    return { row, handle };
  }
  
  async function benchMain(health) {
    const picked = selectedTasks();
    if (!backends.length) { console.error('no backends: none of the known CLIs were found on PATH; pass --backends'); process.exit(2); }
    const runs = [];
    for (const backend of backends) {
      const extra = backend === 'cursor_agent' ? { trust_workspace: true } : {};
      for (const task of picked) {
        const limitMs = 1000 * Number(args['task-timeout'] ?? (task.size === 'small' ? 180 : 600));
        for (let round = 1; round <= repeat; round += 1) {
          const dir = join(sandbox, `bench-${backend}-${task.id}-${round}`);
          let handle = null;
          try {
            freshFixture(dir, task);
            const cold = await benchPhase({
              backend, task, round, phase: 'cold', dir, limitMs, checks: task.checks,
              begin: () => tool('run_agent', { agent_type: backend, project_path: dir, task: task.prompt, checks: task.checks, background: true, wait_ms: 0, keep_pane: Boolean(task.size === 'small' && task.warm), ...extra }),
            });
            runs.push(cold.row);
            handle = cold.handle;
            if (task.size === 'small' && task.warm && cold.row.status === 'completed' && handle) {
              const warm = task.warm;
              const second = await benchPhase({
                backend, task, round, phase: 'warm', dir, limitMs, checks: warm.checks, handleOf: handle,
                begin: async () => {
                  let view = await tool('followup_task', { handle, task: warm.task, checks: warm.checks, wait_ms: 0, project_path: dir });
                  if (view._error) view = await tool('followup_task', { handle, task: warm.task, wait_ms: 0, project_path: dir });
                  return view;
                },
              });
              runs.push(second.row);
            }
          } catch (error) {
            failures.push(`${backend}/${task.id}#${round}: threw ${error.message}`);
          } finally {
            if (handle) await tool('close_agent', { handle, project_path: dir });
          }
        }
      }
    }
    const summary = summarizeRuns(runs);
    printTable(summary);
    const budget = args['max-p95'] === undefined ? null : Number(args['max-p95']);
    if (budget !== null && Number.isFinite(budget)) {
      for (const s of summary) if (s.p95_s > budget) failures.push(`p95 budget: ${s.backend}/${s.task}/${s.phase} p95 ${fmt(s.p95_s)}s > ${budget}s`);
    }
    const outFile = String(args['bench-out'] === true || args['bench-out'] === undefined ? `pm-bench-${new Date().toISOString().replace(/[:.]/g, '-')}.json` : args['bench-out']);
    writeFileSync(outFile, `${JSON.stringify({
      meta: { date: new Date().toISOString(), build_id: health?.build_id ?? null, node: process.version, platform: `${process.platform}-${process.arch}`, backends, tasks: picked.map((t) => t.id), repeat, max_p95_s: budget },
      runs, summary,
    }, null, 2)}\n`);
    console.log(`wrote ${outFile}`);
  }
  
  /** --dry-run: print the plan and prove every task's checks fail on the pristine fixture and pass after a reference solution. */
  async function dryRunMain() {
    const picked = selectedTasks();
    console.log('PM benchmark dry run (no MCP binary or backend is started)');
    console.log(`backends: ${backends.length ? backends.map((b) => `${b}${BACKEND_BINARIES[b] && !onPath(BACKEND_BINARIES[b]) ? ' (binary not on PATH)' : ''}`).join(', ') : '(none found on PATH)'}`);
    console.log(`repeat: ${repeat}  max-p95: ${args['max-p95'] ?? 'none'}  out: ${args['bench-out'] ?? 'pm-bench-<timestamp>.json'}`);
    const small = picked.filter((t) => t.size === 'small').length;
    const perBackend = (picked.length + small) * repeat;
    console.log(`runs: ${perBackend} per backend (${picked.length} tasks x ${repeat}, plus ${small * repeat} warm followups) = ${perBackend * backends.length} total\n`);
    let bad = 0;
    for (const task of picked) {
      const pristine = join(sandbox, `dry-${task.id}-pristine`);
      const solved = join(sandbox, `dry-${task.id}-solved`);
      freshFixture(pristine, task);
      freshFixture(solved, task);
      let reply = '';
      try { reply = task.solve(solved) ?? ''; } catch (error) { console.log(`  solve threw: ${error.message}`); bad += 1; }
      const before = task.checks.map((check) => localCheck(pristine, check, ''));
      const after = task.checks.map((check) => localCheck(solved, check, reply));
      const resultOnly = task.checks.every((check) => check.type === 'result_contains');
      // fix_bug's hash guard is meant to hold on the pristine tree; every other check set must not pass untouched.
      const ok = after.every(Boolean) && (resultOnly || !before.every(Boolean));
      if (!ok) bad += 1;
      console.log(`${ok ? 'ok  ' : 'BAD '} ${task.id} [${task.size}] checks=${task.checks.length} pristine=${before.map((v) => (v ? 'pass' : 'fail')).join('/')} reference=${after.map((v) => (v ? 'pass' : 'fail')).join('/')}`);
      console.log(`       prompt: ${task.prompt}`);
      if (task.warm) {
        const warmDir = join(sandbox, `dry-${task.id}-warm`);
        freshFixture(warmDir, task);
        let warmReply = '';
        try { task.solve(warmDir); warmReply = task.warm.solve(warmDir) ?? ''; } catch (error) { console.log(`  warm solve threw: ${error.message}`); bad += 1; }
        const warmOk = allLocal(warmDir, task.warm.checks, warmReply);
        if (!warmOk) bad += 1;
        console.log(`       warm ${warmOk ? 'ok' : 'BAD'}: ${task.warm.task}`);
      }
    }
    console.log(`\nfixture files: ${Object.keys(FIXTURE).join(', ')}`);
    if (bad) failures.push(`${bad} dry-run self-check(s) failed`);
    else console.log('all fixture self-checks passed');
  }

  return { benchMain, dryRunMain };
}
