// The animation states. Every state is a function frame -> pose overrides (see draw.mjs
// DEFAULT_POSE). 12 fps. Squash and stretch happens on the points about the feet (or the
// head, when dragged), so the line weight never changes and the character stays on-model.
import { MOUTHS, JOINTS, R, FEET } from './rig.mjs';
import { lerp, clamp } from './draw.mjs';

const TAU = Math.PI * 2;
const sin = Math.sin, cos = Math.cos;
const sm = (t) => { t = clamp(t, 0, 1); return t * t * (3 - 2 * t); };       // smoothstep
const bump = (t) => sin(Math.PI * clamp(t, 0, 1));                           // 0 -> 1 -> 0
const mouth = (name, extra = {}) => ({ ...MOUTHS[name], ...extra });
const mix = (a, b, t) => {
  const A = typeof a === 'string' ? MOUTHS[a] : a, B = typeof b === 'string' ? MOUTHS[b] : b;
  const o = {};
  for (const k of Object.keys(A)) o[k] = lerp(A[k], B[k], t);
  return o;
};
const SQUASH = (k) => ({ sx: 1 + k * 0.6, sy: 1 - k });           // k>0 squash, k<0 stretch
const lockFeet = (dy) => ({ legL: { fy: -dy }, legR: { fy: -dy } }); // body bobs, feet stay put

// particles: a burst of small sparkles from an anchor
function sparkBurst(f, { at, start, count, life, speed, size, from = -170, to = -10, star = false, gravity = 0, x = 0, y = 0, w = 6 }) {
  const out = [];
  for (let k = 0; k < count; k++) {
    const age = f - start - (k % 3);
    if (age < 0 || age >= life) continue;
    const a = (lerp(from, to, (k + 0.5) / count) + ((k * 53) % 17) - 8) * Math.PI / 180;
    const d = speed * (0.55 + ((k * 37) % 10) / 14) * Math.sqrt(age + 1) * 1.15;
    const u = age / life;
    out.push({
      type: 'spark', at, x: x + cos(a) * d, y: y + sin(a) * d + gravity * age * age,
      s: size * (1 - u * 0.5) * (0.8 + ((k * 7) % 5) / 10), rot: (k % 2) * 45 + age * 14, op: 1 - u * u, star: star && k % 2 === 0, w,
    });
  }
  return out;
}

const HEAD = (dx, dy) => ({ at: 'head', x: dx, y: dy });

