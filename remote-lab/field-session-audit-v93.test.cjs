'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),crypto=require('node:crypto');
const {auditRecord,summarizeDirectory}=require('./field-session-audit-v93.cjs');
const {filteredAxes,validateTelemetry,summarizeTelemetry}=require('./telemetry.cjs');
function fixture({custom=false,unstableRight=false,twoAxes=false}={}){
 const count=twoAxes?2:custom?10:4;
 const indices=twoAxes?[1,0,-1,-1]:custom?[0,1,5,2]:[0,1,2,3];
 const buttonsCount=custom?12:17;
 const samples=[];
 for(let p=0;p<6;p++)for(let i=0;i<120;i++){
  const a=Array(count).fill(0);
  if(twoAxes){a[1]=0.0039;a[0]=-0.0039;}
  else {
   a[indices[0]]=0.0039;a[indices[1]]=-0.0039;
   a[indices[2]]=unstableRight && p===3?(i%2===0?0:0.008):0.0039;
   a[indices[3]]=-0.0039;
  }
  if(p===1){a[indices[0]]=i%2===0?0.004:0.05;}
  if(p===4&&!twoAxes){a[indices[2]]=i%2===0?0.004:0.045;}
  const f=filteredAxes(a,indices,twoAxes?2:4,.08);
  samples.push([p,p*13500+i*90,a,Array(buttonsCount).fill(0),f]);
 }
 const record={schema:'ga-controller-telemetry-v1',session_id:crypto.randomUUID(),condition:'regular',
 controller:{axes_count:count,buttons_count:buttonsCount,mapping:custom||twoAxes?'custom':'standard',indices},
 filter:{profile:'radial-demo-v1',deadzone:8},consent:{measurement_ack:true,upload:true},
 samples,survey:null};
 assert.equal(validateTelemetry(record),null);
 return {...record,analysis:summarizeTelemetry(record),independent_ground_truth:false,production_gate:'BLOCKED'};
}
test('ten axis / twelve button mapped controller passes without leaking raw samples',()=>{
 const r=fixture({custom:true}),a=auditRecord(r);
 assert.equal(a.ok,true);
 assert.equal(a.controller_axes,10);
 assert.deepEqual(a.indices,[0,1,5,2]);
 assert.equal(a.neutral.length,2);
 assert.equal(a.neutral[0].constant_plateau_unverified,true);
 assert.equal(a.neutral[1].constant_plateau_unverified,true);
 assert.equal(a.micro_comparator.length,2);
 assert.ok(a.micro_comparator[0].zeroed_by_fixed_deadzone>0);
 assert.equal(a.eligible_for_training,false);
 assert.equal(a.production_gate,'BLOCKED');
 assert.equal('samples' in a,false);
 assert.equal('session_id' in a,false);
 assert.equal(JSON.stringify(a).includes(r.session_id),false);
});
test('noisy secondary neutral is flagged, not silently accepted',()=>{
 const a=auditRecord(fixture({unstableRight:true}));
 assert.equal(a.ok,true);
 assert.equal(a.neutral[0].stable_but_unverified,true);
 assert.equal(a.neutral[1].stable_but_unverified,false);
 assert.equal(a.neutral[1].quality,'MOTION_OR_UNSTABLE_NEUTRAL');
 assert.equal(a.neutral[1].constant_plateau_unverified,false);
});
test('two axis controller never invents right hand measurements',()=>{
 const a=auditRecord(fixture({twoAxes:true}));
 assert.equal(a.ok,true);
 assert.equal(a.neutral.length,1);
 assert.equal(a.micro_comparator.length,1);
 assert.deepEqual(a.phase_counts,[120,120,120,120,120,120]);
});
test('modified output, indices, consent and training claims fail closed',()=>{
 const invalid=[
 d=>{d.samples[0][4][0]=0.8},
 d=>{d.controller.indices=[0,0,5,2]},
 d=>{d.consent.upload=false},
 d=>{d.production_gate='OPEN'},
 d=>{d.analysis.auto_training=true},
 d=>{d.independent_ground_truth=true},
 d=>{d.analysis.independent_ground_truth=true},
 d=>{d.analysis.left.rest_raw_rms_percent=98.5},
 d=>{d.analysis.sample_count=5000},
 d=>{d.private_session_ids=['bad']},
 d=>{d.samples[1][1]=d.samples[0][1]}
 ];
 for(const mutate of invalid){
  const d=fixture({custom:true});mutate(d);
  const a=auditRecord(d);
  assert.equal(a.ok,false,'bad record accepted');
 }
});
test('file audit sorts without exposing source filenames or identity',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'ga-v93-'));
 try{
  const a=fixture(),b=fixture({custom:true});
  fs.writeFileSync(path.join(dir,a.session_id+'.json'),JSON.stringify(a));
  fs.writeFileSync(path.join(dir,b.session_id+'.json'),JSON.stringify(b));
  const data=summarizeDirectory(dir);
  assert.equal(data.length,2);
  for(const row of data){
   assert.ok([1,2].includes(row.run));
   assert.equal('session_id' in row,false);
   assert.equal('path' in row,false);
   assert.equal('samples' in row,false);
   assert.equal(row.ok,true);
  }
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
