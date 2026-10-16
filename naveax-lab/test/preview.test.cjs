'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const crypto=require('node:crypto');
const os=require('node:os');
const path=require('node:path');
const fs=require('node:fs/promises');
const {startServer}=require('../server.cjs');
const {validatePreview,filterPair,PREVIEW_PROFILE}=require('../preview.cjs');
const {validateSubmission}=require('../validate.cjs');

function sample(){
  return {
    schema:'ga-preview-v1',session_id:crypto.randomUUID(),
    controller:{severity:'regular'},
    mapping:{mode:'custom',indices:[3,2,1,0],axes_available:4},
    profile:PREVIEW_PROFILE,
    consent:{information_ack:true,upload:true},
    samples:Array.from({length:400},(_,i)=>[i*50,.06,-.04,.22,-.22]),
    survey:{benefit:4,interference:'sometimes',problem:'small_move',affected:'left',
      latency:'slight',rating:3}
  };
}
function mutate(fn){const d=sample();fn(d);return d;}
test('preview radial filter is deterministic, bounded and centered',()=>{
  assert.deepEqual(filterPair(0.06,-0.06),[0,0]);
  assert.deepEqual(filterPair(0,0),[0,0]);
  assert.ok(Math.abs(filterPair(1,0)[0]-1)<1e-10);
  assert.ok(filterPair(.25,0)[0] < .25);
  assert.ok(filterPair(.25,0)[0] > 0);
  assert.deepEqual(validatePreview(sample()),{ok:true});
});
test('preview rejects unsolicited identifiers, forged consent and survey values',()=>{
  const bad=[
    v=>{v.controller.id='gamepad fingerprint';},
    v=>{v.user_agent='secret';},
    v=>{v.consent.upload=false;},
    v=>{v.survey.problem='arbitrary text';},
    v=>{v.survey.rating=6;},
    v=>{v.mapping.indices=[1,1,2,3];},
    v=>{v.samples[3][2]=2;},
    v=>{v.samples[0][0]=999;},
    v=>{v.samples=v.samples.slice(0,6);},
    v=>{v.profile='unverified-core-model';},
    v=>{v.mapping.mode='standard';},
    v=>{v.mapping.axes_available=2;v.mapping.indices=[3,2,-1,-1];},
    v=>{v.samples[1].push(0);},
  ];
  for(const fn of bad)assert.equal(validatePreview(mutate(fn)).ok,false);
  const two=mutate(v=>{
    v.mapping={mode:'custom',axes_available:2,indices:[1,0,-1,-1]};
    v.samples=v.samples.map(row=>[...row.slice(0,3),0,0]);
  });
  assert.equal(validatePreview(two).ok,true);
});
test('guided custom-mapped Gamepad API axes are validated without hardware identifiers',()=>{
  const samples=[];
  for(let p=0;p<6;p++)for(let i=0;i<100;i++)
    samples.push([p,p*9000+i*90,.01,-.01,.03,-.03]);
  const base={schema:'ga-remote-lab-v1',protocol:'GA-RL-01',
    session_id:crypto.randomUUID(),
    controller:{family:'generic',condition:'drift',transport:'unknown',severity:'severe'},
    client:{mapping:'custom',version:'0.1.0',axes:[4,5,2,3]},
    consent:{information_ack:true,upload:true},samples};
  assert.equal(validateSubmission(base).ok,true);
  base.client.axes=[4,4,2,3];
  assert.equal(validateSubmission(base).ok,false);
});
test('preview intake enforces mode/origin/auth/consent/duplicate and deletion',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-preview-tests-'));
  const secret=crypto.randomBytes(32).toString('hex');
  const main=await startServer({host:'127.0.0.1',port:0,adminToken:secret,
    simpleUi:true,uploadEnabled:true,dataDir:dir,publicOrigin:'https://lab.example.test'});
  const old=await startServer({host:'127.0.0.1',port:0,uploadEnabled:false,dataDir:path.join(dir,'old')});
  t.after(async()=>{
    await Promise.all([new Promise(r=>main.server.close(r)),new Promise(r=>old.server.close(r))]);
    await fs.rm(dir,{recursive:true,force:true});
  });
  const root='http://127.0.0.1:'+main.address.port;
  const post=(data,origin='https://lab.example.test')=>fetch(root+'/api/preview-feedback',{
    method:'POST',headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},
    body:JSON.stringify(data)});
  assert.equal((await(await fetch(root+'/api/status')).json()).preview_supported,true);
  const data=sample();
  assert.equal((await post(data,null)).status,403);
  assert.equal((await post(data,'https://evil.example.test')).status,403);
  assert.equal((await post({...data,consent:{information_ack:true,upload:false}})).status,422);
  assert.equal((await post({...data,location:'GPS'})).status,422);
  const saved=await post(data);
  assert.equal(saved.status,201);
  const receipt=await saved.json();
  assert.equal(receipt.receipt_id,data.session_id);
  assert.equal(receipt.actual_game_input_modified,false);
  assert.equal(receipt.production_gate,'BLOCKED');
  assert.equal((await post(data)).status,409);
  const records=path.join(dir,'preview-feedback');
  assert.equal((await fs.readdir(records)).length,1);
  const record=JSON.parse(await fs.readFile(path.join(records,data.session_id+'.json'),'utf8'));
  assert.equal(record.survey.interference,'sometimes');
  assert.equal(Object.hasOwn(record,'gamepad_id'),false);
  assert.equal((await fetch(root+'/api/admin/preview-feedback')).status,401);
  const authorized={Authorization:'Bearer '+secret};
  const list=await fetch(root+'/api/admin/preview-feedback',{headers:authorized});
  assert.equal(list.status,200);
  const info=await list.json();
  assert.equal(info.items.length,1);
  assert.equal(info.items[0].actual_game_input_modified,false);
  assert.equal(info.items[0].interference,'sometimes');
  const one=await fetch(root+'/api/admin/preview-feedback/'+data.session_id,{headers:authorized});
  assert.equal(one.status,200);
  assert.equal((await one.json()).samples.length,400);
  const deleted=await fetch(root+'/api/admin/preview-feedback/'+data.session_id,{method:'DELETE',headers:authorized});
  assert.equal(deleted.status,200);
  assert.equal((await fs.readdir(records)).length,0);
  assert.equal((await fetch(root+'/api/admin/preview-feedback/'+data.session_id,{headers:authorized})).status,404);
  const legacyRoot='http://127.0.0.1:'+old.address.port;
  assert.equal((await fetch(legacyRoot+'/api/preview-feedback',{
    method:'POST',headers:{Origin:legacyRoot,'Content-Type':'application/json'},
    body:JSON.stringify(sample())
  })).status,404);
});
test('preview and normal sessions share the successful-upload quota',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-preview-rate-'));
  const token=crypto.randomBytes(32).toString('hex');
  const {server,address}=await startServer({host:'127.0.0.1',port:0,
    dataDir:dir,uploadEnabled:true,adminToken:token,simpleUi:true});
  t.after(async()=>{await new Promise(r=>server.close(r));await fs.rm(dir,{recursive:true,force:true});});
  const base='http://127.0.0.1:'+address.port;
  const guided=()=>{
    const samples=[];
    for(let p=0;p<6;p++)for(let i=0;i<100;i++)
      samples.push([p,p*9000+i*90,.02,-.02,.04,-.04]);
    return {schema:'ga-remote-lab-v1',protocol:'GA-RL-01',
      session_id:crypto.randomUUID(),
      controller:{family:'generic',condition:'drift',transport:'unknown'},
      client:{mapping:'standard',version:'0.1.0'},
      consent:{information_ack:true,upload:true},samples};
  };
  const concurrent=await Promise.all(Array.from({length:10},(_,i)=>fetch(
    base+(i<5?'/api/preview-feedback':'/api/submissions'),{
      method:'POST',headers:{Origin:base,'Content-Type':'application/json'},
      body:JSON.stringify(i<5?sample():guided())
    })));
  assert.equal(concurrent.filter(r=>r.status===201).length,8);
  assert.equal(concurrent.filter(r=>r.status===429).length,2);
});