// -------------------------------------------------------------------------------------
export const STATES = [
  {
    id: 'idle', n: 24, loop: true, still: 6, desc: 'gentle bob and breathing',
    fn(f, n) {
      const t = f / n, b = (1 - cos(TAU * t)) / 2;
      const dy = -3 * b;
      return {
        g: { dy, sy: 1 + 0.012 * b, sx: 1 - 0.006 * b }, ...lockFeet(dy),
        hair: { dy: 1.6 * sin(TAU * (t - 0.12)), flop: 0.035 * sin(TAU * (t - 0.2)) },
        armL: { rot: 3 * sin(TAU * t + 1.6) }, armR: { rot: 1.2 * sin(TAU * t + 0.4) }, staff: { rot: 2.4 * sin(TAU * t + 0.8) },
      };
    },
  },
  {
    id: 'blink', n: 6, loop: false, still: 2, desc: 'one quick blink',
    fn(f) {
      const o = [1, 0.62, 0.1, 0.1, 0.62, 1][f];
      return { eyeL: { open: o }, eyeR: { open: o }, g: { dy: f === 2 || f === 3 ? 0.8 : 0 }, hair: { dy: f === 2 || f === 3 ? 0.8 : 0 } };
    },
  },
  {
    id: 'talk', n: 12, loop: true, still: 3, desc: 'mouth shapes: closed / small / open / wide',
    fn(f, n) {
      const t = f / n;
      const seq = ['closed', 'small', 'open', 'wide', 'open', 'small', 'wide', 'open', 'small', 'closed', 'open', 'small'];
      const name = seq[f % seq.length];
      const bob = name === 'wide' ? -2.4 : name === 'open' ? -1.2 : 0;
      return {
        mouth: mouth(name), g: { dy: bob, rot: 1.2 * sin(TAU * t) }, ...lockFeet(bob),
        hair: { dy: -bob * 0.6, flop: 0.04 * sin(TAU * t) },
        staff: { rot: 4 * sin(TAU * t) }, armR: { rot: 2 * sin(TAU * t + 0.5) }, armL: { rot: 3 * sin(TAU * t + 2) },
      };
    },
  },
  {
    id: 'listen', n: 18, loop: true, still: 4, desc: 'leans in, eyes look to the side',
    fn(f, n) {
      const t = f / n, s = sin(TAU * t);
      const dart = f >= 9 && f <= 11 ? -2.6 : 0;
      return {
        g: { rot: -4.5 + 1.2 * s, dx: -5, dy: -1.2 * (1 - cos(TAU * 2 * t)) / 2, sx: 1.015, sy: 0.99 },
        face: { dx: -9, dy: 1 }, eyeL: { dx: -4.5, dy: dart, r: 1.08 }, eyeR: { dx: -4.5, dy: dart, r: 1.08 },
        mouth: mouth('small', { smile: 4, dx: -2 }),
        hair: { dx: -2, rot: -2 + 1.2 * s, flop: -0.05 - 0.03 * s },
        armL: { rot: -6 + 3 * s }, armR: { rot: 5 }, staff: { rot: 6 + 2 * sin(TAU * t + 1) },
        legL: { rot: 3 }, legR: { rot: -2 },
      };
    },
  },
  {
    id: 'think', n: 24, loop: true, still: 9, desc: 'staff ring glows and orbits, ... dots',
    fn(f, n) {
      const t = f / n, glow = 0.5 + 0.5 * sin(TAU * 2 * t);
      const dots = Math.floor(t * 4); // 0..3 dots in turn
      const ringR = JOINTS.ringRadius + 8;
      const fx = [
        { type: 'ticks', at: 'ring', n: 8, r0: ringR + 6, r1: ringR + 6 + 6 + 9 * glow, rot: t * 90, op: 0.45 + 0.55 * glow, w: 6 },
        { type: 'circle', at: 'ring', r: ringR + 4, w: 4, op: 0.2 + 0.5 * glow },
      ];
      for (let k = 0; k < 3; k++) {
        const a = -TAU * t * 1 - k * 0.5 + 0.8;
        fx.push({ type: 'dot', at: 'ring', x: cos(a) * (ringR + 22), y: sin(a) * (ringR + 22), r: 6 - k * 1.6, op: 1 - k * 0.3 });
      }
      for (let k = 0; k < Math.min(3, dots); k++) fx.push({ ...HEAD(112 + k * 26, -52 - k * 10), type: 'dot', r: 6.5 });
      return {
        g: { rot: 2 + 0.8 * sin(TAU * t), dy: -1 * (1 - cos(TAU * t)) / 2 },
        face: { dx: 6, dy: -3 }, eyeL: { dx: 3, dy: -3.5 }, eyeR: { dx: 3, dy: -3.5 },
        mouth: mix('small', 'flat', 0.5 + 0.4 * sin(TAU * t)), hair: { rot: 1.5, dy: 1 * sin(TAU * t) },
        armR: { rot: -14 + 2 * sin(TAU * 2 * t), hx: -4, hy: -6 }, staff: { rot: -7 + 4 * sin(TAU * t) },
        armL: { rot: 22 + 3 * sin(TAU * t), ext: 0.92 },
        fx,
      };
    },
  },
  {
    id: 'working', n: 24, loop: true, still: 5, desc: 'taps the staff like typing, sparks',
    fn(f, n) {
      const t = f / n;
      const ph = (f % 4) / 4, tap = 1 - Math.abs(2 * ph - 1);               // 6 taps per loop
      const lph = ((f + 1) % 6) / 6, ltap = 1 - Math.abs(2 * lph - 1);        // 4 left-arm taps
      const down = ph >= 0.5;
      const fx = [];
      if (f % 4 === 2) fx.push(...sparkBurst(0, { at: 'tip', start: 0, count: 4, life: 3, speed: 5, size: 7, from: -160, to: -20, y: -2 }));
      if (f % 4 === 3) fx.push(...sparkBurst(1, { at: 'tip', start: 0, count: 4, life: 3, speed: 5, size: 7, from: -160, to: -20, y: -2 }));
      if (f % 4 === 0) fx.push(...sparkBurst(2, { at: 'tip', start: 0, count: 4, life: 3, speed: 5, size: 7, from: -160, to: -20, y: -2 }));
      const dy = 1.6 * tap;
      return {
        g: { rot: 2.5, dy, sy: 1 - 0.01 * tap }, ...lockFeet(dy),
        face: { dx: 2, dy: 3 }, eyeL: { dy: 3, dx: 1 }, eyeR: { dy: 3, dx: 1 },
        mouth: mouth('small', { w: 0.5, h: 0.45 + 0.12 * (f % 2), smile: 1 }),
        hair: { dy: 1.2 * tap, flop: 0.06 },
        armR: { hy: 16 * tap, hx: 1 }, staff: { rot: down ? 1.2 : -1.2 },
        armL: { rot: 8 + 10 * ltap },
        fx,
      };
    },
  },
  {
    id: 'happy', n: 12, loop: true, still: 6, desc: 'bouncing with joy',
    fn(f, n) {
      const t = f / n, hop = bump(t * 1), air = Math.abs(sin(Math.PI * t));
      const land = 1 - sm(air * 3);                               // 1 when on the ground
      const dy = -26 * air;
      const sq = 0.07 * land - 0.05 * air * (1 - land);
      return {
        g: { dy, ...SQUASH(sq) }, ...{ legL: { rot: 14 * air - 6 * land }, legR: { rot: -14 * air + 6 * land } },
        mouth: mouth('wide', { h: 1.2 + 0.25 * air }),
        eyeL: { r: 1.12 }, eyeR: { r: 1.12 },
        hair: { dy: 4 * land - 5 * air, flop: -0.12 * sin(TAU * t) },
        armL: { rot: 55 + 35 * air }, armR: { rot: 0, hy: -12 * air - 2, hx: 2 }, staff: { rot: -5 + 8 * sin(TAU * t + 1.2) },
      };
    },
  },
  {
    id: 'celebrate', n: 24, loop: false, still: 9, desc: 'jump with sparkles bursting from the staff',
    fn(f, n) {
      // 0-3 crouch, 4-11 jump + peak, 12-14 land (squash), 15-23 settle
      let dy = 0, sq = 0, arms = 0, sx = 1;
      if (f < 4) { const u = f / 3; sq = 0.14 * sm(u); sx = 1 + sq * 0.7; arms = -0.3 * u; }
      else if (f < 12) { const u = (f - 4) / 7; dy = -62 * sin(Math.PI * u); sq = -0.1 * bump(u); sx = 1 + sq * 0.5; arms = 1; }
      else if (f < 15) { const u = (f - 12) / 2; sq = 0.17 * bump(u) + 0.02; sx = 1 + sq * 0.7; arms = 0.6; }
      else { const u = (f - 15) / 8; sq = -0.03 * bump(u * 2); sx = 1 - sq * 0.5; arms = 0.6 * (1 - sm(u)); dy = -4 * bump(u * 1.5); }
      const air = dy < -4;
      const fx = [];
      if (f >= 6) fx.push(...sparkBurst(f, { at: 'ring', start: 6, count: 8, life: 10, speed: 21, size: 13, from: -215, to: -25, star: true, gravity: 0.5, w: 5 }));
      if (f >= 10) fx.push(...sparkBurst(f, { at: 'ring', start: 10, count: 8, life: 10, speed: 28, size: 9, from: -225, to: -15, gravity: 0.7, w: 5 }));
      if (f >= 13 && f < 22) fx.push(...sparkBurst(f, { at: 'head', start: 13, count: 7, life: 9, speed: 22, size: 8, from: -200, to: -20, y: -30, gravity: 0.7, w: 5 }));
      const wob = sin(f * 1.6);
      return {
        g: { dy, ...SQUASH(sq), sx }, ...(air ? {} : lockFeet(dy)),
        legL: { rot: air ? 18 : 0 }, legR: { rot: air ? -18 : 0 },
        mouth: mouth(f < 4 ? 'small' : 'wide', { h: f < 4 ? 0.5 : 1.35 }),
        eyeL: { r: 1.12 }, eyeR: { r: 1.12 },
        hair: { dy: -3 * arms + (air ? 5 : 0), flop: air ? 0.12 : 0 },
        armL: { rot: 105 * arms + (arms > 0.5 ? 6 * wob : 0) }, armR: { rot: 0, hy: -15 * arms, hx: 2 * arms },
        staff: { rot: arms > 0.5 ? -6 + 7 * wob : 0 },
        fx,
      };
    },
  },
  {
    id: 'wave-hello', n: 24, loop: true, still: 3, desc: 'waves the staff hello',
    fn(f, n) {
      const t = f / n, w = sin(TAU * 2 * t), b = (1 - cos(TAU * t)) / 2;
      const dy = -2 * b;
      return {
        g: { dy, rot: -1 + 1.2 * sin(TAU * t) }, ...lockFeet(dy),
        mouth: mouth('open'), eyeL: { r: 1.05 }, eyeR: { r: 1.05 },
        hair: { dy: 1.5 * sin(TAU * (t - 0.1)), flop: 0.04 * w },
        armR: { rot: 0, hx: 4 * w, hy: -12 }, staff: { rot: -6 + 9 * w },
        armL: { rot: 4 * sin(TAU * t + 1) },
      };
    },
  },
  {
    id: 'needs-you', n: 24, loop: true, still: 4, desc: 'waves the staff, orange "!" above',
    fn(f, n) {
      const t = f / n, w = sin(TAU * 3 * t), hop = Math.abs(sin(TAU * 1.5 * t));
      const dy = -7 * hop;
      const bs = 1 + 0.1 * bump((t * 3) % 1);
      return {
        g: { dy, rot: 1.5 * sin(TAU * 1.5 * t), sy: 1 + 0.02 * hop }, ...lockFeet(dy),
        mouth: mouth('open', { h: 1.15, w: 0.95 }), eyeL: { r: 1.15 }, eyeR: { r: 1.15 },
        hair: { dy: 3 * hop, flop: 0.08 * w, lift: 0.25 },
        armR: { rot: 0, hx: 4 * w, hy: -13 }, staff: { rot: -6 + 10 * w },
        armL: { rot: 30 + 12 * sin(TAU * 3 * t + 1.5) },
        fx: [{ type: 'bang', at: 'head', x: -128, y: -92 - 8 * hop, s: bs, tilt: 3 * sin(TAU * 3 * t) }],
      };
    },
  },
  {
    id: 'surprised', n: 12, loop: false, still: 4, desc: 'hair spikes jump up, O mouth',
    fn(f) {
      const k = [0, -0.05, -0.12, -0.1, -0.04, 0.05, 0.02, 0, 0, 0, 0, 0][f];       // squash(+)/stretch(-)
      const dy = [0, 3, -20, -26, -14, -2, 0, 0, 0, 0, 0, 0][f];
      const lift = [0, 0, 0.7, 1.15, 1.1, 1, 1, 1, 1, 1, 1, 1][f];
      const tremble = f >= 6 ? 1.4 * sin(f * 2.6) : 0;
      const on = f >= 1 ? Math.min(1, f / 2) : 0;
      const fx = [];
      if (f >= 2 && f <= 5) {
        const u = (f - 2) / 3, a = 14 + u * 8;
        fx.push({ type: 'line', at: 'head', x: -48 - a * 0.5, y: -74 - a * 0.4, dx: -10, dy: -12, w: 7, op: 1 - u * 0.5 });
        fx.push({ type: 'line', at: 'head', x: 0, y: -108 - a * 0.5, dx: 0, dy: -16, w: 7, op: 1 - u * 0.5 });
        fx.push({ type: 'line', at: 'head', x: 130 + a * 0.2, y: -62 - a * 0.4, dx: 11, dy: -11, w: 7, op: 1 - u * 0.5 });
      }
      return {
        g: { dy, ...SQUASH(k), dx: tremble }, ...(dy < -1 ? {} : lockFeet(dy)),
        legL: { rot: dy < -10 ? 14 : 0 }, legR: { rot: dy < -10 ? -14 : 0 },
        mouth: f === 0 ? mouth('open') : mix('open', 'O', on), eyeL: { r: 1 + 0.45 * on }, eyeR: { r: 1 + 0.45 * on },
        hair: { lift, dy: -2 * on },
        armL: { rot: 38 * on + tremble * 2 }, armR: { rot: -22 * on - tremble * 2, hy: -2 * on }, staff: { rot: 8 * on * sin(f * 2.1) },
        fx,
      };
    },
  },
  {
    id: 'sad', n: 24, loop: true, still: 6, desc: 'droops, the hair flops, a down mouth',
    fn(f, n) {
      const t = f / n, sigh = (1 - cos(TAU * t)) / 2;
      const blink = f === 15 ? 0.35 : f === 16 ? 0.12 : f === 17 ? 0.5 : 1;
      return {
        g: { sy: 0.92 + 0.012 * sigh, sx: 1.045 - 0.006 * sigh, rot: 2.2 + 0.6 * sin(TAU * t) },
        face: { dy: 5 }, eyeL: { dy: 3, open: blink, r: 0.95 }, eyeR: { dy: 3, open: blink, r: 0.95 },
        mouth: mix('down', mouth('down', { w: 0.52, smile: -16 }), sigh),
        hair: { flop: 0.6 + 0.05 * sin(TAU * t), dy: 4 },
        armL: { rot: -26 + 2 * sin(TAU * t + 1), ext: 0.95 }, armR: { rot: 6, hy: 14 }, staff: { rot: -3 + 2 * sin(TAU * t) },
        legL: { rot: 5 }, legR: { rot: -5 },
      };
    },
  },
  {
    id: 'error', n: 24, loop: true, still: 3, desc: 'droop plus a nervous shake, flat mouth',
    fn(f, n) {
      const t = f / n;
      const shaking = f < 14;
      const sh = shaking ? 3.2 * sin(f * 3.4) * (1 - f / 18) : 0;
      const fx = [];
      if (shaking && f % 2 === 0) {
        fx.push({ type: 'line', at: 'head', x: -170, y: 30, dx: -16, dy: -6, w: 6, op: 0.9 });
        fx.push({ type: 'line', at: 'head', x: -166, y: 62, dx: -16, dy: 4, w: 6, op: 0.9 });
        fx.push({ type: 'line', at: 'head', x: 178, y: 20, dx: 16, dy: -6, w: 6, op: 0.9 });
        fx.push({ type: 'line', at: 'head', x: 174, y: 52, dx: 16, dy: 4, w: 6, op: 0.9 });
      }
      return {
        g: { sy: 0.93, sx: 1.04, dx: sh, rot: 1.5 + 0.5 * sin(TAU * t) },
        face: { dy: 4 }, eyeL: { dy: 2, r: 1.05 }, eyeR: { dy: 2, r: 1.05 },
        mouth: mouth('flat', { w: 0.7 }),
        hair: { flop: 0.5 + 0.06 * sin(f * 2.2), dy: 3, lift: 0 },
        armL: { rot: -22 }, armR: { rot: 6, hy: 10 }, staff: { rot: -5 + (shaking ? 4 * sin(f * 3.4) : 0) },
        legL: { rot: 4 }, legR: { rot: -4 },
        fx,
      };
    },
  },
  {
    id: 'sleepy', n: 24, loop: true, still: 14, desc: 'half eyes, a big yawn',
    fn(f, n) {
      const t = f / n;
      const yawn = f >= 8 && f <= 18 ? bump((f - 8) / 10) : 0;
      const lid = 0.5 + 0.25 * sin(TAU * t * 2) * (1 - yawn);
      const squeeze = yawn > 0.35;
      const eyeS = squeeze ? { shape: 'sleep', curve: 0.7 } : { lid };
      const nod = 2 * sin(TAU * t * 2);
      return {
        g: { rot: 1.5 * sin(TAU * t) + 1, sy: 1 + 0.035 * yawn, sx: 1 - 0.015 * yawn, dy: 0.8 * nod },
        face: { dy: 2 }, eyeL: { ...eyeS, dy: 2 }, eyeR: { ...eyeS, dy: 2 },
        mouth: mix('small', 'yawn', yawn),
        hair: { dy: 2 + 3 * yawn, flop: 0.2 },
        armL: { rot: 10 + 55 * yawn }, armR: { rot: 6 + 6 * yawn, hy: 4 }, staff: { rot: -6 - 4 * yawn },
        legL: { rot: 2 }, legR: { rot: -2 },
      };
    },
  },
  {
    id: 'sleep', n: 24, loop: true, still: 8, desc: 'closed-eye lines, floating z',
    fn(f, n) {
      const t = f / n, br = (1 - cos(TAU * t)) / 2;
      const fx = [];
      for (let k = 0; k < 3; k++) {
        const p = (t + k / 3) % 1;
        fx.push({ type: 'z', at: 'head', x: 100 + 58 * p, y: -40 - 74 * p, s: 6 + 9 * p, w: 6.5, op: clamp(Math.pow(sin(Math.PI * p), 0.7), 0, 1) });
      }
      return {
        g: { sy: 0.955 + 0.03 * br, sx: 1.025 - 0.012 * br, rot: 2.5 },
        face: { dy: 4 }, eyeL: { shape: 'sleep', dy: 3 }, eyeR: { shape: 'sleep', dy: 3 },
        mouth: mouth('small', { w: 0.42, h: 0.4 + 0.15 * br, smile: 1 }),
        hair: { flop: 0.55, dy: 3 },
        armL: { rot: -10 }, armR: { rot: 6, hy: 8 }, staff: { rot: -6 },
        legL: { rot: 4 }, legR: { rot: -4 },
        fx,
      };
    },
  },
  ...['left', 'right'].map((dir) => ({
    id: `walk-${dir}`, n: 12, loop: true, still: 2, desc: `walk cycle to the ${dir} (desktop pet)`,
    fn(f, n) {
      const sgn = dir === 'left' ? -1 : 1;
      const ph = (f / n) * TAU;
      const step = Math.abs(sin(ph));
      const dy = -3.4 * step;
      const A = 24;
      const liftL = Math.max(0, cos(ph)), liftR = Math.max(0, -cos(ph));
      return {
        g: { dy, rot: 3.2 * sgn + 1.2 * sin(ph * 2), sy: 1 + 0.01 * step },
        face: { dx: 9 * sgn, dy: 0 }, eyeL: { dx: 4 * sgn }, eyeR: { dx: 4 * sgn },
        mouth: mouth('open', { dx: 2 * sgn, w: 0.9 }),
        hair: { dx: -2 * sgn, dy: 2 * sin(ph * 2 + 1), flop: -0.1 * sgn * (0.6 + 0.4 * sin(ph * 2)) },
        legL: { rot: A * sin(ph) * sgn, fy: -6 * liftL }, legR: { rot: -A * sin(ph) * sgn, fy: -6 * liftR },
        armL: { rot: -14 * sin(ph) * sgn + (sgn < 0 ? 8 : 0) }, armR: { rot: 10 * sin(ph) * sgn - 2 }, staff: { rot: 8 * sin(ph + 0.8) - 4 * sgn },
      };
    },
  })),
  {
    id: 'dragged', n: 12, loop: true, still: 3, desc: 'picked up, legs dangle (desktop pet)',
    fn(f, n) {
      const t = f / n, s = sin(TAU * t), s2 = sin(TAU * t - 0.9);
      return {
        g: { pivot: [256, 92], sy: 1.075, sx: 0.965, rot: 5 * s, dy: 0 },
        face: { dy: -1 }, eyeL: { r: 1.2, dy: -1 }, eyeR: { r: 1.2, dy: -1 },
        mouth: mix('small', 'O', 0.6 + 0.2 * sin(TAU * 2 * t)),
        hair: { lift: 0.5 + 0.2 * s2, flop: 0.25 * s2 },
        legL: { rot: 10 - 20 * s2, fy: 8 }, legR: { rot: -10 - 20 * s2, fy: 8 },
        armL: { rot: 62 + 22 * s2 }, armR: { rot: 0, hy: -10 }, staff: { rot: -6 + 9 * s2 },
      };
    },
  },
  {
    id: 'land', n: 9, loop: false, still: 2, desc: 'squash and stretch landing (desktop pet)',
    fn(f) {
      const k = [-0.1, -0.06, 0.17, 0.11, -0.07, 0.05, -0.02, 0.01, 0][f];
      const dy = [-34, -14, 0, 0, -6, 0, 0, 0, 0][f];
      const splay = [4, 3, 12, 8, 2, 3, 1, 0, 0][f];
      const lagK = [0, -0.1, 0.1, 0.4, 0.2, -0.1, 0, 0, 0][f];
      return {
        g: { dy, ...SQUASH(k) }, ...(dy < -2 ? {} : lockFeet(dy)),
        legL: { rot: splay }, legR: { rot: -splay },
        mouth: f < 2 ? mouth('small', { h: 0.6 }) : f < 5 ? mix('O', 'open', 0.3) : mouth('open'),
        eyeL: { r: f === 2 || f === 3 ? 1.2 : 1, open: f === 2 ? 0.55 : 1 }, eyeR: { r: f === 2 || f === 3 ? 1.2 : 1, open: f === 2 ? 0.55 : 1 },
        hair: { dy: -6 * lagK * 4, lift: 0.4 * Math.max(0, -lagK * 6) },
        armL: { rot: [45, 30, -8, 0, 12, 4, 0, 0, 0][f] }, armR: { rot: [-26, -16, 6, 2, -5, -1, 0, 0, 0][f] }, staff: { rot: [10, 8, -14, 8, -4, 2, 0, 0, 0][f] },
      };
    },
  },
  {
    id: 'loading', n: 12, loop: true, still: 3, desc: 'tiny icon: just the staff ring spinning',
    fn(f, n) {
      const t = f / n;
      const a = R(800, 372), b = R(809, 606);
      const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2 - 8];
      // closed ring, spinner ticks chase around it (12 ticks, fading trail)
      const head = f, fx = [];
      for (let k = 0; k < 12; k++) {
        const age = (head - k + 12) % 12;
        fx.push({ type: 'ticks', at: 'ring', n: 1, rot: k * 30 - 90, r0: (JOINTS.ringRadius + 17) * 1.65, r1: (JOINTS.ringRadius + 17) * 1.65 + 11, w: 6, op: Math.max(0.12, 1 - age * 0.085) });
      }
      return {
        only: 'staff', strokeScale: 1.3,
        g: { pivot: mid, sx: 1.65, sy: 1.65, dx: 256 - mid[0], dy: 256 - mid[1], rot: 0 },
        ring: { arc: null }, staff: { rot: 0 }, fx,
      };
    },
  },
];

export const STATE_BY_ID = Object.fromEntries(STATES.map((s) => [s.id, s]));
