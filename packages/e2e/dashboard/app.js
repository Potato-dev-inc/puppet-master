'use strict';
// The dashboard page: plain DOM code, no libraries. Everything shown comes from /api/* and the live stream /api/events.
const $ = (s) => document.querySelector(s);
const h = (tag, attrs = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) { if (k === 'class') e.className = v; else if (k.startsWith('on')) e.addEventListener(k.slice(2), v); else if (v !== false && v != null) e.setAttribute(k, v === true ? '' : v); }
  for (const k of kids.flat()) if (k != null && k !== false) e.append(k.nodeType ? k : document.createTextNode(String(k)));
  return e;
};
const api = (p, opts) => fetch(p, opts).then((r) => r.json());
const post = (p, body) => api(p, { method: 'POST', headers: { 'content-type': 'application/json', 'x-e2e-dashboard': '1' }, body: JSON.stringify(body || {}) });
const secs = (ms) => (ms == null ? '' : (ms / 1000).toFixed(1) + ' s');
const store = { get: (k, d) => { try { return localStorage.getItem(k) ?? d; } catch { return d; } }, set: (k, v) => { try { localStorage.setItem(k, v); } catch { /* private mode */ } } };

const S = { tests: [], last: {}, job: null, live: {}, sel: null, query: '', tags: new Set(), build: null, runs: [] };

function badge(status) {
  const label = { pass: 'pass', fail: 'FAIL', skip: 'skip', running: 'running' }[status] || 'no run';
  return h('span', { class: 'badge ' + (status || '') }, h('i', { class: 'dot' }), label);
}
const statusOf = (id) => S.live[id] || S.last[id]?.status || '';

function shown() {
  const q = S.query.trim().toLowerCase();
  return S.tests.filter((t) => (!q || [t.id, t.title, ...t.tags].join(' ').toLowerCase().includes(q)) && [...S.tags].every((g) => g === 'failed' ? statusOf(t.id) === 'fail' : g === 'edge' ? t.edge : t.tags.includes(g)));
}

function renderTags() {
  const all = new Set(['failed', 'edge']);
  S.tests.forEach((t) => t.tags.forEach((g) => all.add(g)));
  $('#tags').replaceChildren(...[...all].map((g) => h('button', { class: 'chip' + (S.tags.has(g) ? ' on' : ''), onclick: () => { S.tags.has(g) ? S.tags.delete(g) : S.tags.add(g); render(); } }, g)));
}

function renderList() {
  const list = shown();
  const byArea = new Map();
  for (const t of list) { if (!byArea.has(t.area)) byArea.set(t.area, []); byArea.get(t.area).push(t); }
  const busy = S.job && (S.job.status === 'running' || S.job.status === 'stopping');
  const kids = [];
  for (const [area, tests] of byArea) {
    const fails = tests.filter((t) => statusOf(t.id) === 'fail').length;
    kids.push(h('div', { class: 'area' }, area, h('span', { class: 'count' }, `${tests.length}${fails ? ` · ${fails} failing` : ''}`), h('span', { class: 'spacer' }),
      h('button', { disabled: busy, onclick: () => run(tests.map((t) => t.id)) }, 'Run area')));
    for (const t of tests) {
      const last = S.last[t.id];
      kids.push(h('div', { class: 'row' + (S.sel === t.id ? ' sel' : ''), onclick: () => select(t.id) },
        badge(statusOf(t.id)),
        h('div', { class: 'title', title: t.title }, h('span', { class: 'id' }, t.id.split('.').slice(1).join('.')), '  ', t.title, t.edge ? h('span', { class: 'tag' }, '  edge') : null, ...t.tags.map((g) => h('span', { class: 'tag' }, ' #' + g))),
        h('span', { class: 'dim mono' }, last ? secs(last.durationMs) : ''),
        h('button', { disabled: busy, onclick: (e) => { e.stopPropagation(); run([t.id]); } }, 'Run')));
    }
  }
  $('#list').replaceChildren(...(kids.length ? kids : [h('div', { class: 'empty' }, S.tests.length ? 'No test matches the filter.' : 'No tests found in packages/e2e/tests.')]));
  $('#runShown').textContent = `Run ${list.length === S.tests.length ? 'all' : list.length + ' shown'}`;
  $('#runShown').disabled = busy || !list.length;
}

