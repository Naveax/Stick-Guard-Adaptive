'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const crypto=require('node:crypto');
const fs=require('node:fs/promises');
const os=require('node:os');
const path=require('node:path');
const {startServer}=require('../server.cjs');
const {validateSubmission,summarize}=require('../validate.cjs');
function makeData() {
  const samples=[];
  for(let phase=0;phase<6;phase++)for(let i=0;i<100;i++){
    samples.push([phase,phase*9000+i*90,0.02,-0.01,0.03,-0.02]);
  }
  return {
    schema:'ga-remote-lab-v1',protocol:'GA-RL-01',
    session_id:crypto.randomUUID(),
    controller:{family:'xbox',condition:'drift',transport:'usb'},
    client:{mapping:'standard',version:'0.1.0'},
    consent:{information_ack:true,upload:true},samples
  };
}
test('simple three-button UI is isolated, severity is validated and stored',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-simple-ui-'));
  const token=crypto.randomBytes(32).toString('hex');
  const {server,address}=await startServer({host:'127.0.0.1',port:0,
    dataDir:dir,uploadEnabled:true,adminToken:token,simpleUi:true});
  t.after(async()=>{
    await new Promise(resolve=>server.close(resolve));
    await fs.rm(dir,{recursive:true,force:true});
  });
  const base='http://127.0.0.1:'+address.port;
  const html=await(await fetch(base+'/')).text();
  for(const kind of ['none','regular','severe']){
    assert.match(html,new RegExp('data-kind="'+kind+'"'));
  }
  assert.doesNotMatch(html,/CROWDSOURCED CONTROLLER RESEARCH/);
  assert.match(html,/Stick driftsiz kol/);
  assert.match(html,/Ağır stick driftli kol/);
  assert.match(html,/id="previewBtn"/);
  assert.match(html,/id="axisLX"/);
  assert.match(html,/id="previewConsent"/);
  assert.match(html,/id="interference"/);
  const simpleJs=await(await fetch(base+'/app.js')).text();
  assert.match(simpleJs,/severity:state.kind/);
  assert.match(simpleJs,/uploadConsent/);
  assert.match(simpleJs,/filterPair/);
  assert.match(simpleJs,/uploadPreview/);
  assert.doesNotMatch(simpleJs,/(navigator\.vibrate|sendReport\(|updateGamepad\(|virtualGamepad)/);
  const css=await fetch(base+'/styles.css');
  assert.equal(css.status,200);
  assert.match(await css.text(),/choice-grid/);
  const admin=await(await fetch(base+'/admin')).text();
  assert.match(admin,/id="adminToken"/);
  assert.match(admin,/id="previewRecordsBody"/);

  for(const [severity,condition] of [['none','healthy'],['regular','drift'],['severe','drift']]){
    const obj=makeData();
    obj.controller.condition=condition;
    obj.controller.severity=severity;
    assert.deepEqual(validateSubmission(obj),{ok:true});
    assert.equal(summarize(obj).reported_severity,severity);
    const res=await fetch(base+'/api/submissions',{method:'POST',headers:{
      Origin:base,'Content-Type':'application/json'},body:JSON.stringify(obj)});
    assert.equal(res.status,201);
    assert.equal((await res.json()).production_gate,'BLOCKED');
    const stored=JSON.parse(await fs.readFile(path.join(dir,obj.session_id+'.json'),'utf8'));
    assert.equal(stored.controller.severity,severity);
  }
  for(const [condition,severity] of [
    ['healthy','severe'],['drift','none'],['unknown','regular'],
    ['drift','unbounded']]){
    const obj=makeData();
    obj.controller.condition=condition;
    obj.controller.severity=severity;
    assert.equal(validateSubmission(obj).ok,false);
  }
  // Old three-field payloads remain valid for existing Cloudflare sessions.
  assert.deepEqual(validateSubmission(makeData()),{ok:true});
});

test('schema strictness, descriptive metrics and rejection of extra identifiers',()=>{
  const x=makeData();
  assert.deepEqual(validateSubmission(x),{ok:true});
  const s=summarize(x);
  assert.equal(s.physical_intent_verified,false);
  assert.ok(s.left_idle.rms>0);
  for(const edit of [
    v=>v.consent.upload=false,
    v=>{v.controller.raw_gamepad_id='Potential hardware fingerprint';},
    v=>{v.user_agent='secret';},
    v=>{v.samples[0][2]=NaN;},
    v=>{v.samples[0][3]=1.2;},
    v=>{v.samples[3][0]=5;},
    v=>{v.client.mapping='';},
    v=>{v.samples=v.samples.slice(0,2);},
    v=>{v.samples[2][1]=0;},
    v=>{v.samples[3].push(3);},
  ]){
    const bad=makeData();edit(bad);
    assert.equal(validateSubmission(bad).ok,false);
  }
});
test('real Node HTTP server: opt-in, origin, access, export and deletion',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-remote-lab-test-'));
  const secret=crypto.randomBytes(36).toString('hex');
  const {server,address}=await startServer({host:'127.0.0.1',port:0,dataDir:dir,uploadEnabled:true,adminToken:secret});
  t.after(async()=>{await new Promise(resolve=>server.close(resolve)); await fs.rm(dir,{recursive:true,force:true});});
  const base='http://127.0.0.1:'+address.port, origin=base;
  const get=async route=>fetch(base+route);
  const upload=async(data,org=origin)=>{
    const headers={'Content-Type':'application/json'};
    if(org)headers.Origin=org;
    return fetch(base+'/api/submissions',{method:'POST',headers,body:JSON.stringify(data)});
  };
  let res=await get('/');
  assert.equal(res.status,200);
  assert.match(await res.text(),/GUARDADAPTIVE/);
  const adminUi=await get('/admin');
  assert.equal(adminUi.status,200);
  assert.match(await adminUi.text(),/id="adminToken"/);
  assert.match(res.headers.get('content-security-policy'),/script-src 'self'/);
  assert.equal(res.headers.get('permissions-policy').includes('gamepad=(self)'),true);
  assert.equal((await(await get('/api/status')).json()).upload_enabled,true);
  assert.equal((await get('/server.cjs')).status,404);
  assert.equal((await get('/.env')).status,404);
  assert.equal((await get('/api/admin/sessions')).status,401);
  const data=makeData();
  assert.equal((await upload(data,null)).status,403);
  assert.equal((await upload(data,'https://evil.example')).status,403);
  assert.equal((await upload({...data,controller:{...data.controller,id:'123'}})).status,422);
  res=await upload(data);
  assert.equal(res.status,201);
  const receipt=await res.json();
  assert.equal(receipt.receipt_id,data.session_id);
  assert.equal(receipt.production_gate,'BLOCKED');
  assert.equal((await upload(data)).status,409);
  assert.equal((await get('/api/admin/sessions/'+data.session_id)).status,401);
  // An expired file must not appear in admin listings; retention deletes it.
  const stale=path.join(dir,crypto.randomUUID()+'.json');
  await fs.writeFile(stale,JSON.stringify(makeData()));
  const old=new Date(Date.now()-32*86400000);
  await fs.utimes(stale,old,old);
  res=await fetch(base+'/api/admin/sessions',{headers:{Authorization:'Bearer '+secret}});
  assert.equal(res.status,200);
  const list=await res.json();
  assert.equal(list.items.length,1);
  assert.equal(list.items[0].sample_count,600);
  assert.equal(list.items[0].reported_condition,'drift');
  assert.equal(Object.hasOwn(list.items[0],'user_agent'),false);
  assert.equal((await fs.readdir(dir)).length,1);
  const stored=JSON.parse(await fs.readFile(path.join(dir,data.session_id+'.json'),'utf8'));
  assert.equal(Object.hasOwn(stored,'gamepad_id'),false);
  assert.equal(Object.hasOwn(stored,'ip_address'),false);
  res=await fetch(base+'/api/admin/sessions/'+data.session_id,{headers:{Authorization:'Bearer '+secret}});
  assert.equal(res.status,200);
  assert.equal((await res.json()).samples.length,600);
  res=await fetch(base+'/api/admin/sessions/'+data.session_id,{
    method:'DELETE',headers:{Authorization:'Bearer '+secret}
  });
  assert.equal(res.status,200);
  assert.equal((await fs.readdir(dir)).length,0);
});
test('uploads remain disabled by default even for valid submissions',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-remote-disabled-'));
  const {server,address}=await startServer({host:'127.0.0.1',port:0,dataDir:dir,uploadEnabled:false});
  t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(dir,{force:true,recursive:true});});
  const base='http://127.0.0.1:'+address.port;
  const res=await fetch(base+'/api/submissions',{
    method:'POST',headers:{Origin:base,'Content-Type':'application/json'},body:JSON.stringify(makeData())
  });
  assert.equal(res.status,503);
  assert.equal((await res.json()).error,'UPLOADS_DISABLED');
  assert.equal((await fs.readdir(dir)).length,0);
});

