import fs from 'fs'; import jpeg from 'jpeg-js';
const img = jpeg.decode(fs.readFileSync('../../reference.jpg'), {useTArray:true});
const {width:W,height:H,data}=img; console.log(W,H);
// orange mask
const isO=(i)=>{const r=data[i],g=data[i+1],b=data[i+2];return r>200&&g>130&&g<200&&b<120;};
let minx=1e9,miny=1e9,maxx=0,maxy=0,n=0,sr=0,sg=0,sb=0;
for(let y=0;y<H;y++)for(let x=0;x<W;x++){const i=(y*W+x)*4;if(isO(i)){n++;sr+=data[i];sg+=data[i+1];sb+=data[i+2];minx=Math.min(minx,x);maxx=Math.max(maxx,x);miny=Math.min(miny,y);maxy=Math.max(maxy,y);}}
console.log({n,minx,maxx,miny,maxy,col:[sr/n,sg/n,sb/n].map(Math.round)});
// black circle extents
let cx0=1e9,cx1=0,cy0=1e9,cy1=0;
for(let y=0;y<H;y++)for(let x=0;x<W;x++){const i=(y*W+x)*4;if(data[i]<40&&data[i+1]<40&&data[i+2]<40){cx0=Math.min(cx0,x);cx1=Math.max(cx1,x);cy0=Math.min(cy0,y);cy1=Math.max(cy1,y);}}
console.log({cx0,cx1,cy0,cy1});
// stroke width: horizontal run lengths on row through left body edge / vertical run on bottom
function runs(y){const r=[];let s=-1;for(let x=0;x<W;x++){const o=isO((y*W+x)*4);if(o&&s<0)s=x;if(!o&&s>=0){r.push([s,x-s]);s=-1;}}return r;}
console.log('row 400',runs(400)); console.log('row 600',runs(600)); console.log('row 520',runs(520));
