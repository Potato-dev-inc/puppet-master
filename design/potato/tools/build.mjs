// Rebuilds the whole Potato pack from the rig: node build.mjs   (or: npm run build)
// Flags: --quick  only the base parts, contact sheet and compare image (fast iteration)
import fs from 'fs';
import os from 'os';
import { spawnSync } from 'child_process';
import path from 'path';
import { fileURLToPath } from 'url';
import { Resvg } from '@resvg/resvg-js';
import gifenc from 'gifenc';
const { GIFEncoder } = gifenc;
import { makePose, buildFrame, frameSvg, COLOR, BLACK, STROKE } from './lib/draw.mjs';
import { PARTS, JOINTS, MOUTHS } from './lib/rig.mjs';
import { STATES } from './lib/states.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const QUICK = process.argv.includes('--quick');
// committed set is 128 + 512 (keeps the repo small); --all-sizes also writes 64 and 256
const SIZES = process.argv.includes('--all-sizes') ? [64, 128, 256, 512] : [128, 512];
const FPS = 12;
const nn = (i) => String(i + 1).padStart(2, '0');
const out = (...p) => path.join(ROOT, ...p);
const write = (file, data) => { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, data); };
const rel = (...p) => p.join('/');
const FONTS = { loadSystemFonts: false };
const render = (svg, size) => new Resvg(svg, { fitTo: { mode: 'width', value: size }, font: FONTS }).render();
const png = (svg, size) => render(svg, size).asPng();
const pngText = (svg, size) => new Resvg(svg, { fitTo: { mode: 'width', value: size }, font: { loadSystemFonts: true, defaultFontFamily: 'Arial' } }).render().asPng();
const t0 = Date.now();

// ---------------------------------------------------------------- STEP 1: base parts
function buildBase() {
  const base = buildFrame(makePose({}), 0, 0);
  const head = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">';
  const grp = (inner) => `<g fill="none" stroke="${COLOR}" stroke-width="${STROKE}" stroke-linecap="round" stroke-linejoin="round">${inner}</g>`;
  write(out('base', 'potato-base.svg'), frameSvg(base, { circle: true }) + '\n');
  write(out('base', 'potato-base-transparent.svg'), frameSvg(base) + '\n');
  const rig = { canvas: 512, color: COLOR, black: BLACK, strokeWidth: STROKE, parts: {}, joints: JOINTS, mouths: MOUTHS, note: 'points are canvas px, traced from reference.jpg' };
  for (const l of base.layers) {
    write(out('base', 'parts', `${l.id}.svg`), `${head}${grp(`<g id="${l.id}" data-pivot="${l.pivot.map((n) => n.toFixed(1)).join(',')}">${l.body}</g>`)}</svg>\n`);
    rig.parts[l.id] = { pivot: l.pivot.map((n) => +n.toFixed(1)) };
  }
  for (const k of ['body', 'hair', 'arm-left', 'arm-right', 'staff', 'ring', 'leg-left', 'leg-right'])
    rig.parts[k] = { ...(rig.parts[k] || {}), mode: PARTS[k].mode, closed: PARTS[k].closed, points: PARTS[k].pts.map((p) => p.map((n) => +n.toFixed(1))) };
  write(out('base', 'rig.json'), JSON.stringify(rig, null, 1) + '\n');
}

// ---------------------------------------------------------------- frames
function makeFrames() {
  return STATES.map((s) => {
    const frames = [];
    for (let f = 0; f < s.n; f++) {
      const pose = makePose(s.fn(f, s.n));
      const fr = buildFrame(pose, f % 3, f);      // line boil: 3 wobble variants, desynced per part
      frames.push({
        svg: frameSvg(fr),
        svgC: frameSvg(fr, { circle: true }),
        inner: frameSvg(fr, { ids: false, wrap: false }),
      });
    }
    return { ...s, frames };
  });
}