test('trusted localhost reverse proxy identity and concurrent rate limit are fail-closed',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-remote-proxy-'));
  const {server,address}=await startServer({
    host:'127.0.0.1',port:0,dataDir:dir,uploadEnabled:true,
    trustLoopbackProxy:true,publicOrigin:'https://lab.example.test',adminToken:crypto.randomBytes(40).toString('hex')
  });
  t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(dir,{recursive:true,force:true});});
  const base='http://127.0.0.1:'+address.port;
  const post=(data,ip)=>fetch(base+'/api/submissions',{
    method:'POST',
    headers:{
      'Content-Type':'application/json',
      Origin:'https://lab.example.test',
      ...(ip?{'X-Real-IP':ip}:{})
    },
    body:JSON.stringify(data)
  });
  // Untrusted client IP is rejected; origin is independently pinned.
  assert.equal((await post(makeData(),undefined)).status,403);
  assert.equal((await post(makeData(),'totally-not-an-ip')).status,403);
  const concurrent=await Promise.all(Array.from({length:10},()=>post(makeData(),'203.0.113.12')));
  assert.equal(concurrent.filter(r=>r.status===201).length,8);
  assert.equal(concurrent.filter(r=>r.status===429).length,2);
  assert.equal((await post(makeData(),'203.0.113.13')).status,201);
  assert.equal((await fs.readdir(dir)).length,9);
});

