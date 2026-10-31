// debug: node strip.mjs <state> [outfile] -> every frame of a state in a grid
import fs from 'fs'; import os from 'os'; import path from 'path'; import { Resvg } from '@resvg/resvg-js';
import { makePose, buildFrame, frameSvg, BLACK } from './lib/draw.mjs'; import { STATE_BY_ID } from './lib/states.mjs';
const id=process.argv[2]; const outf=process.argv[3]||path.join(os.tmpdir(),`potato-strip-${id}.png`); const st=STATE_BY_ID[id];
const cell=200, cols=Math.min(st.n,6), rows=Math.ceil(st.n/cols); let g='';
for(let f=0;f<st.n;f++){const fr=buildFrame(makePose(st.fn(f,st.n)),f%3,f);const x=(f%cols)*(cell+6)+6,y=Math.floor(f/cols)*(cell+22)+6;
 g+=`<g transform="translate(${x} ${y}) scale(${cell/512})"><circle cx="256" cy="256" r="256" fill="${BLACK}"/>${frameSvg(fr,{ids:false,wrap:false})}</g><text x="${x+cell/2}" y="${y+cell+15}" font-size="14" fill="#ccc" text-anchor="middle" font-family="Arial">${f+1}</text>`;}
const W=cols*(cell+6)+6,H=rows*(cell+22)+6;
fs.writeFileSync(outf,new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"><rect width="${W}" height="${H}" fill="#2a2a2a"/>${g}</svg>`).render().asPng());