async function select(id) {
  S.sel = id; renderList(); document.body.classList.add('showdetail');
  const t = S.tests.find((x) => x.id === id);
  const last = S.last[id];
  const box = h('div');
  box.append(h('h2', {}, t.title), h('div', { class: 'dim mono' }, t.id, '  ·  ', t.file, t.edge ? '  ·  edge case' : '', t.tags.length ? '  ·  #' + t.tags.join(' #') : ''));
  $('#detail').replaceChildren(box);
  if (!last) { box.append(h('div', { class: 'empty' }, 'Not run yet. Press Run.')); return; }
  await showResult(box, id, last.runId);
}

async function showResult(box, id, runId) {
  const r = await api(`/api/result/${runId}/${id}`);
  const holder = h('div');
  holder.append(h('h3', {}, 'Result'), h('div', {}, badge(r.status), '   ', secs(r.durationMs), '   ', h('span', { class: 'dim mono' }, runId)));
  if (r.error) holder.append(h('h3', {}, 'Error'), h('pre', { class: 'err' }, r.error.message + (r.error.stack ? '\n\n' + r.error.stack : '')));
  if (r.skipReason) holder.append(h('div', { class: 'dim' }, 'Skipped: ' + r.skipReason));
  if (r.video) {
    const v = h('video', { controls: true, preload: 'metadata', src: `/results/${runId}/${id}/${r.video}` });
    holder.append(h('h3', {}, 'Video'), v);
    if (r.steps?.length) holder.append(h('h3', {}, 'Steps'), h('ul', { class: 'steps' }, r.steps.map((s) => h('li', { onclick: () => { v.currentTime = Math.max(0, s.atMs / 1000 - 0.3); v.play(); } }, h('span', { class: 't' }, (s.atMs / 1000).toFixed(1) + ' s'), s.name))));
  } else if (r.steps?.length) holder.append(h('h3', {}, 'Steps'), h('ul', { class: 'steps' }, r.steps.map((s) => h('li', {}, h('span', { class: 't' }, (s.atMs / 1000).toFixed(1) + ' s'), s.name))));
  if (r.screenshot) holder.append(h('h3', {}, 'Screenshot at the failure'), h('img', { class: 'shot', src: `/results/${runId}/${id}/${r.screenshot}` }));
  if (r.logs?.length) holder.append(h('h3', {}, 'Log'), h('pre', { class: 'err' }, r.logs.map((l) => `${(l.atMs / 1000).toFixed(1)} s  ${l.line}`).join('\n')));
  if (r.consoleErrors?.length) holder.append(h('h3', {}, 'Page console errors'), h('pre', { class: 'err' }, r.consoleErrors.join('\n')));
  // history of this test across the runs we know
  const past = S.runs.filter((x) => (x.results || []).some((y) => y.id === id)).slice(0, 12);
  if (past.length) holder.append(h('h3', {}, 'History'), h('table', { class: 'hist' }, past.map((x) => { const y = x.results.find((z) => z.id === id); return h('tr', { onclick: async () => { holder.remove(); const b = h('div'); box.append(b); await showResult(box, id, x.runId); } }, h('td', {}, x.runId), h('td', {}, badge(y.status)), h('td', {}, secs(y.durationMs))); })));
  box.querySelectorAll('.empty').forEach((e) => e.remove());
  box.append(holder);
}

async function run(ids) {
  const res = await post('/api/run', { only: ids, video: $('#optVideo').checked, fresh: $('#optFresh').checked, headless: $('#optHeadless').checked });
  if (res.error) { log(`! ${res.error}`); return; }
  S.live = {}; ids.forEach((i) => { S.live[i] = 'queued'; });
  $('#logwrap').classList.remove('min'); $('#logtoggle').textContent = 'Hide';
  $('#log').textContent = '';
  render();
}

