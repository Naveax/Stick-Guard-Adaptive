'use strict';
// Browser-only, conservative two-window calibration preview. No game/driver writes.
(function(root,factory){
 const api=Object.freeze(factory(root.GuardAdaptiveIdleQuality));
 if(typeof module!=='undefined'&&module.exports)module.exports=api;
 else root.GuardAdaptiveNeutralPreview=api;
})(typeof window!=='undefined'?window:{},function(quality){
 const THRESHOLD=0.075;
 const MAX_AGREEMENT=0.006;
 const validPoint=v=>Array.isArray(v)&&v.length===4&&v.every(x=>typeof x==='number'&&Number.isFinite(x)&&Math.abs(x)<=1);
 const median=data=>{
  const a=[...data].sort((x,y)=>x-y),mid=Math.floor(a.length/2);
  return a.length%2?a[mid]:(a[mid-1]+a[mid])*0.5;
 };
 const center=(rows,side)=>[median(rows.map(v=>v[side*2])),median(rows.map(v=>v[side*2+1]))];
 const clamp=n=>Math.max(-1,Math.min(1,n));
 const radialSaturate=(x,y)=>{const r=Math.hypot(x,y);return r>1?[x/r,y/r]:[x,y]};
 function moveEvidence(raw,firstCenters,axesCount){
  if(!validPoint(raw)||![2,4].includes(axesCount)||!Array.isArray(firstCenters))return [false,false];
  return [0,1].map(side=>side===1&&axesCount===2?true:
   Math.hypot(raw[side*2]-firstCenters[side][0],raw[side*2+1]-firstCenters[side][1])>=THRESHOLD);
 }
 function reviewFirst(first,axesCount){
  if(!quality||typeof quality.checkIdle!=='function'||![2,4].includes(axesCount)||
     !Array.isArray(first)||first.length<35||first.length>4096||
     first.some(v=>!validPoint(v)))return {ok:false,reason:'FIRST_WINDOW_INVALID'};
  const centers=[];
  for(let side=0;side<axesCount/2;side++){
   if(!quality.checkIdle(first,side).valid)
     return {ok:false,reason:'FIRST_WINDOW_UNSTABLE_'+(side?'RIGHT':'LEFT')};
   centers.push(center(first,side));
  }
  if(axesCount===2)centers.push([0,0]);
  return {ok:true,centers,physicallyVerified:false};
 }
 function review(first,second,moved,axesCount){
  const fail=reason=>({ok:false,reason,physicallyVerified:false,productionGate:'BLOCKED'});
  if(!quality||typeof quality.checkIdle!=='function')return fail('QUALITY_MODULE_UNAVAILABLE');
  if(!Array.isArray(first)||!Array.isArray(second)||![2,4].includes(axesCount))
   return fail('INVALID_WINDOWS');
  if(first.length<35||first.length>4096||second.length<35||second.length>4096)
   return fail('INSUFFICIENT_WINDOWS');
  if(first.some(v=>!validPoint(v))||second.some(v=>!validPoint(v)))
   return fail('INVALID_SAMPLE');
  if(!Array.isArray(moved)||!moved[0]||(axesCount===4&&!moved[1]))
   return fail('MOVEMENT_NOT_OBSERVED');
  const centers=[];
  const diagnostics=[];
  for(let s=0;s<axesCount/2;s++){
   const a=quality.checkIdle(first,s),b=quality.checkIdle(second,s);
   if(!a.valid||!b.valid)return fail('UNSTABLE_WINDOW_'+(s?'RIGHT':'LEFT'));
   const c1=center(first,s),c2=center(second,s);
   const disagree=Math.hypot(c1[0]-c2[0],c1[1]-c2[1]);
   if(disagree>MAX_AGREEMENT)return fail('WINDOWS_DISAGREE_'+(s?'RIGHT':'LEFT'));
   centers.push([(c1[0]+c2[0])/2,(c1[1]+c2[1])/2]);
   diagnostics.push({disagreementPct:disagree*100,maxNoisePct:Math.max(a.noiseRmsPct,b.noiseRmsPct)});
  }
  if(axesCount===2)centers.push([0,0]);
  return {ok:true,centers,diagnostics,physicallyVerified:false,
   label:'SELF_REPORTED_NEUTRAL_NOT_GROUND_TRUTH',productionGate:'BLOCKED'};
 }
 function compare(raw,centers,deadzone,axesCount){
  if(!validPoint(raw)||![2,4].includes(axesCount)||!Array.isArray(centers)||
   centers.length!==2||!centers.every(v=>Array.isArray(v)&&v.length===2&&v.every(Number.isFinite))||
   !Number.isFinite(deadzone)||deadzone<0||deadzone>=1)return null;
  return [0,1].map(side=>{
   if(side===1&&axesCount===2)return null;
   const x=raw[2*side],y=raw[2*side+1],r=Math.hypot(x,y);
   const fixed=r<=deadzone?[0,0]:
    radialSaturate(x*(r-deadzone)/(r*(1-deadzone)),y*(r-deadzone)/(r*(1-deadzone)));
   const corrected=radialSaturate(x-centers[side][0],y-centers[side][1]);
   return {rawPct:r*100,fixedPct:Math.hypot(...fixed)*100,
    centerOnlyPct:Math.hypot(...corrected)*100,centerOnlyXY:corrected};
  });
 }
 return {moveEvidence,reviewFirst,review,compare};
});
