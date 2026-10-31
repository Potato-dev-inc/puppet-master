// Potato rig: the traced geometry of the founder's doodle (reference.jpg).
// All source numbers below are centre-line samples measured from the reference
// (954x958 px, black circle centre (452,481), diameter 874). They are mapped into a
// 512x512 canvas where the black circle exactly fills the canvas.

export const COLOR = '#F0A845';      // sampled from reference.jpg (most common orange)
export const BLACK = '#050505';      // sampled from reference.jpg (circle)
export const CANVAS = 512;
export const STROKE = 14;            // ~2.7% of canvas; reference measures ~2.5% (22px of 874)
export const K = 512 / 874;

// reference px -> canvas px
export const R = (x, y) => [(x - 452) * K + 256, (y - 481) * K + 256];
const RR = (arr) => arr.map(([x, y]) => R(x, y));

export const EYE_R = 9.6;
export const FEET = R(452, 745);      // squash/stretch pivot: between the feet

// ---- parts -------------------------------------------------------------
// mode: 'smooth' = Catmull-Rom through the points, 'sharp' = straight segments (round joins)
export const PARTS = {
  // the head/body: one open path. It starts at the hair-join (left of the top line) and ends
  // just past it, so the two ends overlap a little like in the drawing.
  body: {
    mode: 'smooth', closed: false, amp: 1.5, seed: 11,
    pts: RR([
      [412, 337], [360, 340], [319, 342], [275, 349], [232, 368], [198, 399], [177, 443], [166, 490],
      [174, 538], [203, 578], [238, 610], [278, 630], [320, 639], [357, 643], [388, 646], [417, 648],
      [445, 650], [473, 651], [504, 652], [539, 653], [593, 648], [623, 640], [662, 615], [692, 580],
      [707, 536], [711, 490], [702, 445], [679, 405], [655, 369], [621, 342], [581, 328], [545, 316],
      [509, 313], [477, 311], [445, 311], [414, 317],
    ]),
  },
  // the spiky crown: base line, left edge, three spikes, right edge.
  hair: {
    mode: 'sharp', closed: false, amp: 1.7, seed: 23,
    pts: RR([
      [412, 325], [356, 319], [302, 314], [322, 276], [353, 232], [383, 259], [430, 213],
      [462, 255], [510, 226], [566, 192], [576, 232], [584, 274], [594, 301],
    ]),
    // per-point weight used when the spikes jump / flop (0 = anchored base)
    weight: [0, 0, 0, 0.35, 1, 0.6, 1, 0.6, 0.8, 1.15, 0.7, 0.3, 0.1],
    pivot: R(452, 316),
  },
  'arm-left': {
    mode: 'smooth', closed: false, amp: 1.0, seed: 31,
    pts: RR([[166, 504], [133, 530], [112, 550], [92, 570], [86, 578]]),
  },
  'arm-right': {
    mode: 'smooth', closed: false, amp: 1.0, seed: 37,
    pts: RR([[716, 470], [742, 470], [762, 478], [782, 488], [805, 499]]),
  },
  staff: {
    mode: 'smooth', closed: false, amp: 0.9, seed: 41,
    pts: RR([[792, 372], [794, 390], [797, 420], [802, 450], [805, 480], [807, 520], [808, 550], [809, 580], [809, 606]]),
  },
  ring: {
    mode: 'smooth', closed: true, amp: 1.3, seed: 43,
    pts: RR([[816, 334], [814, 350], [804, 364], [786, 372], [766, 371], [753, 354], [754, 334],
      [761, 319], [772, 308], [787, 308], [800, 312], [810, 321]]),
  },
  'leg-left': {
    mode: 'smooth', closed: false, amp: 1.0, seed: 53,
    pts: RR([[332, 648], [322, 680], [314, 700], [305, 720], [300, 742]]),
  },
  'leg-right': {
    mode: 'smooth', closed: false, amp: 1.0, seed: 59,
    pts: RR([[580, 650], [596, 680], [608, 700], [620, 720], [638, 747]]),
  },
};

export const JOINTS = {
  shoulderL: PARTS['arm-left'].pts[0],
  shoulderR: PARTS['arm-right'].pts[0],
  grip: R(805, 499),                    // where the hand holds the staff
  hipL: PARTS['leg-left'].pts[0],
  hipR: PARTS['leg-right'].pts[0],
  hairBase: R(452, 316),
  ringCentre: R(785, 340),
  ringRadius: 27 * K + 4,
  staffTip: PARTS.staff.pts[PARTS.staff.pts.length - 1],
  eyeL: R(280, 479),
  eyeR: R(596, 483),
  mouth: R(425.5, 549.5),
};

// ---- mouth: 16 outline points (reference px), normalised to the bbox ----------------
const MOUTH_REF = [
  [537, 538], [526, 562], [501, 581], [463, 587], [426, 588], [390, 586], [357, 579], [328, 566],
  [314, 550], [322, 532], [358, 522], [395, 519], [426, 518], [462, 514], [500, 511], [526, 518],
];
const MX = 425.5, MY = 549.5, MHW = 111.5, MHH = 38.5;
export const MOUTH = {
  centre: R(MX, MY),
  halfW: MHW * K - 3,   // minus half of the 6px fill-stroke that the renderer adds back
  halfH: MHH * K - 3,
  norm: MOUTH_REF.map(([x, y]) => {
    const nx = (x - MX) / MHW, ny = (y - MY) / MHH;
    return { nx, ny, a: Math.atan2(ny, nx) };
  }),
};

// Named mouth shapes. w/h scale the reference mouth, round blends towards an ellipse,
// smile lifts (+) or drops (-) the corners in px.
export const MOUTHS = {
  open: { w: 1, h: 1, round: 0, smile: 0 },                 // the doodle's happy open mouth
  small: { w: 0.6, h: 0.55, round: 0.1, smile: 2 },
  closed: { w: 0.62, h: 0.08, round: 0, smile: 5 },
  wide: { w: 1.12, h: 1.45, round: 0.1, smile: 0 },
  O: { w: 0.34, h: 1.05, round: 1, smile: 0 },
  flat: { w: 0.62, h: 0.06, round: 0, smile: 0 },
  down: { w: 0.62, h: 0.1, round: 0, smile: -13 },
  yawn: { w: 0.56, h: 1.6, round: 0.85, smile: 0 },
  grin: { w: 1.1, h: 0.8, round: 0, smile: 6 },
};
