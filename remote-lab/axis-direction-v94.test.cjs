'use strict';
const test=require('node:test'), assert=require('node:assert/strict');
const core=require('./public/axis-direction-v94.js');
function samples(axes,fn,n=65){return Array.from({length:n},(_,i)=>fn(i,Array(axes).fill(0)));}
test('nonstandard axis mapping [0,1,5,2] accepts manually directed samples, no remap',()=>{
 const map=[0,1,5,2],raw=samples(10,(_,a)=>(a[0]=.02,a[1]=-.01,a[5]=.04,a[2]=-.03,a));
 assert.equal(core.mappingOk(map,10),true);
 const rest=core.neutral(raw,map);assert.equal(rest.ok,true);
 for(const step of core.moves){
  const shifted=samples(10,(_,a)=>{a[0]=.02;a[1]=-.01;a[5]=.04;a[2]=-.03;a[map[step.axis]]+=step.sign*.75;return a;});
  const result=core.analyzeStep(shifted,map,rest.centers,step);
  assert.equal(result.status,'CONSISTENT_WITH_PROMPT',step.name);
  assert.equal(result.physicallyVerified,false);
 }
});
test('standard Y-axis inversion must be reported, not silently rewritten',()=>{
 const map=[0,1,2,3],neutral=core.neutral(samples(4,(_,a)=>a),map);
 const up=core.moves[2],out=core.analyzeStep(samples(4,(_,a)=>{a[1]=.7;return a;}),map,neutral.centers,up);
 assert.equal(out.status,'REVERSED_SIGN');
});
test('cross-talk to other axis and stick blocks claim of isolation',()=>{
 const map=[0,1,2,3],zero=[0,0,0,0],step=core.moves[0];
 assert.equal(core.analyzeStep(samples(4,(_,a)=>{a[0]=-.7;a[1]=.6;return a;}),map,zero,step).status,'CROSS_AXIS_ACTIVITY');
 assert.equal(core.analyzeStep(samples(4,(_,a)=>{a[0]=-.7;a[2]=.5;return a;}),map,zero,step).status,'CROSS_AXIS_ACTIVITY');
});
test('single spike or weakly held input cannot pass a direction',()=>{
 const map=[0,1,2,3],step=core.moves[1];
 const spike=samples(4,(i,a)=>{if(i===0)a[0]=.9;return a;});
 const out=core.analyzeStep(spike,map,[0,0,0,0],step);
 assert.equal(out.status,'WEAK_OR_UNSTABLE_DIRECTION');
});
test('unstable neutral, invalid axis sample, and duplicate indices are rejected',()=>{
 assert.equal(core.mappingOk([0,0,5,2],10),false);
 assert.equal(core.mappingOk([0,1,5,11],10),false);
 assert.equal(core.mappingOk([0,1],2),true);
 assert.equal(core.neutral(samples(4,(i,a)=>{a[1]=i%2===0?.06:-.06;return a;}),[0,1,2,3]).ok,false);
 assert.equal(core.neutral(samples(4,(i,a)=>{if(i===7)a[1]=Infinity;return a;}),[0,1,2,3]).ok,false);
});
test('two-axis controller has only four intended manual steps',()=>{
 const map=[1,0];assert.equal(core.mappingOk(map,2),true);
 const center=core.neutral(samples(2,(_,a)=>a),map);
 assert.equal(center.ok,true);
 for(const step of core.moves.slice(0,4)){
  const a=samples(2,(_,a)=>{a[map[step.axis]]=step.sign*.65;return a;});
  assert.equal(core.analyzeStep(a,map,center.centers,step).status,'CONSISTENT_WITH_PROMPT');
 }
});