// ---------------------------------------------------------------- animated SVG (CSS, no JS)
function animatedSvg(st, circle) {
  const n = st.frames.length, dur = (n / FPS).toFixed(4);
  const iter = st.loop ? 'infinite' : '1 forwards';
  let css = '.f{opacity:0}';
  let body = '';
  st.frames.forEach((fr, i) => {
    const a = (i / n) * 100, b = ((i + 1) / n) * 100;
    const last = i === n - 1 && !st.loop;
    let kf;
    if (i === 0) kf = `0%{opacity:1}${b.toFixed(3)}%{opacity:0}100%{opacity:0}`;
    else if (last) kf = `0%{opacity:0}${a.toFixed(3)}%{opacity:1}100%{opacity:1}`;
    else kf = `0%{opacity:0}${a.toFixed(3)}%{opacity:1}${b.toFixed(3)}%{opacity:0}100%{opacity:0}`;
    css += `@keyframes k${i}{${kf}}.f${i}{animation:k${i} ${dur}s steps(1,end) ${iter}}`;
    body += `<g class="f f${i}">${fr.inner}</g>`;
  });
  // the first frame must show before the animation clock starts
  css += '.f0{opacity:1}';
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512"><title>Potato - ${st.id}</title><style>${css}</style>${circle ? `<circle cx="256" cy="256" r="256" fill="${BLACK}"/>` : ''}${body}</svg>\n`;
}

// ---------------------------------------------------------------- sprite sheets
function sheetSvg(st, cell) {
  const cols = Math.min(st.frames.length, 8), rows = Math.ceil(st.frames.length / cols);
  const sc = cell / 512;
  let g = '';
  st.frames.forEach((fr, i) => {
    g += `<g transform="translate(${(i % cols) * cell} ${Math.floor(i / cols) * cell}) scale(${sc})">${fr.inner}</g>`;
  });
  return { svg: `<svg xmlns="http://www.w3.org/2000/svg" width="${cols * cell}" height="${rows * cell}" viewBox="0 0 ${cols * cell} ${rows * cell}">${g}</svg>`, cols, rows, w: cols * cell, h: rows * cell };
}

// ---------------------------------------------------------------- gif
const PAL_N = 32;
const palette = Array.from({ length: PAL_N + 1 }, (_, i) => {
  const t = Math.min(i, PAL_N - 1) / (PAL_N - 1);
  const c0 = [5, 5, 5], c1 = [0xf0, 0xa8, 0x45];
  return i === PAL_N ? [0, 0, 0] : c0.map((v, k) => Math.round(v + (c1[k] - v) * t));
});
function toIndexed(px) {
  const idx = new Uint8Array(px.length / 4);
  const d = [0xf0 - 5, 0xa8 - 5, 0x45 - 5], dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
  for (let i = 0; i < idx.length; i++) {
    const a = px[i * 4 + 3];
    if (a < 128) { idx[i] = PAL_N; continue; }
    const t = ((px[i * 4] - 5) * d[0] + (px[i * 4 + 1] - 5) * d[1] + (px[i * 4 + 2] - 5) * d[2]) / dd;
    idx[i] = Math.round(Math.max(0, Math.min(1, t)) * (PAL_N - 1));
  }
  return idx;
}
function gifFor(st, size = 256) {
  const enc = GIFEncoder();
  st.frames.forEach((fr, i) => {
    const img = render(fr.svgC, size);
    const hold = !st.loop && i === st.frames.length - 1;
    enc.writeFrame(toIndexed(img.pixels), size, size, { palette, delay: hold ? 900 : i % 3 === 2 ? 90 : 80, transparent: true, transparentIndex: PAL_N, repeat: 0 });
  });
  enc.finish();
  return Buffer.from(enc.bytes());
}

// ---------------------------------------------------------------- icons
function icoFile(entries) {
  const n = entries.length, head = Buffer.alloc(6 + 16 * n);
  head.writeUInt16LE(0, 0); head.writeUInt16LE(1, 2); head.writeUInt16LE(n, 4);
  let off = head.length;
  entries.forEach(({ size, data }, i) => {
    const o = 6 + 16 * i;
    head[o] = size >= 256 ? 0 : size; head[o + 1] = size >= 256 ? 0 : size; head[o + 2] = 0; head[o + 3] = 0;
    head.writeUInt16LE(1, o + 4); head.writeUInt16LE(32, o + 6); head.writeUInt32LE(data.length, o + 8); head.writeUInt32LE(off, o + 12);
    off += data.length;
  });
  return Buffer.concat([head, ...entries.map((e) => e.data)]);
}
function buildIcons() {
  const sizes = [16, 32, 48, 64, 128, 256, 512];
  const entries = [];
  for (const s of sizes) {
    // thicker line and eyes on tiny sizes so the doodle still reads
    const b = s <= 16 ? 2.3 : s <= 32 ? 1.75 : s <= 48 ? 1.4 : s <= 64 ? 1.2 : 1;
    const pose = makePose({ strokeScale: b, eyeL: { r: 1 + (b - 1) * 0.9 }, eyeR: { r: 1 + (b - 1) * 0.9 }, mouth: { h: 1 + (b - 1) * 0.35 } });
    const svg = frameSvg(buildFrame(pose, 0, 0), { circle: true, ids: false });
    const data = png(svg, s);
    write(out('icons', `icon-${s}.png`), data);
    if (s <= 256) entries.push({ size: s, data });
  }
  write(out('icons', 'icon.ico'), icoFile(entries));
  write(out('icons', 'icon.svg'), frameSvg(buildFrame(makePose({}), 0, 0), { circle: true }) + '\n');
}