function renderJob() {
  const j = S.job;
  const busy = j && (j.status === 'running' || j.status === 'stopping');
  const done = j ? Object.values(j.tests || {}).filter((x) => x !== 'running').length : 0;
  $('#jobstate').textContent = !j ? 'idle' : busy ? `${j.status} · ${done}/${j.total || '?'}${j.current ? ' · ' + j.current : ''}` : `last run ${j.status}`;
  $('#stop').disabled = !busy || j.status === 'stopping';
  $('#logsub').textContent = j ? j.runId : '';
}
function render() { renderTags(); renderList(); renderJob(); $('#build').textContent = S.build ? `build ${S.build.git || '?'} · ${S.build.builtAt?.slice(0, 16).replace('T', ' ') || ''}` : 'no build yet: npm run e2e:build'; }

function log(line) {
  const el = $('#log'); const stick = el.scrollTop + el.clientHeight >= el.scrollHeight - 30;
  el.append(document.createTextNode(line + '\n')); if (stick) el.scrollTop = el.scrollHeight;
}

async function load() {
  const st = await api('/api/state');
  Object.assign(S, { tests: st.tests, last: st.last, job: st.job, build: st.build });
  S.runs = (await api('/api/runs')).filter((r) => r.results);
  render();
  if (st.problems?.length) st.problems.forEach((p) => log(`! ${p.file}: ${p.error}`));
}

function connect() {
  const es = new EventSource('/api/events');
  es.addEventListener('job', (e) => { const j = JSON.parse(e.data); const was = S.job?.status; S.job = j; if (j) { for (const [id, s] of Object.entries(j.tests || {})) S.live[id] = s; } renderJob(); renderList(); if (j && !['running', 'stopping'].includes(j.status) && ['running', 'stopping'].includes(was)) { S.live = {}; load().then(() => S.sel && select(S.sel)); } });
  es.addEventListener('backlog', (e) => { $('#log').textContent = JSON.parse(e.data).lines.join('\n') + '\n'; $('#log').scrollTop = 1e9; });
  es.addEventListener('log', (e) => log(JSON.parse(e.data).line));
  es.addEventListener('e2e', (e) => {
    const m = JSON.parse(e.data);
    if (m.type === 'test-start') { S.live[m.id] = 'running'; if (!S.job || S.job.status === 'stopping') return renderList(); S.job.current = m.id; S.job.tests = { ...S.job.tests, [m.id]: 'running' }; }
    if (m.type === 'test-end') { S.live[m.id] = m.status; if (S.job) { S.job.tests = { ...S.job.tests, [m.id]: m.status }; S.job.current = null; } if (S.sel === m.id) setTimeout(() => load().then(() => select(m.id)), 300); }
    if (m.type === 'run-start' && S.job) S.job.total = m.total;
    renderJob(); renderList();
  });
  es.onerror = () => { $('#jobstate').textContent = 'dashboard disconnected'; };
}

$('#q').addEventListener('input', (e) => { S.query = e.target.value; renderList(); });
$('#runShown').addEventListener('click', () => run(shown().map((t) => t.id)));
$('#stop').addEventListener('click', () => post('/api/stop'));
$('#logclear').addEventListener('click', () => { $('#log').textContent = ''; });
$('#logtoggle').addEventListener('click', () => { const w = $('#logwrap'); w.classList.toggle('min'); $('#logtoggle').textContent = w.classList.contains('min') ? 'Show' : 'Hide'; });
$('#theme').addEventListener('click', () => { const d = document.documentElement; const next = (d.dataset.theme || (matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')) === 'dark' ? 'light' : 'dark'; d.dataset.theme = next; store.set('e2e-theme', next); });
if (store.get('e2e-theme')) document.documentElement.dataset.theme = store.get('e2e-theme');
$('#optVideo').checked = store.get('e2e-video', '1') === '1';
$('#optVideo').addEventListener('change', (e) => store.set('e2e-video', e.target.checked ? '1' : '0'));
load().then(connect);