test('deployment configuration is fail-closed on missing admin or HTTPS origin',async()=>{
  assert.throws(()=>startServer({
    host:'0.0.0.0',port:0,uploadEnabled:false
  }),/LOOPBACK_BIND_ONLY/);
  assert.throws(()=>startServer({
    host:'127.0.0.1',port:0,uploadEnabled:true,adminToken:'weak'
  }),/STRONG_ADMIN_TOKEN_REQUIRED/);
  assert.throws(()=>startServer({
    host:'127.0.0.1',port:0,uploadEnabled:true,adminToken:'X'.repeat(48),
    trustLoopbackProxy:true
  }),/PUBLIC_HTTPS_ORIGIN_PIN_REQUIRED/);
  assert.throws(()=>startServer({
    host:'127.0.0.1',port:0,uploadEnabled:true,adminToken:'X'.repeat(48),
    trustLoopbackProxy:true,publicOrigin:'http://lab.example.test'
  }),/PUBLIC_ORIGIN_MUST_BE_HTTPS/);
});
test('public HTTPS Origin pin rejects a forged HTTP localhost page',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-remote-origin-'));
  const {server,address}=await startServer({
    host:'127.0.0.1',port:0,dataDir:dir,uploadEnabled:true,
    adminToken:crypto.randomBytes(32).toString('hex'),
    publicOrigin:'https://lab.example.test'
  });
  t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(dir,{recursive:true,force:true});});
  const base='http://127.0.0.1:'+address.port;
  const request=origin=>fetch(base+'/api/submissions',{
    method:'POST',headers:{'Content-Type':'application/json',Origin:origin},
    body:JSON.stringify(makeData())
  });
  assert.equal((await request(base)).status,403);
  assert.equal((await request('https://another.example.test')).status,403);
  assert.equal((await request('https://lab.example.test')).status,201);
});