// ---------------------------------------------------------------- contact sheet
function buildContact(all) {
  const cell = 300, cols = 6, pad = 24, labelH = 34;
  const ref = fs.readFileSync(out('reference.jpg')).toString('base64');
  const tiles = [{ label: 'reference.jpg', ref: true }, ...all.map((s) => ({ label: `${s.id}  (${s.n}f${s.loop ? '' : ', once'})`, st: s }))];
  const rows = Math.ceil(tiles.length / cols);
  const W = cols * (cell + pad) + pad, H = rows * (cell + labelH + pad) + pad;
  let g = '';
  tiles.forEach((t, i) => {
    const x = pad + (i % cols) * (cell + pad), y = pad + Math.floor(i / cols) * (cell + labelH + pad);
    if (t.ref) {
      g += `<g transform="translate(${x} ${y})"><circle cx="${cell / 2}" cy="${cell / 2}" r="${cell / 2}" fill="#050505"/><g transform="scale(${cell / 874}) translate(-15 -44)" clip-path="url(#rc)"><clipPath id="rc"><circle cx="452" cy="481" r="437"/></clipPath><image width="954" height="958" href="data:image/jpeg;base64,${ref}"/></g></g>`;
    } else {
      const fr = t.st.frames[t.st.still];
      g += `<g transform="translate(${x} ${y}) scale(${cell / 512})"><circle cx="256" cy="256" r="256" fill="${BLACK}"/>${fr.inner}</g>`;
    }
    g += `<text x="${x + cell / 2}" y="${y + cell + 24}" text-anchor="middle" font-family="Arial" font-size="19" fill="#e8e8e8">${t.label}</text>`;
  });
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><rect width="${W}" height="${H}" fill="#262626"/>${g}</svg>`;
  write(out('contact-sheet.png'), pngText(svg, W));
}


// ---------------------------------------------------------------- animated webp (ffmpeg)
const FFMPEG = process.env.FFMPEG || 'ffmpeg';
function webpFor(st, size = 256) {
  const dir = path.join(os.tmpdir(), 'potato-webp', st.id);
  fs.mkdirSync(dir, { recursive: true });
  const frames = st.frames.map((fr) => png(fr.svg, size));
  const list = frames.slice();
  if (!st.loop) for (let k = 0; k < 10; k++) list.push(frames[frames.length - 1]);   // once: hold the last pose ~0.8s before the loop restarts
  list.forEach((d, i) => fs.writeFileSync(path.join(dir, `${String(i + 1).padStart(3, '0')}.png`), d));
  const dest = out('animated', `${st.id}.webp`);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const r = spawnSync(FFMPEG, ['-y', '-v', 'error', '-framerate', String(FPS), '-i', path.join(dir, '%03d.png'), '-c:v', 'libwebp_anim', '-lossless', '1', '-loop', '0', '-pix_fmt', 'bgra', dest], { encoding: 'utf8' });
  if (r.status !== 0) console.warn(`webp failed for ${st.id}: ${r.stderr || r.error}`);
  for (let i = 1; i <= list.length; i++) fs.unlinkSync(path.join(dir, `${String(i).padStart(3, '0')}.png`));
}

// ---------------------------------------------------------------- preview pages
function previewData() {
  const data = {};
  for (const s of STATES) data[s.id] = { n: s.n, loop: s.loop, desc: s.desc };
  return data;
}
function buildPreview() {
  const tpl = fs.readFileSync(path.join(ROOT, 'tools', 'preview.template.html'), 'utf8');
  write(out('preview.html'), tpl.replace('/*STATES*/{}', JSON.stringify(previewData())).replace('/*INLINE*/{}', '{}'));
}
function buildStandalone(all) {
  const tpl = fs.readFileSync(path.join(ROOT, 'tools', 'preview.template.html'), 'utf8');
  const inline = {};
  for (const st of all) inline[st.id] = animatedSvg(st, false);     // transparent animated SVG, inlined as text
  write(out('preview-standalone.html'), tpl.replace('/*STATES*/{}', JSON.stringify(previewData())).replace('/*INLINE*/{}', JSON.stringify(inline).replace(/</g, '\u003c')));
}

// ================================================================== run
const PHASE = (process.argv.find((a) => a.startsWith('--phase=')) || '').slice(8);   // '' (all) | animated | rest
buildBase();
console.log('base parts written');
const all = makeFrames();
const total = all.reduce((a, s) => a + s.frames.length, 0);
console.log(`${all.length} states, ${total} frames`);
buildContact(all);
console.log('contact sheet written');

if (!QUICK && PHASE !== 'rest') {
  // ---- phase 1: everything that moves
  for (const st of all) {
    write(out('animated', `${st.id}.svg`), animatedSvg(st, false));
    write(out('animated-circle', `${st.id}.svg`), animatedSvg(st, true));
    write(out('gif', `${st.id}.gif`), gifFor(st));
    webpFor(st);
    process.stdout.write(`  ${st.id} `);
  }
  console.log();
  buildPreview();
  buildStandalone(all);
  console.log(`ANIMATED READY: gif/, animated/*.svg, animated/*.webp, preview.html, preview-standalone.html (${((Date.now() - t0) / 1000).toFixed(1)}s)`);
}

if (!QUICK && PHASE !== 'animated') {
  // ---- phase 2: frame exports, sheets, pet pack, manifests, icons
  const manifest = { name: 'Potato', version: 1, fps: FPS, canvas: 512, color: COLOR, black: BLACK, strokeWidth: STROKE,
    layout: { svg: 'svg/<state>/<NN>.svg (transparent)', png: 'png/<size>/<state>/<NN>.png (transparent) and png-circle/<size>/<state>/<NN>.png (on black circle)' }, states: {} };
  const pet = { fps: FPS, size: { width: 256, height: 256 }, states: {} };
  for (const st of all) {
    const files = { svg: [], png: {} };
    for (const sz of SIZES) files.png[sz] = { transparent: [], circle: [] };
    st.frames.forEach((fr, i) => {
      const nm = nn(i);
      write(out('exports', 'svg', st.id, `${nm}.svg`), fr.svg + '\n');
      files.svg.push(rel('svg', st.id, `${nm}.svg`));
      for (const sz of SIZES) {
        write(out('exports', 'png', String(sz), st.id, `${nm}.png`), png(fr.svg, sz));
        write(out('exports', 'png-circle', String(sz), st.id, `${nm}.png`), png(fr.svgC, sz));
        files.png[sz].transparent.push(rel('png', String(sz), st.id, `${nm}.png`));
        files.png[sz].circle.push(rel('png-circle', String(sz), st.id, `${nm}.png`));
      }
      write(out('exports', 'pet', `${st.id}-${nm}.png`), png(fr.svg, 256));    // desktop pet pack
    });
    pet.states[st.id] = st.frames.map((_, i) => `${st.id}-${nn(i)}.png`);
    const sh = sheetSvg(st, 128);
    write(out('exports', 'sheets', `${st.id}.png`), png(sh.svg, sh.w));
    manifest.states[st.id] = {
      fps: FPS, loop: st.loop, mode: st.loop ? 'loop' : 'once', frames: st.frames.length, size: { width: 512, height: 512 }, description: st.desc,
      files, sheet: { file: rel('sheets', `${st.id}.png`), cols: sh.cols, rows: sh.rows, cell: 128 },
      animated: rel('..', 'animated', `${st.id}.svg`), animatedCircle: rel('..', 'animated-circle', `${st.id}.svg`), animatedWebp: rel('..', 'animated', `${st.id}.webp`), gif: rel('..', 'gif', `${st.id}.gif`),
    };
    process.stdout.write(`  ${st.id} `);
  }
  console.log();
  write(out('exports', 'manifest.json'), JSON.stringify(manifest, null, 1) + '\n');
  write(out('exports', 'pet', 'manifest.json'), JSON.stringify(pet) + '\n');
  buildIcons();
}
console.log(`done in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
