'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs/promises'),path=require('node:path'),os=require('node:os'),crypto=require('node:crypto');
const {filterPair,filteredAxes,validateTelemetry,summarizeTelemetry}=require('./telemetry.cjs');
const {createServer}=require('./server.cjs');
function record(two=false){
 const axesCount=two?2:6,buttonsCount=3,indices=two?[1,0,-1,-1]:[4,5,2,3];
 const samples=[];
 for(let p=0;p<6;p++)for(let i=0;i<120;i++){
  const a=[.035, .025, .05, -.03, .045,-.037].slice(0,axesCount);
  const b=[0, i%2, .4];
  const f=filteredAxes(a,indices,two?2:4,.08);
  samples.push([p,p*13500+i*90,a,b,f]);
 }
 return {
  schema:'ga-controller-telemetry-v1',session_id:crypto.randomUUID(),
  condition:'regular',
  controller:{axes_count:axesCount,buttons_count:buttonsCount,
   mapping:'custom',indices},
  filter:{profile:'radial-demo-v1',deadzone:8},
  consent:{measurement_ack:true,upload:true},samples,
  survey:{benefit:3,interference:'sometimes',problem:'small_move',affected:'left',
   latency:'slight',rating:4,comment:'Küçük hareketlerde zorlandım.'}
 };
}
test('deterministic radial deadzone and unmodified raw data',()=>{
 assert.deepEqual(filterPair(.01,.01,.08),[0,0]);
 assert.deepEqual(filterPair(0,0,.08),[0,0]);
 assert.deepEqual(filterPair(1,0,.08),[1,0]);
 const r=record(),before=JSON.stringify(r.samples);
 assert.equal(validateTelemetry(r),null);
 assert.equal(JSON.stringify(r.samples),before);
 assert.equal(summarizeTelemetry(r).auto_training,false);
 assert.equal(summarizeTelemetry(r).actual_game_input_modified,false);
 const two=record(true);assert.equal(validateTelemetry(two),null);
 const skip=record();skip.survey=null;assert.equal(validateTelemetry(skip),null);
});
test('strict schema forbids fingerprints, forged filters, bad button data, comments and missing consent',()=>{
 for(const f of [
   x=>{x.controller.device_id='fingerprint';},x=>{x.ua='private';},
   x=>{x.consent.upload=false;},x=>{x.filter.deadzone=26;},
   x=>{x.samples[2][4][0]=.9;},x=>{x.samples[4][3][1]=2;},
   x=>{x.samples[3][2][1]=100;},x=>{x.samples[0][1]=99999;},
   x=>{x.samples=x.samples.slice(0,25);},x=>{x.survey.comment='x'.repeat(501);},
   x=>{x.survey.interference='nonsense';},
   x=>{x.controller.indices=[4,4,2,3];},
   x=>{x.samples[0].push('device identity');},
   x=>{x.filter.profile='GuardAdaptive-production';}
 ]){
  const r=record();f(r);assert.notEqual(validateTelemetry(r),null);
 }
});
test('HTTP opt-in telemetry is separate, audited, erasable and admin protected',async t=>{
 const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-telemetry-v1-'));
 const secret=crypto.randomBytes(32).toString('hex'),token=path.join(dir,'secret.txt');
 await fs.writeFile(token,secret,{mode:0o600});
 const origin='https://nexowire.tail10f02d.ts.net:8443';
 const app=createServer({port:0,host:'127.0.0.1',origin,tokenFile:token,uploads:true,
  dataDir:path.join(dir,'data')});
 const addr=await app.listen(),base='http://127.0.0.1:'+addr.port;
 t.after(async()=>{await new Promise(ok=>app.server.close(ok));await fs.rm(dir,{recursive:true,force:true});});
 const post=(d,o=origin)=>fetch(base+'/api/telemetry',{
  method:'POST',headers:{Origin:o,'Content-Type':'application/json'},body:JSON.stringify(d)});
 const st=await fetch(base+'/api/status');
 assert.equal((await st.json()).telemetry_supported,true);
 for(const route of ['/','/app.js','/simple.css','/telemetry-admin','/telemetry-admin.js'])assert.equal((await fetch(base+route)).status,200);
 assert.equal((await fetch(base+'/server.cjs')).status,404);
 assert.equal((await fetch(base+'/api/admin/telemetry')).status,401);
 assert.equal((await post(record(),'https://wrong.example')).status,403);
 const noConsent=record();noConsent.consent.upload=false;
 assert.equal((await post(noConsent)).status,422);
 const invalid=record();invalid.samples[0][4][0]=1;
 assert.equal((await post(invalid)).status,422);
 const doc=record(),accepted=await post(doc);
 assert.equal(accepted.status,201);
 assert.equal((await accepted.json()).receipt_id,doc.session_id);
 assert.equal((await post(doc)).status,409);
 const stored=path.join(dir,'data','telemetry-v1',doc.session_id+'.json');
 const payload=JSON.parse(await fs.readFile(stored,'utf8'));
 assert.equal(payload.samples.length,720);
 assert.equal(payload.survey.comment,'Küçük hareketlerde zorlandım.');
 assert.equal(payload.analysis.production_gate,'BLOCKED');
 assert.equal(Object.hasOwn(payload,'gamepad_id'),false);
 assert.equal((await fs.readdir(path.join(dir,'data','paired-v1')).catch(()=>[])).length,0);
 const headers={Authorization:'Bearer '+secret};
 const lst=await fetch(base+'/api/admin/telemetry',{headers});
 assert.equal(lst.status,200);
 assert.equal((await lst.json()).items.length,1);
 const read=await fetch(base+'/api/admin/telemetry/'+doc.session_id,{headers});
 assert.equal((await read.json()).samples.length,720);
 const del=await fetch(base+'/api/admin/telemetry/'+doc.session_id,{method:'DELETE',headers});
 assert.equal(del.status,200);
 assert.equal((await fs.readdir(path.join(dir,'data','telemetry-v1'))).length,0);
 assert.equal((await fetch(base+'/api/admin/telemetry/'+doc.session_id,{headers})).status,404);
});
