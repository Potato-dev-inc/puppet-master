import fs from 'fs'; import jpeg from 'jpeg-js';
const img = jpeg.decode(fs.readFileSync('../../reference.jpg'), {useTArray:true});
const {width:W,height:H,data}=img;
const isO=(x,y)=>{x=Math.round(x);y=Math.round(y);const i=(y*W+x)*4;return data[i]>200&&data[i+1]>130&&data[i+1]<200&&data[i+2]<120;};
function outline(C,n,max=160){const pts=[];for(let k=0;k<n;k++){const a=k*2*Math.PI/n;let r=0;while(r<max&&isO(C[0]+Math.cos(a)*(r+1),C[1]+Math.sin(a)*(r+1)))r++;pts.push([Math.round(C[0]+Math.cos(a)*r),Math.round(C[1]+Math.sin(a)*r)]);}return pts;}
console.log('mouth',JSON.stringify(outline([426,550],16)));
console.log('leye',JSON.stringify(outline([280,479],8)));
console.log('reye',JSON.stringify(outline([596,483],8)));
// ring centerline: rays from ring center
const C=[787,334];
for(let a=0;a<360;a+=30){const rad=a*Math.PI/180;const runs=[];let s=-1;for(let r=0;r<70;r++){const o=isO(C[0]+Math.cos(rad)*r,C[1]+Math.sin(rad)*r);if(o&&s<0)s=r;if(!o&&s>=0){runs.push([s,r]);s=-1;}}
 console.log('ring',a,runs.map(([s,e])=>{const m=(s+e)/2;return `(${Math.round(C[0]+Math.cos(rad)*m)},${Math.round(C[1]+Math.sin(rad)*m)}) w${e-s}`}).join(' '));}
// staff & arm sample rows
for(const y of [390,420,450,480,500,520,550,580,600,610]){let runs=[],s=-1;for(let x=700;x<860;x++){const o=isO(x,y);if(o&&s<0)s=x;if(!o&&s>=0){runs.push([s,x-s]);s=-1;}}console.log('row',y,JSON.stringify(runs));}
for(const x of [760,780]){let runs=[],s=-1;for(let y=440;y<520;y++){const o=isO(x,y);if(o&&s<0)s=y;if(!o&&s>=0){runs.push([s,y-s]);s=-1;}}console.log('col',x,JSON.stringify(runs));}
// legs & left arm
for(const y of [660,680,700,720,735,745]){let runs=[],s=-1;for(let x=250;x<700;x++){const o=isO(x,y);if(o&&s<0)s=x;if(!o&&s>=0){runs.push([s,x-s]);s=-1;}}console.log('legrow',y,JSON.stringify(runs));}
for(const y of [510,530,550,570,580]){let runs=[],s=-1;for(let x=60;x<180;x++){const o=isO(x,y);if(o&&s<0)s=x;if(!o&&s>=0){runs.push([s,x-s]);s=-1;}}console.log('armrow',y,JSON.stringify(runs));}
