'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),
 fs=require('node:fs/promises'),path=require('node:path'),os=require('node:os'),crypto=require('node:crypto');
const {createServer}=require('./server.cjs');
const {soften,simulate,metrics,validateStudy,analyzeStudy}=require('./paired-study.cjs');
function rows(){
 const out=[];
 for(let p=0;p<6;p++){
  for(let i=0;i<100;i++){
   const radius=p===0||p===3?.04:p===1||p===4?.075:.8;
   const x=Math.round(radius*Math.cos(i*.09)*10000)/10000;
   const y=Math.round(radius*Math.sin(i*.09)*10000)/10000;
   out.push([p,p*12000+i*90,x,y,x,y]);
  }
 }
 return out;
}
function doc(mode='simulation'){
 return {
  schema:'ga-anti-drift-study-v1',protocol:'GA-RL-02',session_id:crypto.randomUUID(),
  controller:{family:'generic',condition:'drift',transport:'usb'},
  guard:{mode,deadzone:mode==='simulation'?.05:null},
  before:rows(),after:mode==='manual_external'?rows():null,
  survey:{
   baseline_severity:6,use_context:'aim',subjective_impact:'medium',
   perceived_effect:'not_tested',micro_motion:'not_tested',
   recommendation:'unsure'
  },
  consent:{research_ack:true,upload:true,self_report_ack:true}
 };
}
test('virtual radial deadzone is deterministic, keeps axes finite and flags intentional motion loss',()=>{
 assert.deepEqual(soften(.01,.01,.05),[0,0]);
 assert.deepEqual(soften(0,0,.05),[0,0]);
 assert.deepEqual(soften(1,0,.05),[1,0]);
 const base=rows(),after=simulate(base,.05),m=metrics(base,after);
 assert.equal(after.length,base.length);
 assert.deepEqual(m.stick_metrics.map(x=>x.with_idle_rms),[0,0]);
 assert.ok(m.stick_metrics.every(x=>x.idle_reduction_percent>90));
 assert.ok(m.stick_metrics.every(x=>x.intentional_motion_loss_percent>25));
 assert.ok(m.stick_metrics.every(x=>x.finding==='KASITLI_HAREKET_KAYBI_RISKI'));
 assert.equal(m.original_guardadaptive_model_used,false);
 assert.equal(m.game_input_modified,false);
 assert.equal(m.production_gate,'BLOCKED');
});
test('GA-RL-02 strict categories, consent, simulated-vs-real labels and full phase completeness',()=>{
 const s=doc();
 assert.equal(validateStudy(s),null);
 for(const condition of ['healthy','drift','severe']){
  const check=doc();check.controller.condition=condition;
  assert.equal(validateStudy(check),null,'GA-RL-02 category '+condition);
 }
 for(const edit of [
  x=>{x.consent.upload=false;},x=>{x.controller.serial='secret';},
  x=>{x.survey.email='person@private.test';},
  x=>{x.survey.baseline_severity=20;},
  x=>{x.survey.perceived_effect='better';},
  x=>{x.after=rows();},
  x=>{x.guard.deadzone=.300;},
  x=>{x.before[0][2]=1.2;},
  x=>{x.before=x.before.slice(0,10);},
  x=>{x.survey.use_context='other with private notes';}
 ]){
  const bad=doc();edit(bad);
  assert.notEqual(validateStudy(bad),null);
 }
 const manual=doc('manual_external');assert.equal(validateStudy(manual),null);
 const noafter=doc('manual_external');noafter.after=null;
 assert.notEqual(validateStudy(noafter),null);
 const fake=doc('manual_external');fake.guard.deadzone=.01;
 assert.notEqual(validateStudy(fake),null);
 const verdict=analyzeStudy(manual);
 assert.equal(verdict.comparison.comparison_type,'TWO_SELF_REPORTED_SESSIONS');
 assert.equal(verdict.comparison.physical_repair_proven,false);
});
test('HTTP opt-in simulation, self-reported real pair, admin read/delete and isolated records',async t=>{
 const base=await fs.mkdtemp(path.join(os.tmpdir(),'guardadaptive-pair-test-'));
 const dataDir=path.join(base,'records'),secret=path.join(base,'private-token');
 const token=crypto.randomBytes(32).toString('hex');
 await fs.writeFile(secret,token,{flag:'wx',mode:0o600});
 const origin='https://lab.example.test:8443';
 const {server,listen}=createServer({host:'127.0.0.1',port:0,dataDir,tokenFile:secret,origin,uploads:true});
 const addr=await listen(),url='http://127.0.0.1:'+addr.port;
 t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(base,{recursive:true,force:true});});
 const get=route=>fetch(url+route);
 const post=(v,o=origin)=>fetch(url+'/api/studies',{
   method:'POST',headers:{Origin:o,'Content-Type':'application/json'},body:JSON.stringify(v)
 });
 const anon=await get('/api/admin/studies');
 assert.equal(anon.status,401);
 for(const route of ['/compare','/compare.js','/compare.css','/admin-studies','/admin-studies.js']){
  const res=await get(route);assert.equal(res.status,200,route);
 }
 assert.equal((await get('/paired-study.cjs')).status,404);
 assert.equal((await post(doc(),'https://untrusted.example')).status,403);
 const incorrect=doc();incorrect.survey.email='not permitted';
 assert.equal((await post(incorrect)).status,422);
 const simulated=doc();let res=await post(simulated);
 assert.equal(res.status,201);let receipt=await res.json();
 assert.equal(receipt.id,simulated.session_id);
 assert.equal(receipt.before_frames,600);assert.equal(receipt.after_frames,600);
 assert.equal(receipt.production_gate,'BLOCKED');
 assert.equal((await post(simulated)).status,409);
 const manual=doc('manual_external');res=await post(manual);
 assert.equal(res.status,201);
 const header={Authorization:'Bearer '+token};
 res=await fetch(url+'/api/admin/studies',{headers:header});
 assert.equal(res.status,200);
 const overview=await res.json();assert.equal(overview.studies.length,2);
 assert.ok(overview.studies.every(x=>x.manual_review_required===true));
 res=await fetch(url+'/api/admin/studies/'+simulated.session_id,{headers:header});
 const saved=await res.json();
 assert.equal(saved.before.length,600);assert.equal(saved.derived_after.length,600);
 assert.equal(saved.after,null);
 assert.equal(saved.survey.baseline_severity,6);
 assert.equal(saved.comparison.original_guardadaptive_model_used,false);
 assert.equal(saved.comparison.independent_ground_truth,false);
 assert.equal(saved.production_gate,'BLOCKED');
 assert.equal(Object.hasOwn(saved,'ip'),false);
 const listed=await fs.readdir(path.join(dataDir,'paired-v1'));
 assert.equal(listed.length,2);
 const raw=await fs.readFile(path.join(dataDir,'paired-v1',simulated.session_id+'.json'),'utf8');
 assert.doesNotMatch(raw,/controller_serial|email_address|user_agent/);
 assert.equal((await fetch(url+'/api/admin/studies/'+manual.session_id,{method:'DELETE',headers:header})).status,200);
 assert.equal((await fetch(url+'/api/admin/studies/'+simulated.session_id,{method:'DELETE',headers:header})).status,200);
 assert.deepEqual(await fs.readdir(path.join(dataDir,'paired-v1')),[]);
});
test('disabled collection never saves study records',async t=>{
 const base=await fs.mkdtemp(path.join(os.tmpdir(),'ga-pair-disabled-'));
 const {server,listen}=createServer({host:'127.0.0.1',port:0,dataDir:base,uploads:false});
 const addr=await listen();
 t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(base,{recursive:true,force:true});});
 const res=await fetch('http://127.0.0.1:'+addr.port+'/api/studies',
  {method:'POST',headers:{Origin:'https://lab.example.test','Content-Type':'application/json'},body:JSON.stringify(doc())});
 assert.equal(res.status,503);
});