test('Ubuntu deployment files are isolated from VPN and uploads default off',async()=>{
  const dir=path.join(__dirname,'..','deploy');
  const read=async name=>fs.readFile(path.join(dir,name),'utf8');
  const service=await read('guardadaptive-remote-lab.service');
  const env=await read('service.env.example');
  const nginx=await read('nginx-site.example.conf');
  const caddy=await read('Caddyfile.example');
  const instructions=await read('README.md');
  const preflight=await read('check-ubuntu-prereqs.sh');
  assert.match(service,/DynamicUser=yes/);
  assert.match(service,/ProtectSystem=strict/);
  assert.match(service,/NoNewPrivileges=yes/);
  assert.match(service,/LoadCredential=admin_token:/);
  assert.match(service,/REMOTE_LAB_HOST=127\.0\.0\.1/);
  assert.match(service,/REMOTE_LAB_PORT=48927/);
  assert.match(env,/REMOTE_LAB_UPLOADS=disabled/);
  assert.match(env,/REMOTE_LAB_PUBLIC_ORIGIN=https:/);
  assert.match(nginx,/proxy_pass http:\/\/127\.0\.0\.1:48927/);
  assert.match(caddy,/reverse_proxy 127\.0\.0\.1:48927/);
  assert.match(instructions,/NOT DEPLOYED/);
  assert.match(preflight,/Read-only Ubuntu preflight/);
  assert.doesNotMatch(service,/ExecStart=.*(xray|nginx|vpn|ufw|iptables|sudo)/i);
  assert.doesNotMatch(preflight,/\b(sudo|apt install|systemctl restart|systemctl stop|systemctl enable|rm -rf|ufw allow)\b/);
});

test('Cloudflare localhost connector requires HTTPS pin and trusted CF-Connecting-IP',async t=>{
  const dir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-remote-cf-'));
  const token=crypto.randomBytes(32).toString('hex');
  assert.throws(()=>startServer({host:'127.0.0.1',port:0,uploadEnabled:true,adminToken:token,
    trustCloudflareTunnel:true}),/PUBLIC_HTTPS_ORIGIN_PIN_REQUIRED/);
  assert.throws(()=>startServer({host:'127.0.0.1',port:0,uploadEnabled:true,adminToken:token,
    trustCloudflareTunnel:true,trustLoopbackProxy:true,publicOrigin:'https://lab.example.test'}),
    /ONLY_ONE_TRUSTED_PROXY_MODE_ALLOWED/);
  const {server,address}=await startServer({host:'127.0.0.1',port:0,dataDir:dir,
    uploadEnabled:true,adminToken:token,trustCloudflareTunnel:true,
    publicOrigin:'https://lab.example.test'});
  t.after(async()=>{await new Promise(resolve=>server.close(resolve));await fs.rm(dir,{recursive:true,force:true});});
  const url='http://127.0.0.1:'+address.port+'/api/submissions';
  const send=(cf,origin='https://lab.example.test')=>fetch(url,{method:'POST',headers:{
    'Content-Type':'application/json',Origin:origin,...(cf?{'CF-Connecting-IP':cf}:{})
  },body:JSON.stringify(makeData())});
  assert.equal((await send(undefined)).status,403);
  assert.equal((await send('malformed')).status,403);
  assert.equal((await send('203.0.113.10','https://evil.example.test')).status,403);
  const sent=await send('203.0.113.10');
  assert.equal(sent.status,201);
  assert.equal((await sent.json()).production_gate,'BLOCKED');
  assert.equal((await fs.readdir(dir)).length,1);
});

