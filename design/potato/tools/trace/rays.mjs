import fs from 'fs'; import jpeg from 'jpeg-js';
const img = jpeg.decode(fs.readFileSync('../../reference.jpg'), {useTArray:true});
const {width:W,height:H,data}=img;
const isO=(x,y)=>{x=Math.round(x);y=Math.round(y);if(x<0||y<0||x>=W||y>=H)return false;const i=(y*W+x)*4;return data[i]>200&&data[i+1]>130&&data[i+1]<200&&data[i+2]<120;};
const C=[445,490];
for(let a=0;a<360;a+=10){const rad=a*Math.PI/180;const runs=[];let s=-1;
 for(let r=0;r<420;r+=1){const x=C[0]+Math.cos(rad)*r,y=C[1]+Math.sin(rad)*r;const o=isO(x,y);if(o&&s<0)s=r;if(!o&&s>=0){runs.push([s,r]);s=-1;}}
 console.log(a, runs.map(([s,e])=>{const m=(s+e)/2;return `(${Math.round(C[0]+Math.cos(rad)*m)},${Math.round(C[1]+Math.sin(rad)*m)}) w${e-s}`}).join(' '));}
