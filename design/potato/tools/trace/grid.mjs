import fs from 'fs'; import {Resvg} from '@resvg/resvg-js';
const [,, x0,y0,x1,y1,out,step='25'] = process.argv;
const X0=+x0,Y0=+y0,X1=+x1,Y1=+y1,S=+step; const w=X1-X0,h=Y1-Y0; const sc=Math.min(1400/w,1400/h);
const b64=fs.readFileSync('../../reference.jpg').toString('base64');
let g='';
for(let x=Math.ceil(X0/S)*S;x<=X1;x+=S){g+=`<line x1="${x}" y1="${Y0}" x2="${x}" y2="${Y1}" stroke="${x%100==0?'#0ff':'#0a8'}" stroke-width="${0.6/sc*1.2}"/><text x="${x+1}" y="${Y0+10/sc}" font-size="${11/sc}" fill="#fff" font-family="Arial">${x}</text>`;}
for(let y=Math.ceil(Y0/S)*S;y<=Y1;y+=S){g+=`<line x1="${X0}" y1="${y}" x2="${X1}" y2="${y}" stroke="${y%100==0?'#0ff':'#0a8'}" stroke-width="${0.6/sc*1.2}"/><text x="${X0+1}" y="${y-1}" font-size="${11/sc}" fill="#fff" font-family="Arial">${y}</text>`;}
const svg=`<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${w*sc}" height="${h*sc}" viewBox="${X0} ${Y0} ${w} ${h}"><image x="0" y="0" width="954" height="958" href="data:image/jpeg;base64,${b64}"/>${g}</svg>`;
fs.writeFileSync(out,new Resvg(svg).render().asPng());
