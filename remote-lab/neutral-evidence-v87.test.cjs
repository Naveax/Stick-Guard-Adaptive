'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
global.window={GuardAdaptiveIdleQuality:require('./public/idle-quality.js')};
const neutral=require('./public/neutral-preview.js');
global.window.GuardAdaptiveNeutralPreview=neutral;
const {build,check}=require('./public/neutral-evidence-v87.js');
delete global.window;
function windowAt(x,y,n=101){
 return Array.from({length:n},(_,i)=>{
  const eps=i%2===0?0.0002:-0.0002;
  return [x+eps,y-eps,0.0372+eps,-eps];
 });
}
test('accept 441 signed +-10% synthetic neutral pairs as unverified evidence',()=>{
 let count=0;
 for(let x=-10;x<=10;x++)for(let y=-10;y<=10;y++){
  const o=build({first:windowAt(x/100,y/100),second:windowAt(x/100,y/100),moved:[true,true],axesCount:4});
  assert.equal(o.ok,true,JSON.stringify({x,y,o}));
  assert.equal(o.data.physically_verified,false);
  assert.equal(o.data.eligible_for_training,false);
  assert.equal(check(o.data).ok,true);
  assert.equal(o.data.first_window.length,101);
  count++;
 }
 assert.equal(count,441);
});
test('deny false claims of physical provenance or training consent',()=>{
 const valid=build({first:windowAt(0,0),second:windowAt(0,0),moved:[true,true],axesCount:4}).data;
 assert.equal(check({...valid,physically_verified:true}).ok,false);
 assert.equal(check({...valid,eligible_for_training:true}).ok,false);
 assert.equal(check({...valid,eligible_for_holdout:true}).ok,false);
 assert.equal(check({...valid,production_gate:'OPEN'}).ok,false);
 assert.equal(check({...valid,data_origin:'independently_verified'}).ok,false);
});
test('deny contaminated windows, insufficient data, disagreement, missing exercise',()=>{
 assert.equal(build({first:windowAt(0,0).slice(0,9),second:windowAt(0,0),moved:[true,true],axesCount:4}).ok,false);
 assert.equal(build({first:windowAt(0,0),second:windowAt(0.03,0),moved:[true,true],axesCount:4}).ok,false);
 assert.equal(build({first:windowAt(0,0),second:windowAt(0,0),moved:[false,true],axesCount:4}).ok,false);
 const bad=windowAt(0,0);bad[4][0]=Infinity;
 assert.equal(build({first:bad,second:windowAt(0,0),moved:[true,true],axesCount:4}).ok,false);
});
test('anonymous export contains no controller ID, buttons, session ID or survey',()=>{
 const data=build({first:windowAt(0.035,-0.02),second:windowAt(0.035,-0.02),moved:[true,true],axesCount:4}).data;
 const text=JSON.stringify(data);
 for(const secret of ['controller_id','session_id','gamepad_id','device_id','buttons','survey','comment','fingerprint'])
  assert.equal(text.includes(secret),false,secret);
});
