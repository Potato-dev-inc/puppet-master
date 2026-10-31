import fs from 'fs'; import jpeg from 'jpeg-js';
const img = jpeg.decode(fs.readFileSync('../../reference.jpg'), {useTArray:true});
const {width:W,height:H,data}=img; const h=new Map();
for(let y=0;y<H;y++)for(let x=0;x<W;x++){const i=(y*W+x)*4;const r=data[i],g=data[i+1],b=data[i+2];if(r>200&&g>140&&g<190&&b<110){const k=[r>>2<<2,g>>2<<2,b>>2<<2].join(',');h.set(k,(h.get(k)||0)+1);}}
console.log([...h.entries()].sort((a,b)=>b[1]-a[1]).slice(0,5));
const bi=(470*W+10)*4; console.log('bg',data[(481*W+452)*4],data[(481*W+452)*4+1], data[(481*W+452)*4+2]);