test('administrator token can load from a private local file, not web assets',async t=>{
  const baseDir=await fs.mkdtemp(path.join(os.tmpdir(),'ga-admin-token-'));
  const tokenFile=path.join(baseDir,'key.txt');
  const dataDir=path.join(baseDir,'records');
  const key=crypto.randomBytes(32).toString('hex');
  await fs.writeFile(tokenFile,key,{mode:0o600,flag:'wx'});
  const previous=process.env.REMOTE_LAB_ADMIN_TOKEN_FILE;
  process.env.REMOTE_LAB_ADMIN_TOKEN_FILE=tokenFile;
  t.after(async()=>{
    if(previous===undefined)delete process.env.REMOTE_LAB_ADMIN_TOKEN_FILE;
    else process.env.REMOTE_LAB_ADMIN_TOKEN_FILE=previous;
    await fs.rm(baseDir,{recursive:true,force:true});
  });
  const {server,address}=await startServer({host:'127.0.0.1',port:0,dataDir,uploadEnabled:true,
    trustCloudflareTunnel:true,publicOrigin:'https://lab.example.test'});
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const origin='http://127.0.0.1:'+address.port;
  const unauth=await fetch(origin+'/api/admin/sessions');
  assert.equal(unauth.status,401);
  const auth=await fetch(origin+'/api/admin/sessions',{headers:{Authorization:'Bearer '+key}});
  assert.equal(auth.status,200);
  const secretAttempt=await fetch(origin+'/key.txt');
  assert.equal(secretAttempt.status,404);
});

test('stable Windows Funnel watchdog is least-privileged, pinned and non-invasive',async()=>{
  const dir=path.join(__dirname,'..','deploy','windows');
  const read=async name=>fs.readFile(path.join(dir,name),'utf8');
  const start=await read('Start-GuardAdaptive-Stable.ps1');
  const watch=await read('Watch-GuardAdaptive-Stable.ps1');
  const register=await read('Register-GuardAdaptive-StableTask.ps1');
  const docs=await read('README.md');
  const init=await read('Initialize-GuardAdaptive-StableStorage.ps1');
  assert.match(init,/GuardAdaptiveRemoteLabStable/);
  assert.match(init,/RandomNumberGenerator/);
  assert.match(init,/FileMode]::CreateNew/);
  assert.match(init,/icacls\.exe/);
  assert.match(start,/GuardAdaptiveRemoteLabStable/);
  assert.match(docs,/separate secret and separate submission directory/);
  assert.match(start,/REMOTE_LAB_SIMPLE_UI='enabled'/);
  assert.match(start,/REMOTE_LAB_HOST='127\.0\.0\.1'/);
  assert.match(start,/\$port=48928/);
  assert.match(start,/REMOTE_LAB_PORT="\$port"/);
  assert.match(start,/REMOTE_LAB_PUBLIC_ORIGIN=\$origin/);
  assert.match(start,/REMOTE_LAB_ADMIN_TOKEN_FILE=\$keyFile/);
  assert.match(start,/REMOTE_LAB_TRUST_CF_TUNNEL='disabled'/);
  assert.match(start,/Refusing to replace automatically/);
  assert.match(start,/Get-NetTCPConnection/);
  assert.match(watch,/Mutex/);
  assert.match(watch,/Start-Sleep -Seconds 60/);
  assert.match(register,/New-ScheduledTaskPrincipal/);
  assert.match(register,/RunLevel Limited/);
  assert.match(register,/LogonType Interactive/);
  assert.match(register,/PreviewOnly/);
  assert.match(docs,/not itself prove access from a client outside that tailnet/i);
  assert.doesNotMatch(start,/Stop-Process|New-NetFirewallRule|Set-NetFirewallRule|tailscale funnel --bg|Set-ExecutionPolicy/i);
  assert.doesNotMatch(watch,/Stop-Process|Set-NetFirewallRule|tailscale funnel --bg/i);
  assert.doesNotMatch(register,/RunLevel Highest|LogonType ServiceAccount|Set-NetFirewallRule|Stop-Service/i);
});
