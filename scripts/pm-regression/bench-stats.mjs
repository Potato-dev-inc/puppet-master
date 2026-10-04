// Benchmark statistics and the summary table printer.
export function percentile(sorted, p) {
  if (!sorted.length) return null;
  const rank = (sorted.length - 1) * p;
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo);
}
export const fmt = (value) => (value === null || value === undefined || Number.isNaN(value) ? '-' : Number(value).toFixed(1));

export function summarizeRuns(runs) {
  const groups = new Map();
  for (const run of runs) {
    const key = `${run.backend}|${run.task}|${run.phase}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(run);
  }
  return [...groups.entries()].map(([key, rows]) => {
    const [backend, task, phase] = key.split('|');
    const totals = rows.map((r) => r.total_s).sort((a, b) => a - b);
    const handles = rows.map((r) => r.ttfh_s).filter((v) => v !== null).sort((a, b) => a - b);
    return {
      backend, task, phase, n: rows.length,
      p50_s: percentile(totals, 0.5), p95_s: percentile(totals, 0.95), min_s: totals[0], max_s: totals[totals.length - 1],
      ttfh_p50_s: percentile(handles, 0.5),
      pass_rate: rows.filter((r) => r.pass).length / rows.length,
      verified_rate: rows.filter((r) => r.verified).length / rows.length,
    };
  });
}
export function printTable(summary) {
  const cols = ['backend', 'task', 'phase', 'n', 'p50', 'p95', 'min', 'max', 'ttfh50', 'pass', 'verified'];
  const rows = summary.map((s) => [s.backend, s.task, s.phase, String(s.n), fmt(s.p50_s), fmt(s.p95_s), fmt(s.min_s), fmt(s.max_s), fmt(s.ttfh_p50_s), `${Math.round(s.pass_rate * 100)}%`, `${Math.round(s.verified_rate * 100)}%`]);
  const widths = cols.map((c, i) => Math.max(c.length, ...rows.map((r) => r[i].length)));
  const line = (cells) => cells.map((c, i) => (i < 3 ? c.padEnd(widths[i]) : c.padStart(widths[i]))).join('  ');
  console.log(`\n${line(cols)}\n${widths.map((w) => '-'.repeat(w)).join('  ')}`);
  rows.forEach((r) => console.log(line(r)));
  console.log('(seconds; p50/p95/min/max = total wall time, ttfh50 = median time to first handle)');
}
