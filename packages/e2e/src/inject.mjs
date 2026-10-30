// Page-side overlay for the video: a visible cursor, click ripples, key caps and a step label. It only ADDS fixed,
// pointer-events:none elements in one container (#__pmE2e) and never touches the app's own DOM. A tiny blinking
// dot keeps the compositor producing frames while the page is idle, so the screencast has a heartbeat.
export function overlaySource({ testId = '', cursor = true } = {}) {
  return `(()=>{
  if (window.__pmE2e) { window.__pmE2e.remove(); }
  const Z = 2147483646, root = document.createElement('div');
  root.id = '__pmE2e'; root.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:' + Z;
  document.documentElement.appendChild(root);
  const st = document.createElement('style');
  st.textContent = \`
   #__pmE2eCur{position:fixed;left:0;top:0;width:24px;height:24px;margin:-3px 0 0 -4px;z-index:\${Z+1};transition:transform .3s cubic-bezier(.22,.8,.24,1),opacity .25s;opacity:0}
   #__pmE2eCur svg{filter:drop-shadow(0 1px 3px rgba(0,0,0,.75))}
   .__pmRip{position:fixed;width:16px;height:16px;margin:-8px 0 0 -8px;border-radius:50%;border:2px solid rgba(255,255,255,.95);box-shadow:0 0 0 1px rgba(0,0,0,.5);animation:__pmRip .5s ease-out forwards}
   @keyframes __pmRip{to{transform:scale(3.2);opacity:0}}
   .__pmKey{position:fixed;left:50%;bottom:30px;transform:translateX(-50%);padding:7px 14px;border-radius:10px;background:rgba(20,20,22,.94);border:1px solid rgba(255,255,255,.22);color:#f5f5f7;font:600 16px/1.2 'Segoe UI',system-ui,sans-serif;letter-spacing:.02em;box-shadow:0 8px 28px rgba(0,0,0,.6);transition:opacity .25s}
   #__pmE2eStep{position:fixed;left:12px;bottom:10px;max-width:60vw;padding:4px 10px;border-radius:7px;background:rgba(17,17,19,.86);border:1px solid rgba(255,255,255,.16);color:#d8d8dc;font:500 12px/1.3 ui-monospace,Consolas,monospace;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;opacity:0;transition:opacity .2s}
   #__pmE2eRec{position:fixed;right:10px;bottom:10px;display:flex;gap:6px;align-items:center;padding:3px 8px;border-radius:7px;background:rgba(17,17,19,.7);color:#9a9aa2;font:500 11px/1 ui-monospace,Consolas,monospace}
   #__pmE2eRec i{width:7px;height:7px;border-radius:50%;background:#e8e8ea;animation:__pmBlink 1s steps(2,jump-none) infinite}
   @keyframes __pmBlink{from{opacity:1}to{opacity:.25}}\`;
  root.appendChild(st);
  const cur = document.createElement('div'); cur.id = '__pmE2eCur';
  cur.innerHTML = '<svg width="24" height="24" viewBox="0 0 22 22"><path d="M4 2l13 8.5-5.6 1.3 3.2 6-2.4 1.2-3.2-6L4 17z" fill="#fff" stroke="#000" stroke-width="1.2" stroke-linejoin="round"/></svg>';
  if (${JSON.stringify(cursor)}) root.appendChild(cur);
  const stepEl = document.createElement('div'); stepEl.id = '__pmE2eStep'; root.appendChild(stepEl);
  const rec = document.createElement('div'); rec.id = '__pmE2eRec'; rec.innerHTML = '<i></i><span></span>'; rec.lastChild.textContent = ${JSON.stringify(testId)}; root.appendChild(rec);
  window.__pmE2e = {
    cursor(x, y) { cur.style.opacity = 1; cur.style.transform = 'translate(' + x + 'px,' + y + 'px)'; },
    ripple(x, y) { const r = document.createElement('div'); r.className = '__pmRip'; r.style.left = x + 'px'; r.style.top = y + 'px'; root.appendChild(r); setTimeout(() => r.remove(), 650); },
    keycap(t, ms = 1100) { const k = document.createElement('div'); k.className = '__pmKey'; k.textContent = t; root.appendChild(k); setTimeout(() => { k.style.opacity = 0; setTimeout(() => k.remove(), 300); }, ms); },
    step(name) { stepEl.textContent = name; stepEl.style.opacity = name ? 1 : 0; },
    remove() { root.remove(); delete window.__pmE2e; },
  };
  return 'ok';
})()`;
}
