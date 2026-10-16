'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const quality=require('../public-simple/idle-quality.js');
// Node shim only for the browser module's quality dependency, no devices or network.
global.GuardAdaptiveIdleQuality=quality;
const {moveEvidence,review,compare}=require('../public-simple/neutral-preview.js');
// CommonJS factory resolves root.GuardAdaptiveIdleQuality only when window is defined.
// In Node no window exists, so inject a narrow isolated window then reload module.
delete require.cache[require.resolve('../public-simple/neutral-preview.js')];
global.window={GuardAdaptiveIdleQuality:quality};
const preview=require('../public-simple/neutral-preview.js');
delete global.window;
const samples=(x,y,n=128)=>Array.from({length:n},(_,i)=>{const noise=i%2?0.0002:-0.0002;return [x+noise,y-noise,0.0372+noise,-noise];});
test('two independently reviewed neutral windows accept signed 441-point grid',()=>{
 let accepted=0;
 for(let x=-10;x<=10;x++)for(let y=-10;y<=10;y++){
  const first=samples(x/100,y/100),second=samples(x/100,y/100);
  const proof=preview.moveEvidence([x/100+0.11,y/100,0.14,0],[[x/100,y/100],[0.0372,0]],4);
  assert.deepEqual(proof,[true,true]);
  const result=preview.review(first,second,proof,4);
  assert.equal(result.ok,true,JSON.stringify({x,y,result}));
  const out=preview.compare([x/100+0.002,y/100,0.0382,0],result.centers,0.08,4);
  assert.ok(Math.abs(out[0].centerOnlyPct-0.2)<0.02);
  assert.ok(Math.abs(out[1].centerOnlyPct-0.1)<0.02);
  assert.equal(result.physicallyVerified,false);
  assert.equal(result.productionGate,'BLOCKED');
  accepted++;
 }
 assert.equal(accepted,441);
});
test('false neutral, jump and disagreement fail safely or show uncertainty',()=>{
 const first=samples(0.025,0.012),second=samples(0.025,0.012);
 assert.equal(preview.review(first,second,[false,true],4).reason,'MOVEMENT_NOT_OBSERVED');
 assert.equal(preview.review(first,samples(0.04,0.012),[true,true],4).reason,'WINDOWS_DISAGREE_LEFT');
 const contaminated=samples(0.025,0.012);contaminated[17][0]=0.9;
 assert.equal(preview.review(contaminated,second,[true,true],4).reason,'UNSTABLE_WINDOW_LEFT');
 const stableHeld=samples(0.01,0);
 const fooled=preview.review(stableHeld,stableHeld,[true,true],4);
 assert.equal(fooled.ok,true); // cannot prove physical release
 assert.equal(fooled.physicallyVerified,false);
 assert.ok(preview.compare([0,0,0,0],fooled.centers,0.08,4)[0].centerOnlyPct>0.9);
});
test('micro intention remains after center subtraction, but fixed deadzone erases it',()=>{
 const result=preview.review(samples(0.05,0),samples(0.05,0),[true,true],4);
 assert.equal(result.ok,true);
 const row=preview.compare([0.053,0,0.0372,0.001],result.centers,0.08,4);
 assert.equal(row[0].fixedPct,0);
 assert.ok(row[0].centerOnlyPct>0.28&&row[0].centerOnlyPct<0.32);
});
test('two-axis controls and invalid records are handled without phantom right stick',()=>{
 const data=samples(-0.01,0.015);
 const passed=preview.review(data,data,[true,false],2);
 assert.equal(passed.ok,true);
 assert.deepEqual(preview.moveEvidence([0.10,0.015,0,0],[[-0.01,0.015],[0,0]],2),[true,true]);
 assert.equal(preview.compare([-0.01,0.015,0,0],passed.centers,0.08,2)[1],null);
 assert.equal(preview.review(data,data,[false,true],2).ok,false);
 assert.equal(preview.compare([NaN,0,0,0],passed.centers,0.08,2),null);
});
