'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const {checkIdle}=require('./public/idle-quality.js');
const stable=(x,y,n=128)=>Array.from({length:n},(_,i)=>{
  const d=i%2===0?0.0002:-0.0002;
  return [x+d,y-d,0.0372+d,0.0-d];
});
test('441 signed +-10% clean neutral positions are observationally stable',()=>{
  let pass=0;
  for(let x=-10;x<=10;x++)for(let y=-10;y<=10;y++){
    const info=checkIdle(stable(x/100,y/100),0);
    assert.equal(info.valid,true,JSON.stringify({x,y,info}));
    assert.equal(info.physicalReleaseVerified,false);
    assert.equal(info.eligibleForTraining,false);
    pass++;
  }
  assert.equal(pass,441);
});
test('large transient in a supposed neutral interval is rejected',()=>{
  const rows=stable(0.025,0.01);
  rows[29]=[0.81,0.4,0.0372,0];
  const left=checkIdle(rows,0);
  const right=checkIdle(rows,1);
  assert.equal(left.valid,false);
  assert.equal(left.reason,'MOTION_OR_UNSTABLE_NEUTRAL');
  assert.equal(right.valid,true);
});
test('small persistent drift does not automatically imply physical release',()=>{
  const rows=stable(0.10,-0.10);
  const check=checkIdle(rows,0);
  assert.equal(check.valid,true);
  assert.equal(check.physicalReleaseVerified,false);
  assert.equal(check.interpretation,'UNVERIFIED_OBSERVATION');
});
test('slow change, malformed values and missing windows fail closed',()=>{
  const rows=stable(0.03,0.01);
  rows.forEach((r,i)=>{r[0]+=0.02*i/127;});
  assert.equal(checkIdle(rows,0).valid,false);
  assert.equal(checkIdle(stable(0,0,34),0).reason,'INSUFFICIENT_SAMPLES');
  const broken=stable(0,0);broken[20]=[Infinity,0,0,0];
  assert.equal(checkIdle(broken,0).reason,'INVALID_SAMPLE');
  assert.equal(checkIdle(stable(0,0),4).reason,'INVALID_STICK');
});
