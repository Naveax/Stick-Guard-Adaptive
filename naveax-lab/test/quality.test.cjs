'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const os=require('node:os');
const path=require('node:path');
const crypto=require('node:crypto');
const {reviewSampleQuality}=require('../quality.cjs');
const {reviewDirectory}=require('../review-sessions.cjs');
const {validateSubmission}=require('../validate.cjs');

function session({kind='plausible',uuid=crypto.randomUUID()}={}){
  const samples=[];
  for(let p=0;p<6;p++)for(let i=0;i<450;i++){
    let x=.013,y=-.017;
    if(kind==='plausible'){
      if(p===1||p===4){x=.07*Math.cos(i*.07);y=.07*Math.sin(i*.07);}
      if(p===2||p===5){x=.78*Math.cos(i*.07);y=.78*Math.sin(i*.07);}
    }
    samples.push([p,p*15000+i*25,Math.round(x*10000)/10000,Math.round(y*10000)/10000,
      Math.round(x*10000)/10000,Math.round(y*10000)/10000]);
  }
  const doc={
    schema:'ga-remote-lab-v1',protocol:'GA-RL-01',session_id:uuid,
    controller:{family:'generic',condition:'unknown',transport:'usb'},
    client:{mapping:'standard',version:'0.1.0'},
    consent:{information_ack:true,upload:true},samples
  };
  assert.equal(validateSubmission(doc).ok,true);
  return doc;
}
test('quality metrics distinguish plausible self-report from static/fake sweep',()=>{
  const good=reviewSampleQuality(session().samples);
  assert.deepEqual(good.flags,[]);
  assert.equal(good.eligible_for_model_training,false);
  assert.equal(good.eligible_for_blind_holdout,false);
  assert.equal(good.manual_review_required,true);
  assert.equal(good.physical_release_intent_verified,false);
  assert.equal(good.production_gate,'BLOCKED');
  const bad=reviewSampleQuality(session({kind:'static'}).samples);
  assert.ok(bad.flags.includes('SWEEP_SIGNAL_STATIC_LEFT'));
  assert.ok(bad.flags.includes('SWEEP_QUADRANTS_MISSING_RIGHT'));
  assert.ok(bad.flags.includes('SMALL_MOTION_INDETERMINATE_LEFT'));
  assert.equal(bad.eligible_for_model_training,false);
});
test('read-only directory reviewer quarantines exact duplicated signals and bad files',async t=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'ga-review-offline-'));
  const input=path.join(root,'inputs');
  await fs.mkdir(input);
  t.after(()=>fs.rm(root,{recursive:true,force:true}));
  const a=session(), b=session({uuid:crypto.randomUUID()}),
    c=session({kind:'static'}), invalid=session();
  invalid.consent.upload=false;
  const save=async doc=>fs.writeFile(path.join(input,doc.session_id+'.json'),JSON.stringify(doc));
  await save(a);await save(b);await save(c);await save(invalid);
  const out=path.join(root,'report.json');
  const report=await reviewDirectory(input,out);
  assert.equal(report.session_count,4);
  assert.equal(report.unique_samples_seen,2);
  assert.equal(report.quarantined_count,2);
  assert.equal(report.no_auto_model_training,true);
  assert.equal(report.production_gate,'BLOCKED');
  assert.equal(report.sessions.filter(s=>s.result==='QUARANTINE_DUPLICATE').length,1);
  assert.equal(report.sessions.filter(s=>s.result==='QUARANTINE_INVALID_SCHEMA_OR_FILENAME').length,1);
  assert.ok(report.sessions.some(s=>s.quality_flags.includes('SWEEP_SIGNAL_STATIC_LEFT')));
  assert.ok(report.sessions.every(s=>s.allow_training===false));
  assert.ok(report.sessions.every(s=>s.allow_blind_holdout===false));
  assert.equal((await fs.readdir(input)).length,4);
  await assert.rejects(reviewDirectory(input,out),/EEXIST/);
  await assert.rejects(reviewDirectory(input,path.join(input,'bad.json')),/REPORT_OUTSIDE_INPUT_REQUIRED/);
});
