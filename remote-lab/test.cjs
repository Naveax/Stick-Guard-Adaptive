'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs/promises'),os=require('node:os'),path=require('node:path'),crypto=require('node:crypto');
const {createServer,valid}=require('./server.cjs');
function makeDoc(){
 const samples=[];
 for(let phase=0;phase<6;phase++)for(let i=0;i<100;i++){
   const radius=[.015,.07,.7,.015,.07,.7][phase],x=Math.round(radius*Math.cos(i*.12)*10000)/10000,y=Math.round(radius*Math.sin(i*.12)*10000)/10000;
   samples.push([phase,phase*12000+i*90,x,y,x,y]);
 }
 return {schema:'ga-remote-lab-v1',protocol:'GA-RL-01',session_id:crypto.randomUUID(),
 controller:{family:'xbox',condition:'drift',transport:'usb'},
 client:{mapping:'standard',version:'0.1.0'},
 consent:{information_ack:true,upload:true},samples};
}
test('strict schema and self-reported physical release never trusted',()=>{
 const x=makeDoc();assert.equal(valid(x),null);
 for(const condition of ['healthy','drift','severe']){
  const check=makeDoc();check.controller.condition=condition;
  assert.equal(valid(check),null,'GA-RL-01 category '+condition);
 }
 for(const fn of [o=>{o.controller.id='potential-fingerprint';},o=>{o.consent.upload=false;},
  o=>{o.samples[0][2]=999;},o=>{o.samples[2][1]=1;},o=>{o.samples[0][0]=5;},o=>{o.samples=o.samples.slice(0,30);}
 ]){const altered=makeDoc();fn(altered);assert.notEqual(valid(altered),null);}
});
test('HTTP consent / HTTPS Origin / administrator / erasure / no path traversal',async t=>{
 const folder=await fs.mkdtemp(path.join(os.tmpdir(),'ga-workpc-failover-test-'));
 const secret=crypto.randomBytes(32).toString('hex');
 const file=path.join(folder,'admin-token.txt');
 await fs.writeFile(file,secret,{flag:'wx',mode:0o600});
 const app=createServer({host:'127.0.0.1',port:0,origin:'https://nexowire.tail10f02d.ts.net:8443',
 tokenFile:file,dataDir:path.join(folder,'data'),uploads:true});
 const bound=await app.listen();const url='http://127.0.0.1:'+bound.port;
 t.after(async()=>{await new Promise(resolve=>app.server.close(resolve));await fs.rm(folder,{recursive:true,force:true});});
 async function post(doc,origin='https://nexowire.tail10f02d.ts.net:8443'){
   return fetch(url+'/api/submissions',{method:'POST',headers:{'Content-Type':'application/json',Origin:origin},body:JSON.stringify(doc)});
 }
 assert.equal((await fetch(url+'/')).status,200);
 assert.equal((await fetch(url+'/app.js')).status,200);
 assert.equal((await fetch(url+'/admin')).status,200);
 for(const alias of ['/test','/lab','/station']){
   const r=await fetch(url+alias,{redirect:'manual'});
   assert.equal(r.status,302);
   assert.equal(r.headers.get('location'),' /#station'.trim());
 }
 assert.equal((await fetch(url+'/index.html',{redirect:'manual'})).status,302);
 assert.equal((await fetch(url+'/favicon.ico')).status,204);
 assert.equal((await fetch(url+'/server.cjs')).status,404);
 assert.equal((await fetch(url+'/api/admin/sessions')).status,401);
 assert.equal((await post(makeDoc(),'https://attacker.example')).status,403);
 const x=makeDoc();assert.equal((await post({...x,consent:{information_ack:true,upload:false}})).status,422);
 const result=await post(x);assert.equal(result.status,201);assert.equal((await result.json()).production,'BLOCKED');
 assert.equal((await post(x)).status,409);
 const h={Authorization:'Bearer '+secret};
 const list=await fetch(url+'/api/admin/sessions',{headers:h});
 assert.equal((await list.json()).records.length,1);
 const response=await fetch(url+'/api/admin/sessions/'+x.session_id,{headers:h});
 assert.equal((await response.json()).samples.length,600);
 const del=await fetch(url+'/api/admin/sessions/'+x.session_id,{headers:h,method:'DELETE'});
 assert.equal(del.status,200);
 assert.equal((await fetch(url+'/api/admin/sessions/'+x.session_id,{headers:h})).status,404);
 assert.equal((await fs.readdir(path.join(folder,'data'))).length,0);
});
test('disabled uploads and unsafe host fail closed',async t=>{
 assert.throws(()=>createServer({host:'0.0.0.0',port:0}),/LOCALHOST_ONLY/);
 assert.throws(()=>createServer({host:'127.0.0.1',port:0,uploads:true}),/PUBLIC_ORIGIN_REQUIRED/);
 const app=createServer({host:'127.0.0.1',port:0});
 const bound=await app.listen();t.after(()=>new Promise(resolve=>app.server.close(resolve)));
 const url='http://127.0.0.1:'+bound.port;
 const r=await fetch(url+'/api/submissions',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(makeDoc())});
 assert.equal(r.status,503);
});

test('simple home contains exactly three choices and opens drift/filtered percentage test',async()=>{
 const html=await fs.readFile(path.join(__dirname,'public','index.html'),'utf8');
 const compare=await fs.readFile(path.join(__dirname,'public','compare.html'),'utf8');
 const js=await fs.readFile(path.join(__dirname,'public','app.js'),'utf8');
 for(const option of ['none','regular','severe']){
  assert.ok(html.includes('data-kind="'+option+'"'),'missing choice '+option);
 }
 assert.equal((html.match(/data-kind="/g)||[]).length,3);
 assert.match(html,/id="rawTotal"/);
 assert.match(html,/id="filteredTotal"/);
 assert.match(html,/id="fullTestBtn"/);
 assert.match(html,/id="surveyPrompt"/);
 assert.match(html,/id="surveyComment"/);
 assert.match(js,/navigator\.getGamepads/);
 assert.match(js,/\'\/api\/telemetry\'/);
 assert.match(compare,/option value="severe"/);
});
