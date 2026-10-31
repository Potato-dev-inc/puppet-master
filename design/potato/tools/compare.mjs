// Side-by-side: reference | our idle render | overlay (reference dimmed under our line).
// usage: node compare.mjs [out.png] [poseName]
import fs from 'fs';
import { Resvg } from '@resvg/resvg-js';
import { makePose, buildFrame, frameSvg } from './lib/draw.mjs';

const out = process.argv[2] || '../base/compare-reference.png';
const b64 = fs.readFileSync(new URL('../reference.jpg', import.meta.url)).toString('base64');
const frame = buildFrame(makePose({}), 0, 0);
const mine = frameSvg(frame, { circle: true, ids: false, wrap: false });
const mineOnly = frameSvg(frame, { circle: false, ids: false, wrap: false });
const refImg = (x) =>
  `<g transform="translate(${x} 0)"><svg x="0" y="0" width="512" height="512" viewBox="15 44 874 874"><image width="954" height="958" href="data:image/jpeg;base64,${b64}"/></svg></g>`;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1590" height="540" viewBox="0 0 1590 540"><rect width="1590" height="540" fill="#fff"/>
<g transform="translate(14 14)">${refImg(0)}
<g transform="translate(524 0)"><circle cx="256" cy="256" r="256" fill="#050505"/>${mineOnly}</g>
<g transform="translate(1048 0)">${refImg(0).replace('translate(0 0)', 'translate(0 0)')}
<g opacity="0.8"><g stroke-opacity="1">${mineOnly.replace('stroke="#F0A845"', 'stroke="#33ddff"')}</g></g></g></g>
<g font-family="Arial" font-size="14" fill="#222"><text x="14" y="534">reference</text><text x="538" y="534">our render (idle, boil 0)</text><text x="1062" y="534">overlay (cyan = ours)</text></g></svg>`;
fs.mkdirSync(new URL('..' + '/base/', import.meta.url), { recursive: true });
fs.writeFileSync(new URL(out, import.meta.url), new Resvg(svg).render().asPng());
console.log('wrote', out);
