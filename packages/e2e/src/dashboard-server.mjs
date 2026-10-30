#!/usr/bin/env node
// The developer dashboard: npm run e2e:dashboard [-- --port 4747]
// A small local web server (127.0.0.1 only) that lists every test, runs one / an area / all through the same CLI
// (one child process at a time), streams its log live (Server-Sent Events) and plays the videos of past runs.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { RESULTS_DIR, E2E_DIR, newRunId, readBuildInfo } from './paths.mjs';
import { loadTests, select } from './registry.mjs';
import { assertSafePort } from './guards.mjs';
import { freePort } from './launcher.mjs';

const DASH_DIR = path.join(E2E_DIR, 'dashboard');
const CLI = path.join(path.dirname(fileURLToPath(import.meta.url)), 'cli.mjs');
const argv = process.argv.slice(2);
const argValue = (n) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : undefined; };

const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.mp4': 'video/mp4', '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml' };
const SAFE = /^[A-Za-z0-9._-]+$/;

// ---- the one job at a time ----------------------------------------------------------------------------------------
let job = null; // { id, runId, selectors, status, startedAt, tests: {id: status}, current, log: [], child }
const clients = new Set();
const send = (res, event, data) => res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
const broadcast = (event, data) => { for (const c of clients) { try { send(c, event, data); } catch { clients.delete(c); } } };
const jobView = (j) => (j ? { id: j.id, runId: j.runId, selectors: j.selectors, status: j.status, startedAt: j.startedAt, finishedAt: j.finishedAt, exitCode: j.exitCode, tests: j.tests, current: j.current, total: j.total, options: j.options } : null);

function startJob(selectors, options) {
  const runId = newRunId();
  const args = [CLI, '--events', '--run-id', runId];
  if (selectors.length) args.push('--only', selectors.join(','));
  if (options.video === false) args.push('--no-video');
  if (options.fresh) args.push('--fresh');
  if (options.headless) args.push('--headless');
  const child = spawn(process.execPath, args, { cwd: E2E_DIR, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  job = { id: runId, runId, selectors, options, status: 'running', startedAt: new Date().toISOString(), tests: {}, current: null, total: 0, log: [], child };
  const j = job;
  const onLine = (line) => {
    if (line.startsWith('@@E2E ')) {
      let e; try { e = JSON.parse(line.slice(6)); } catch { return; }
      if (e.type === 'run-start') j.total = e.total;
      if (e.type === 'test-start') { j.current = e.id; j.tests[e.id] = 'running'; }
      if (e.type === 'test-end') { j.tests[e.id] = e.status; if (j.current === e.id) j.current = null; }
      broadcast('e2e', e);
      return;
    }
    j.log.push(line); if (j.log.length > 4000) j.log.shift();
    broadcast('log', { line });
  };
  for (const stream of [child.stdout, child.stderr]) {
    let buf = '';
    stream.setEncoding('utf8');
    stream.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { onLine(buf.slice(0, i).replace(/\r$/, '')); buf = buf.slice(i + 1); } });
    stream.on('end', () => { if (buf) onLine(buf); });
  }
  child.on('exit', (code) => {
    j.exitCode = code; j.finishedAt = new Date().toISOString();
    j.status = j.status === 'stopping' ? 'stopped' : code === 0 ? 'passed' : code === 1 ? 'failed' : 'error';
    broadcast('job', jobView(j));
  });
  broadcast('job', jobView(j));
  return j;
}

function stopJob() {
  if (!job || job.status !== 'running') return false;
  job.status = 'stopping';
  try { job.child.stdin.write('stop\n'); } catch { /* gone */ }
  // a graceful stop closes the app; if the child does not end soon, close its input (the CLI treats that as stop too)
  setTimeout(() => { try { job.child.stdin.end(); } catch { /* gone */ } }, 3000);
  broadcast('job', jobView(job));
  return true;
}

// ---- results on disk ----------------------------------------------------------------------------------------------
const readJson = (f) => { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; } };
function listRuns(limit = 60) {
  if (!fs.existsSync(RESULTS_DIR)) return [];
  return fs.readdirSync(RESULTS_DIR, { withFileTypes: true }).filter((d) => d.isDirectory() && SAFE.test(d.name)).map((d) => d.name).sort().reverse().slice(0, limit)
    .map((id) => ({ runId: id, ...(readJson(path.join(RESULTS_DIR, id, 'run.json')) || { status: 'unknown' }) }));
}
function lastResults(runs) {
  const last = {};
  for (const r of runs) {
    const dir = path.join(RESULTS_DIR, r.runId);
    for (const id of r.tests || []) {
      if (last[id]) continue;
      const res = readJson(path.join(dir, id, 'result.json'));
      if (res) last[id] = { runId: r.runId, status: res.status, durationMs: res.durationMs, finishedAt: res.finishedAt, video: !!res.video, error: res.error?.message || null };
    }
  }
  return last;
}

// ---- http ---------------------------------------------------------------------------------------------------------
const json = (res, code, body) => { res.writeHead(code, { 'content-type': 'application/json', 'cache-control': 'no-store' }); res.end(JSON.stringify(body)); };

