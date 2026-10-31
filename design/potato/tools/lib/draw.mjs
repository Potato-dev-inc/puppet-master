// Pose -> geometry -> SVG. All deformation happens on the points (never as an SVG
// transform), so the stroke width stays constant under squash and stretch.
import { COLOR, BLACK, STROKE, PARTS, JOINTS, MOUTH, MOUTHS, EYE_R, FEET, CANVAS } from './rig.mjs';

// ---------- small maths helpers ----------
export const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
export const lerp = (a, b, t) => a + (b - a) * t;
export const rot = (p, c, deg) => {
  if (!deg) return p;
  const a = (deg * Math.PI) / 180, s = Math.sin(a), co = Math.cos(a);
  const x = p[0] - c[0], y = p[1] - c[1];
  return [c[0] + x * co - y * s, c[1] + x * s + y * co];
};
const f1 = (n) => (Math.round(n * 10) / 10).toString();
const fp = (p) => `${f1(p[0])} ${f1(p[1])}`;

function mulberry32(a) {
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// 3 boil variants per part: a stable list of small, slightly correlated offsets.
const noiseCache = new Map();
function noise(seed, variant, n, amp) {
  const key = `${seed}:${variant}:${n}:${amp}`;
  if (noiseCache.has(key)) return noiseCache.get(key);
  const rnd = mulberry32(seed * 7919 + variant * 104729 + 17);
  const raw = Array.from({ length: n + 1 }, () => [(rnd() * 2 - 1), (rnd() * 2 - 1)]);
  const out = [];
  for (let i = 0; i < n; i++) {
    out.push([
      (0.65 * raw[i + 1][0] + 0.35 * raw[i][0]) * amp,
      (0.65 * raw[i + 1][1] + 0.35 * raw[i][1]) * amp,
    ]);
  }
  noiseCache.set(key, out);
  return out;
}
const widthJitter = (seed, variant) => (mulberry32(seed * 31 + variant * 977)() * 2 - 1) * 0.5;

// ---------- paths ----------
export function smoothPath(pts, closed) {
  const n = pts.length;
  const g = (i) => (closed ? pts[((i % n) + n) % n] : pts[clamp(i, 0, n - 1)]);
  let d = `M${fp(pts[0])}`;
  const segs = closed ? n : n - 1;
  for (let i = 0; i < segs; i++) {
    const p0 = g(i - 1), p1 = g(i), p2 = g(i + 1), p3 = g(i + 2);
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${fp(c1)} ${fp(c2)} ${fp(p2)}`;
  }
  return closed ? d + 'Z' : d;
}
export function sharpPath(pts, closed) {
  return 'M' + pts.map(fp).join('L') + (closed ? 'Z' : '');
}
// sample a closed Catmull-Rom spline densely (used for the spinning loading arc)
function sampleClosed(pts, per) {
  const n = pts.length, out = [];
  const g = (i) => pts[((i % n) + n) % n];
  for (let i = 0; i < n; i++) {
    const p0 = g(i - 1), p1 = g(i), p2 = g(i + 1), p3 = g(i + 2);
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    for (let k = 0; k < per; k++) {
      const t = k / per, u = 1 - t;
      out.push([
        u * u * u * p1[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * p2[0],
        u * u * u * p1[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * p2[1],
      ]);
    }
  }
  return out;
}

// ---------- pose ----------
export const DEFAULT_POSE = () => ({
  g: { dx: 0, dy: 0, rot: 0, sx: 1, sy: 1, pivot: FEET },
  hair: { dx: 0, dy: 0, rot: 0, lift: 0, flop: 0 },
  face: { dx: 0, dy: 0 },
  eyeL: { shape: 'dot', open: 1, lid: 0, dx: 0, dy: 0, r: 1, curve: 1 },
  eyeR: { shape: 'dot', open: 1, lid: 0, dx: 0, dy: 0, r: 1, curve: 1 },
  mouth: { ...MOUTHS.open, dx: 0, dy: 0 },
  armL: { rot: 0, hx: 0, hy: 0, ext: 1 },
  armR: { rot: 0, hx: 0, hy: 0 },
  staff: { rot: 0 },
  legL: { rot: 0, fx: 0, fy: 0 },
  legR: { rot: 0, fx: 0, fy: 0 },
  ring: { arc: null },           // {gap, at}: draw an open arc (loading spinner)
  only: null,                    // 'staff' for the loading icon
  strokeScale: 1,
  fx: [],
});

export function mergePose(base, over) {
  for (const k of Object.keys(over || {})) {
    const v = over[k];
    if (v && typeof v === 'object' && !Array.isArray(v) && base[k] && typeof base[k] === 'object' && !Array.isArray(base[k])) {
      mergePose(base[k], v);
    } else base[k] = v;
  }
  return base;
}
export const makePose = (over) => mergePose(DEFAULT_POSE(), over);

// ---------- limbs ----------
function limb(pts, o, pivot) {
  const cum = [0];
  for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]));
  const total = cum[cum.length - 1] || 1;
  const ext = o.ext ?? 1;
  return pts.map((p, i) => {
    const w = cum[i] / total;
    let q = [pivot[0] + (p[0] - pivot[0]) * ext, pivot[1] + (p[1] - pivot[1]) * ext];
    q = [q[0] + (o.hx ?? o.fx ?? 0) * w, q[1] + (o.hy ?? o.fy ?? 0) * w];
    return rot(q, pivot, o.rot || 0);
  });
}

function mouthPoints(m) {
  const { centre, halfW, halfH, norm } = MOUTH;
  const hw = halfW * m.w, hh = halfH * m.h;
  return norm.map(({ nx, ny, a }) => {
    const ex = nx + (Math.cos(a) - nx) * m.round, ey = ny + (Math.sin(a) - ny) * m.round;
    const x = ex * hw, y = ey * hh;
    return [centre[0] + x + (m.dx || 0), centre[1] + y - (m.smile || 0) * (ex * ex) + (m.dy || 0)];
  });
}

function eyePath(cx, cy, e, jit) {
  const r = EYE_R * e.r + jit.r;
  if (e.shape === 'sleep' || e.shape === 'happy') {
    const w = 11 * e.r, c = (e.shape === 'sleep' ? 9 : -9) * (e.curve ?? 1);
    return { stroke: true, d: `M${f1(cx - w)} ${f1(cy - c * 0.12)}Q${f1(cx)} ${f1(cy + c)} ${f1(cx + w)} ${f1(cy - c * 0.12)}` };
  }
  const ry = Math.max(r * e.open, 2.1);
  const cutY = cy - ry + 2 * ry * clamp(e.lid, 0, 0.9);
  const pts = [];
  for (let k = 0; k < 24; k++) {
    const a = (k / 24) * Math.PI * 2;
    pts.push([cx + r * Math.cos(a), Math.max(cy + ry * Math.sin(a), cutY)]);
  }
  return { stroke: false, d: sharpPath(pts, true) };
}

// ---------- the main builder ----------
// Returns {layers:[{id,pivot,body}], ctx} where body is an SVG string for the layer.
export function buildFrame(pose, variant = 0, frame = 0) {
  const p = pose;
  const g = p.g;
  const G = (pt) => {
    let x = g.pivot[0] + (pt[0] - g.pivot[0]) * g.sx;
    let y = g.pivot[1] + (pt[1] - g.pivot[1]) * g.sy;
    const r = rot([x, y], g.pivot, g.rot);
    return [r[0] + g.dx, r[1] + g.dy];
  };
  const phase = { body: 0, hair: 1, 'arm-left': 2, 'arm-right': 1, staff: 2, ring: 0, 'leg-left': 1, 'leg-right': 2, eyes: 2, mouth: 1 };
  const v = (name) => (variant + (phase[name] ?? 0)) % 3;
  const noisy = (name, pts, amp) => {
    const nz = noise(PARTS[name]?.seed ?? 5, v(name), pts.length, amp);
    return pts.map((q, i) => [q[0] + nz[i][0], q[1] + nz[i][1]]);
  };
  const sw = (name) => ((STROKE + widthJitter(PARTS[name]?.seed ?? 5, v(name))) * p.strokeScale).toFixed(2);
  const strokePath = (name, d, extra = '') => `<path d="${d}" stroke-width="${sw(name)}"${extra}/>`;
  const layers = [];
  const add = (id, pivot, body) => layers.push({ id, pivot, body });

  const shoulderR = JOINTS.shoulderR;
  // staff chain: spin about the grip, follow the hand, swing with the arm, then global
  const staffX = (pt) => {
    let q = rot(pt, JOINTS.grip, p.staff.rot - p.armR.rot);   // staff.rot is the final tilt of the staff
    q = [q[0] + p.armR.hx, q[1] + p.armR.hy];
    q = rot(q, shoulderR, p.armR.rot);
    return G(q);
  };

  // ---- anchors for fx (un-noised) ----
  const ringPts0 = PARTS.ring.pts;
  const rc = ringPts0.reduce((a, q) => [a[0] + q[0] / ringPts0.length, a[1] + q[1] / ringPts0.length], [0, 0]);
  const ctx = {
    ring: staffX(rc),
    tip: staffX(JOINTS.staffTip),
    grip: staffX(JOINTS.grip),
    head: G(JOINTS.hairBase),
    mouth: G(JOINTS.mouth),
    handL: G(limb(PARTS['arm-left'].pts, p.armL, JOINTS.shoulderL).slice(-1)[0]),
    ringR: JOINTS.ringRadius * p.g.sx,
  };

  if (!p.only) {
    // legs (behind everything)
    const legs = [['leg-left', p.legL, JOINTS.hipL], ['leg-right', p.legR, JOINTS.hipR]];
    for (const [name, o, hip] of legs) {
      const pts = limb(noisy(name, PARTS[name].pts, PARTS[name].amp), o, hip).map(G);
      add(name, hip, strokePath(name, smoothPath(pts, false)));
    }
    // left arm
    {
      const pts = limb(noisy('arm-left', PARTS['arm-left'].pts, PARTS['arm-left'].amp), p.armL, JOINTS.shoulderL).map(G);
      add('arm-left', JOINTS.shoulderL, strokePath('arm-left', smoothPath(pts, false)));
    }
    // body
    {
      const pts = noisy('body', PARTS.body.pts, PARTS.body.amp).map(G);
      add('body', FEET, strokePath('body', smoothPath(pts, false)));
    }
    // hair
    {
      const H = PARTS.hair, piv = H.pivot;
      let pts = noisy('hair', H.pts, H.amp).map((q, i) => {
        const w = H.weight[i];
        const h = Math.max(0, piv[1] - q[1]);
        let x = q[0], y = q[1];
        y -= p.hair.lift * w * 16;
        x += (q[0] - piv[0]) * p.hair.lift * w * 0.05;
        x += p.hair.flop * h * 0.9;
        y += p.hair.flop * h * 0.7;
        return [x, y];
      });
      pts = pts.map((q) => {
        let r = rot(q, piv, p.hair.rot);
        r = [r[0] + p.hair.dx, r[1] + p.hair.dy];
        return G(r);
      });
      add('hair', piv, strokePath('hair', sharpPath(pts, false)));
    }
    // eyes
    {
      const nz = noise(97, v('eyes'), 4, 0.7);
      let body = '';
      [[JOINTS.eyeL, p.eyeL, 0], [JOINTS.eyeR, p.eyeR, 2]].forEach(([c, e, k]) => {
        const cx0 = c[0] + p.face.dx + e.dx + nz[k][0], cy0 = c[1] + p.face.dy + e.dy + nz[k][1];
        const [cx, cy] = G([cx0, cy0]);
        const ep = eyePath(cx, cy, e, { r: nz[k + 1][0] * 0.5 });
        body += ep.stroke
          ? `<path d="${ep.d}" fill="none" stroke-width="${(8.5 * p.strokeScale).toFixed(1)}"/>`
          : `<path d="${ep.d}" fill="${COLOR}" stroke-width="1.6"/>`;
      });
      add('eyes', JOINTS.eyeL, body);
    }
    // mouth
    {
      const m = { ...p.mouth, dx: (p.mouth.dx || 0) + p.face.dx, dy: (p.mouth.dy || 0) + p.face.dy };
      const nz = noise(131, v('mouth'), 16, 0.8);
      const pts = mouthPoints(m).map((q, i) => G([q[0] + nz[i][0], q[1] + nz[i][1]]));
      add('mouth', JOINTS.mouth, `<path d="${smoothPath(pts, true)}" fill="${COLOR}" stroke-width="6"/>`);
    }
    // right arm
    {
      const pts = limb(noisy('arm-right', PARTS['arm-right'].pts, PARTS['arm-right'].amp), p.armR, shoulderR).map(G);
      add('arm-right', shoulderR, strokePath('arm-right', smoothPath(pts, false)));
    }
  }

  // staff = shaft + ring
  {
    const shaft = noisy('staff', PARTS.staff.pts, PARTS.staff.amp).map(staffX);
    let body = strokePath('staff', smoothPath(shaft, false));
    const rp = noisy('ring', PARTS.ring.pts, PARTS.ring.amp);
    if (p.ring.arc) {
      const dense = sampleClosed(rp, 8);
      const M = dense.length;
      const len = Math.round(((360 - p.ring.arc.gap) / 360) * M);
      const s0 = Math.round(((((p.ring.arc.at % 360) + 360) % 360) / 360) * M);
      const arc = [];
      for (let i = 0; i < len; i++) arc.push(staffX(dense[(s0 + i) % M]));
      body += strokePath('ring', 'M' + arc.map(fp).join('L'));
    } else {
      body += strokePath('ring', smoothPath(rp.map(staffX), true));
    }
    add('staff', JOINTS.grip, body);
  }

  // fx (sparkles, "!", zzz, dots ...). Items may anchor to the ring/tip/head/mouth.
  if (p.fx.length) add('fx', JOINTS.hairBase, drawFx(p.fx, ctx, p));

  return { layers, ctx };
}

// ---------- effects ----------
function drawFx(items, ctx, pose) {
  let out = '';
  for (const it of items) {
    let ax = 0, ay = 0;
    if (it.at) [ax, ay] = ctx[it.at];
    const x = ax + (it.x || 0), y = ay + (it.y || 0);
    const op = it.op ?? 1;
    const o = op < 1 ? ` opacity="${op.toFixed(2)}"` : '';
    const w = (it.w ?? 7) * pose.strokeScale;
    if (it.type === 'spark') {          // a doodled plus / star
      const s = it.s ?? 10, a = it.rot || 0;
      const P = (dx, dy) => rot([x + dx, y + dy], [x, y], a);
      const l = (q, r) => `M${fp(q)}L${fp(r)}`;
      let d = l(P(-s, 0), P(s, 0)) + l(P(0, -s), P(0, s));
      if (it.star) d += l(P(-s * 0.55, -s * 0.55), P(s * 0.55, s * 0.55)) + l(P(-s * 0.55, s * 0.55), P(s * 0.55, -s * 0.55));
      out += `<path d="${d}" stroke-width="${w}"${o}/>`;
    } else if (it.type === 'ticks') {   // short rays around a point
      const n = it.n ?? 8, r0 = it.r0 ?? 30, r1 = it.r1 ?? 42;
      let d = '';
      for (let k = 0; k < n; k++) {
        const a = ((k / n) * 360 + (it.rot || 0)) * Math.PI / 180;
        d += `M${fp([x + Math.cos(a) * r0, y + Math.sin(a) * r0])}L${fp([x + Math.cos(a) * r1, y + Math.sin(a) * r1])}`;
      }
      out += `<path d="${d}" stroke-width="${w}"${o}/>`;
    } else if (it.type === 'circle') {  // outline echo ring
      out += `<circle cx="${f1(x)}" cy="${f1(y)}" r="${f1(it.r ?? 30)}" fill="none" stroke-width="${w}"${o}/>`;
    } else if (it.type === 'dot') {
      out += `<circle cx="${f1(x)}" cy="${f1(y)}" r="${f1(it.r ?? 6.5)}" fill="${COLOR}" stroke="none"${o}/>`;
    } else if (it.type === 'bang') {    // the "!" : a thick stick and a dot
      const s = it.s ?? 1, tilt = it.tilt ?? 2;
      out += `<path d="M${f1(x)} ${f1(y)}Q${f1(x + tilt * 0.2)} ${f1(y + 24 * s)} ${f1(x + tilt)} ${f1(y + 46 * s)}" stroke-width="${f1(14 * Math.min(1.1, 0.6 + s * 0.4))}"${o}/>`;
      out += `<circle cx="${f1(x + tilt * 1.1)}" cy="${f1(y + 70 * s)}" r="${f1(8.5 * Math.min(1.1, 0.6 + s * 0.4))}" fill="${COLOR}" stroke="none"${o}/>`;
    } else if (it.type === 'z') {       // a hand-drawn Z
      const s = it.s ?? 12;
      out += `<path d="M${fp([x - s, y - s])}L${fp([x + s, y - s * 0.9])}L${fp([x - s, y + s])}L${fp([x + s, y + s * 0.95])}" stroke-width="${f1(w * 1.15)}"${o}/>`;
    } else if (it.type === 'line') {
      out += `<path d="M${fp([x, y])}L${fp([x + it.dx, y + it.dy])}" stroke-width="${w}"${o}/>`;
    }
  }
  return out;
}

// ---------- SVG output ----------
export function frameSvg(frame, { circle = false, ids = true, size = CANVAS, wrap = true, strokeScale = 1 } = {}) {
  const inner = frame.layers
    .map((l) => (ids ? `<g id="${l.id}" data-pivot="${f1(l.pivot[0])},${f1(l.pivot[1])}">${l.body}</g>` : `<g>${l.body}</g>`))
    .join('');
  const sw = (STROKE * strokeScale).toFixed(2);
  const group = `<g${ids ? ' id="potato"' : ''} fill="none" stroke="${COLOR}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${inner}</g>`;
  if (!wrap) return group;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${CANVAS} ${CANVAS}" width="${size}" height="${size}">${circle ? `<circle${ids ? ' id="bg"' : ''} cx="256" cy="256" r="256" fill="${BLACK}"/>` : ''}${group}</svg>`;
}
export { COLOR, BLACK, CANVAS, STROKE };