function serveFile(req, res, file) {
  let st; try { st = fs.statSync(file); } catch { return json(res, 404, { error: 'not found' }); }
  if (!st.isFile()) return json(res, 404, { error: 'not found' });
  const type = MIME[path.extname(file).toLowerCase()] || 'application/octet-stream';
  const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range || '');
  if (range) {
    const start = range[1] ? +range[1] : Math.max(0, st.size - +range[2]);
    const end = range[1] && range[2] ? Math.min(+range[2], st.size - 1) : st.size - 1;
    if (start > end || start >= st.size) { res.writeHead(416, { 'content-range': `bytes */${st.size}` }); return res.end(); }
    res.writeHead(206, { 'content-type': type, 'content-length': end - start + 1, 'content-range': `bytes ${start}-${end}/${st.size}`, 'accept-ranges': 'bytes' });
    return fs.createReadStream(file, { start, end }).pipe(res);
  }
  res.writeHead(200, { 'content-type': type, 'content-length': st.size, 'accept-ranges': 'bytes', 'cache-control': type.startsWith('video') || type.startsWith('image') ? 'max-age=60' : 'no-store' });
  fs.createReadStream(file).pipe(res);
}

async function readBody(req) {
  let s = ''; for await (const c of req) { s += c; if (s.length > 100000) throw new Error('body too large'); }
  return s ? JSON.parse(s) : {};
}

export function createServer(port) {
  return http.createServer(async (req, res) => {
    try {
      // Only this machine, under its own name: a web page elsewhere cannot drive the dashboard.
      const host = String(req.headers.host || '');
      if (!new RegExp(`^(127\\.0\\.0\\.1|localhost|\\[::1\\]):${port}$`).test(host)) return json(res, 403, { error: 'bad host' });
      const url = new URL(req.url, 'http://x');
      const p = url.pathname;
      if (req.method === 'POST' && req.headers['x-e2e-dashboard'] !== '1') return json(res, 403, { error: 'missing x-e2e-dashboard header' });

      if (req.method === 'GET' && (p === '/' || p === '/index.html')) return serveFile(req, res, path.join(DASH_DIR, 'index.html'));
      if (req.method === 'GET' && /^\/static\/[A-Za-z0-9._-]+$/.test(p)) return serveFile(req, res, path.join(DASH_DIR, p.slice(8)));

      if (req.method === 'GET' && p === '/api/state') {
        const { tests, problems } = await loadTests({ bust: true });
        const runs = listRuns();
        return json(res, 200, {
          tests: tests.map(({ run, setup, ...t }) => t), problems, last: lastResults(runs), job: jobView(job), build: readBuildInfo(), resultsDir: RESULTS_DIR,
        });
      }
      if (req.method === 'GET' && p === '/api/runs') return json(res, 200, listRuns());
      let m;
      if (req.method === 'GET' && (m = /^\/api\/runs\/([^/]+)$/.exec(p)) && SAFE.test(m[1])) {
        const run = readJson(path.join(RESULTS_DIR, m[1], 'run.json'));
        if (!run) return json(res, 404, { error: 'no such run' });
        const results = (run.tests || []).map((id) => readJson(path.join(RESULTS_DIR, m[1], id, 'result.json'))).filter(Boolean);
        return json(res, 200, { ...run, results });
      }
      if (req.method === 'GET' && (m = /^\/api\/result\/([^/]+)\/([^/]+)$/.exec(p)) && SAFE.test(m[1]) && SAFE.test(m[2])) {
        const r = readJson(path.join(RESULTS_DIR, m[1], m[2], 'result.json'));
        return r ? json(res, 200, r) : json(res, 404, { error: 'no such result' });
      }
      if (req.method === 'GET' && (m = /^\/results\/([^/]+)\/([^/]+)\/([^/]+)$/.exec(p)) && SAFE.test(m[1]) && SAFE.test(m[2]) && SAFE.test(m[3])) {
        return serveFile(req, res, path.join(RESULTS_DIR, m[1], m[2], m[3]));
      }
      if (req.method === 'GET' && p === '/api/events') {
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
        send(res, 'job', jobView(job));
        if (job) send(res, 'backlog', { lines: job.log.slice(-400) });
        clients.add(res);
        const ka = setInterval(() => { try { res.write(': keep-alive\n\n'); } catch { /* closed */ } }, 20000);
        req.on('close', () => { clearInterval(ka); clients.delete(res); });
        return;
      }
      if (req.method === 'POST' && p === '/api/run') {
        if (job && (job.status === 'running' || job.status === 'stopping')) return json(res, 409, { error: 'a run is in progress', job: jobView(job) });
        const body = await readBody(req);
        const { tests } = await loadTests();
        const selectors = (Array.isArray(body.only) ? body.only : []).map(String);
        const picked = select(tests, selectors);
        if (!picked.length) return json(res, 400, { error: 'no test matches', selectors });
        // pass exact ids to the CLI, never the raw text of the request
        const j = startJob(picked.map((t) => t.id), { video: body.video !== false, fresh: !!body.fresh, headless: !!body.headless });
        return json(res, 202, { jobId: j.id, runId: j.runId, tests: picked.map((t) => t.id) });
      }
      if (req.method === 'POST' && p === '/api/stop') return json(res, 200, { stopping: stopJob() });
      return json(res, 404, { error: 'not found' });
    } catch (e) {
      return json(res, 500, { error: String(e.message || e) });
    }
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const port = +(argValue('--port') || process.env.E2E_DASHBOARD_PORT || 0) || (await freePort());
  assertSafePort(port, 'dashboard port');
  const server = createServer(port);
  server.listen(port, '127.0.0.1', () => {
    const url = `http://127.0.0.1:${port}/`;
    console.log(`E2E dashboard: ${url}`);
    console.log(`results folder: ${RESULTS_DIR}`);
  });
  const down = () => { if (job?.status === 'running') stopJob(); setTimeout(() => process.exit(0), 4000); };
  process.on('SIGINT', down);
  process.on('SIGTERM', down);
}
