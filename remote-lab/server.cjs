'use strict';
// GuardAdaptive Remote Lab FAILOVER: independent Work PC build because Naveax is offline.
// Passive Gamepad API collection only. Never drive/modify controller inputs or models.
const http=require('node:http'),fs=require('node:fs'),fsp=require('node:fs/promises'),path=require('node:path'),crypto=require('node:crypto');
const {validateStudy,analyzeStudy}=require('./paired-study.cjs');
const {validateTelemetry,summarizeTelemetry}=require('./telemetry.cjs');
const ROOT=__dirname, PUBLIC=path.join(ROOT,'public');
const TYPES={'/':['index.html','text/html; charset=utf-8'],'/app.js':['app.js','text/javascript; charset=utf-8'],'/axis-direction-v94.js':['axis-direction-v94.js','text/javascript; charset=utf-8'],'/idle-quality.js':['idle-quality.js','text/javascript; charset=utf-8'],'/neutral-preview.js':['neutral-preview.js','text/javascript; charset=utf-8'],'/neutral-evidence-v87.js':['neutral-evidence-v87.js','text/javascript; charset=utf-8'],'/styles.css':['styles.css','text/css; charset=utf-8'],'/simple.css':['simple.css','text/css; charset=utf-8'],'/admin':['admin.html','text/html; charset=utf-8'],'/admin.js':['admin.js','text/javascript; charset=utf-8'],
'/compare':['compare.html','text/html; charset=utf-8'],
'/compare/':['compare.html','text/html; charset=utf-8'],
'/compare.js':['compare.js','text/javascript; charset=utf-8'],
'/compare.css':['compare.css','text/css; charset=utf-8'],
'/admin-studies':['admin-studies.html','text/html; charset=utf-8'],
'/admin-studies.js':['admin-studies.js','text/javascript; charset=utf-8'],
'/telemetry-admin':['telemetry-admin.html','text/html; charset=utf-8'],
'/telemetry-admin.js':['telemetry-admin.js','text/javascript; charset=utf-8']};
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const MAX_BYTES=850000, MAX_ROWS=9000;
function valid(doc){
  const exact=(x,keys)=>x&&typeof x==='object'&&!Array.isArray(x)&&Object.keys(x).length===keys.length&&keys.every(k=>Object.hasOwn(x,k));
  if(!exact(doc,['schema','protocol','session_id','controller','client','consent','samples'])||doc.schema!=='ga-remote-lab-v1'||doc.protocol!=='GA-RL-01'||typeof doc.session_id!=='string'||!UUID.test(doc.session_id))return 'SCHEMA';
  if(!exact(doc.controller,['family','condition','transport'])||!['xbox','playstation','nintendo','generic','other'].includes(doc.controller.family)||!['drift','severe','healthy','unknown'].includes(doc.controller.condition)||!['usb','bluetooth','unknown'].includes(doc.controller.transport))return 'CONTROLLER';
  if(!exact(doc.client,['mapping','version'])||doc.client.mapping!=='standard'||doc.client.version!=='0.1.0')return 'MAPPING';
  if(!exact(doc.consent,['information_ack','upload'])||doc.consent.information_ack!==true||doc.consent.upload!==true)return 'CONSENT';
  const arr=doc.samples;
  if(!Array.isArray(arr)||arr.length<480||arr.length>MAX_ROWS)return 'SAMPLE_COUNT';
  let previous=-1,stage=0;const counts=Array(6).fill(0),start=Array(6).fill(-1),last=Array(6).fill(-1);
  for(const row of arr){
    if(!Array.isArray(row)||row.length!==6)return 'ROW_SHAPE';
    const [p,t]=row;
    if(!Number.isInteger(p)||p<stage||p>stage+1||p>5)return 'PHASE_ORDER';
    if(!Number.isInteger(t)||t<0||t>180000||t<=previous||(previous>=0&&t-previous>5000))return 'TIME';
    if(row.slice(2).some(v=>typeof v!=='number'||!Number.isFinite(v)||v < -1||v > 1))return 'AXIS';
    if(start[p]<0)start[p]=t;
    last[p]=t;counts[p]++;previous=t;stage=p;
  }
  if(stage!==5||counts.some(n=>n<80)||start.some((s,i)=>s<0||last[i]-s<8000||last[i]-s>22000))return 'INCOMPLETE';
  return null;
}
function rms(arr,stage,i,j){
  const values=arr.filter(r=>r[0]===stage);
  if(!values.length)return null;
  return Math.round(Math.sqrt(values.reduce((sum,r)=>sum+r[i]**2+r[j]**2,0)/values.length)*10000)/10000;
}
function summary(doc){
  return {id:doc.session_id,controller:doc.controller,sample_count:doc.samples.length,
  left_idle_rms:rms(doc.samples,0,2,3),right_idle_rms:rms(doc.samples,3,4,5),
  physical_intent_verified:false,manual_review_required:true,production:'BLOCKED'};
}
function reply(res,status,data){
  const json=JSON.stringify(data);res.writeHead(status,{'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store','Content-Length':Buffer.byteLength(json)});res.end(json);
}
function headers(res){
  res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'");
  res.setHeader('X-Frame-Options','DENY');res.setHeader('X-Content-Type-Options','nosniff');res.setHeader('Referrer-Policy','no-referrer');
  res.setHeader('Permissions-Policy','gamepad=(self),camera=(),microphone=(),geolocation=()');
}
function readBody(req){
  return new Promise((resolve,reject)=>{
    const size=Number(req.headers['content-length']);
    if(req.headers['content-type']?.split(';')[0].trim()!=='application/json'||!Number.isSafeInteger(size)||size<2||size>MAX_BYTES){reject({status:413,code:'BODY_LIMIT'});return;}
    let n=0;const chunks=[];
    req.on('data',chunk=>{n+=chunk.length;if(n>MAX_BYTES||n>size){reject({status:413,code:'BODY_LIMIT'});req.destroy();return;}chunks.push(chunk);});
    req.on('end',()=>{if(n!==size){reject({status:400,code:'BODY_INCOMPLETE'});return;}try{resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')));}catch{reject({status:400,code:'BAD_JSON'});}});
    req.on('error',reject);
  });
}
function createServer(conf={}){
  const host=conf.host??'127.0.0.1',port=conf.port??Number(process.env.REMOTE_LAB_PORT??48927);
  const origin=conf.origin??process.env.REMOTE_LAB_PUBLIC_ORIGIN??'';
  const tokenFile=conf.tokenFile??process.env.REMOTE_LAB_ADMIN_TOKEN_FILE??'';
  const dataDir=conf.dataDir??process.env.REMOTE_LAB_DATA_DIR??path.join(ROOT,'data');
  const enabled=conf.uploads??(process.env.REMOTE_LAB_UPLOADS==='enabled');
  if(host!=='127.0.0.1'||!Number.isInteger(port)||port<0||port>65535)throw Error('LOCALHOST_ONLY');
  if(origin&&(!/^https:\/\/[a-z0-9.-]+(?::8443)?$/.test(origin)||new URL(origin).origin!==origin))throw Error('HTTPS_ORIGIN_REQUIRED');
  if(enabled&&!origin)throw Error('PUBLIC_ORIGIN_REQUIRED');
  let token='';
  if(tokenFile)token=fs.readFileSync(tokenFile,'utf8').trim();
  if(enabled&&token.length<32)throw Error('PRIVATE_ADMIN_TOKEN_REQUIRED');
  let queue=Promise.resolve(),successes=[];
  const studyDir=path.join(dataDir,'paired-v1');
  const telemetryDir=path.join(dataDir,'telemetry-v1');
  async function trimTelemetry(){
    await fsp.mkdir(telemetryDir,{recursive:true,mode:0o700});
    let count=0,size=0;
    for(const entry of await fsp.readdir(telemetryDir,{withFileTypes:true})){
      if(!entry.isFile()||!entry.name.endsWith('.json')||
         !UUID.test(entry.name.slice(0,-5)))continue;
      const filename=path.join(telemetryDir,entry.name),st=await fsp.stat(filename);
      if(Date.now()-st.mtimeMs>30*86400000){await fsp.unlink(filename);continue;}
      count++;size+=st.size;
    }
    return {count,size};
  }
  async function trimStudies(){
    await fsp.mkdir(studyDir,{recursive:true,mode:0o700});
    let count=0,size=0;
    for(const e of await fsp.readdir(studyDir,{withFileTypes:true})){
      if(!e.isFile()||!e.name.endsWith('.json')||!UUID.test(e.name.slice(0,-5)))continue;
      const file=path.join(studyDir,e.name),s=await fsp.stat(file);
      if(Date.now()-s.mtimeMs>30*86400000){await fsp.unlink(file);continue;}
      count++;size+=s.size;
    }
    return {count,size};
  }
  async function trim(){
    await fsp.mkdir(dataDir,{recursive:true,mode:0o700});
    let count=0,size=0;for(const e of await fsp.readdir(dataDir,{withFileTypes:true})){
      if(!e.isFile()||!UUID.test(e.name.replace(/\.json$/,''))||!e.name.endsWith('.json'))continue;
      const filename=path.join(dataDir,e.name),s=await fsp.stat(filename);
      if(Date.now()-s.mtimeMs>30*86400000){await fsp.unlink(filename);continue;}
      count++;size+=s.size;
    }return {count,size};
  }
  function admin(req){
    const h=req.headers.authorization??'';
    if(!token||!h.startsWith('Bearer '))return false;
    const a=Buffer.from(h.slice(7)),b=Buffer.from(token);
    return a.length===b.length&&crypto.timingSafeEqual(a,b);
  }
  const server=http.createServer(async(req,res)=>{
    headers(res);
    try{
      const u=req.url||'';
      // Human-facing aliases: a shared /test or /lab link must not show raw JSON 404.
      if(req.method==='GET'&&['/test','/test/','/lab','/lab/','/station','/station/'].includes(u)){
        res.writeHead(302,{'Location':'/#station','Cache-Control':'no-store'});res.end();return;
      }
      if(req.method==='GET'&&['/index.html','/home','/home/'].includes(u)){
        res.writeHead(302,{'Location':'/','Cache-Control':'no-store'});res.end();return;
      }
      if(req.method==='GET'&&u==='/favicon.ico'){
        res.writeHead(204,{'Cache-Control':'no-store'});res.end();return;
      }
      if(req.method==='GET'&&Object.hasOwn(TYPES,u)){
        const [name,type]=TYPES[u],b=await fsp.readFile(path.join(PUBLIC,name));
        res.writeHead(200,{'Content-Type':type,'Content-Length':b.length,'Cache-Control':'no-store'});res.end(b);return;
      }
      if(req.method==='GET'&&u==='/api/status'){
        reply(res,200,{schema:1,site:'GuardAdaptive Remote Lab Work-PC failover',uploads_enabled:enabled,telemetry_supported:true,retention_days:30,production:'BLOCKED',physical_intent_verified:false});return;
      }
      if(req.method==='POST'&&u==='/api/telemetry'){
        if(!enabled){reply(res,503,{error:'UPLOADS_DISABLED'});return;}
        if(req.headers.origin!==origin){reply(res,403,{error:'ORIGIN_DENIED'});return;}
        const doc=await readBody(req),reason=validateTelemetry(doc);
        if(reason){reply(res,422,{error:'INVALID_TELEMETRY',reason});return;}
        const run=queue.then(async()=>{
          const usage=await trimTelemetry(),now=Date.now();
          successes=successes.filter(t=>now-t<3600000);
          if(successes.length>=60){reply(res,429,{error:'GLOBAL_HOURLY_LIMIT'});return;}
          const result=summarizeTelemetry(doc);
          const blob=Buffer.from(JSON.stringify({...doc,analysis:result,
            independent_ground_truth:false,production_gate:'BLOCKED'}));
          if(usage.count>=500||usage.size+blob.length>256*1024*1024){
            reply(res,507,{error:'TELEMETRY_CAPACITY'});return;
          }
          const output=path.join(telemetryDir,doc.session_id+'.json');
          let file;
          try{
            file=await fsp.open(output,'wx',0o600);
            await file.writeFile(blob);await file.sync();
          }catch(e){
            reply(res,e.code==='EEXIST'?409:500,
              {error:e.code==='EEXIST'?'DUPLICATE':'WRITE_FAILED'});return;
          }finally{if(file)await file.close();}
          successes.push(now);
          reply(res,201,{accepted:true,receipt_id:doc.session_id,frames:doc.samples.length,
            survey_completed:doc.survey!==null,production_gate:'BLOCKED'});
        });
        queue=run.catch(()=>{});await run;return;
      }
      if(req.method==='POST'&&u==='/api/submissions'){
        if(!enabled){reply(res,503,{error:'UPLOADS_DISABLED'});return;}
        if(req.headers.origin!==origin){reply(res,403,{error:'ORIGIN_DENIED'});return;}
        const doc=await readBody(req),reason=valid(doc);
        if(reason){reply(res,422,{error:'INVALID_SESSION',reason});return;}
        const run=queue.then(async()=>{
          const usage=await trim(),now=Date.now();successes=successes.filter(t=>now-t<3600000);
          // Conservative global cap because a Tailscale Funnel origin may not
          // carry a trustworthy per-client IP.
          if(successes.length>=60){reply(res,429,{error:'GLOBAL_HOURLY_LIMIT'});return;}
          const data=Buffer.from(JSON.stringify(doc));
          if(usage.count>=2000||usage.size+data.length>256*1024*1024){reply(res,507,{error:'DATA_CAPACITY'});return;}
          let file;try{
            file=await fsp.open(path.join(dataDir,doc.session_id+'.json'),'wx',0o600);
            await file.writeFile(data);await file.sync();
          }catch(e){reply(res,e.code==='EEXIST'?409:500,{error:e.code==='EEXIST'?'DUPLICATE':'WRITE_FAILED'});return;}
          finally{if(file)await file.close();}
          successes.push(now);
          reply(res,201,{accepted:true,id:doc.session_id,samples:doc.samples.length,physical_intent_verified:false,production:'BLOCKED'});
        });
        queue=run.catch(()=>{});await run;return;
      }
      if(u==='/api/studies'&&req.method==='POST'){
        if(!enabled){reply(res,503,{error:'UPLOADS_DISABLED'});return;}
        if(req.headers.origin!==origin){reply(res,403,{error:'ORIGIN_DENIED'});return;}
        const study=await readBody(req),reason=validateStudy(study);
        if(reason){reply(res,422,{error:'INVALID_STUDY',reason});return;}
        const run=queue.then(async()=>{
          const usage=await trimStudies(),now=Date.now();
          successes=successes.filter(t=>now-t<3600000);
          if(successes.length>=60){reply(res,429,{error:'GLOBAL_HOURLY_LIMIT'});return;}
          if(usage.count>=500||usage.size>=256*1024*1024){reply(res,507,{error:'STUDY_CAPACITY'});return;}
          const {derived_after,comparison}=analyzeStudy(study);
          const archive={...study,derived_after,comparison,
            independent_hardware_ground_truth:false,source_version:'GA-RL-02',
            production_gate:'BLOCKED'};
          const blob=Buffer.from(JSON.stringify(archive));
          if(usage.size+blob.length>256*1024*1024){reply(res,507,{error:'STUDY_CAPACITY'});return;}
          let f;
          try{
            f=await fsp.open(path.join(studyDir,study.session_id+'.json'),'wx',0o600);
            await f.writeFile(blob);await f.sync();
          }catch(e){reply(res,e.code==='EEXIST'?409:500,{error:e.code==='EEXIST'?'DUPLICATE':'WRITE_FAILED'});return;}
          finally{if(f)await f.close();}
          successes.push(now);
          reply(res,201,{accepted:true,id:study.session_id,mode:study.guard.mode,
            before_frames:study.before.length,after_frames:derived_after.length,
            comparison,production_gate:'BLOCKED'});
        });
        queue=run.catch(()=>{});await run;return;
      }
      if(u==='/api/admin/telemetry'&&req.method==='GET'){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        await trimTelemetry();
        const items=[];
        for(const name of (await fsp.readdir(telemetryDir))
            .filter(n=>n.endsWith('.json')&&UUID.test(n.slice(0,-5))).slice(0,500)){
          try{
            const file=JSON.parse(await fsp.readFile(path.join(telemetryDir,name),'utf8'));
            items.push(file.analysis);
          }catch{}
        }
        reply(res,200,{count:items.length,items,production_gate:'BLOCKED'});return;
      }
      const telemetryMatch=/^\/api\/admin\/telemetry\/([0-9a-f-]{36})$/.exec(u);
      if(telemetryMatch&&['GET','DELETE'].includes(req.method)){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        if(!UUID.test(telemetryMatch[1])){reply(res,400,{error:'INVALID_ID'});return;}
        const filename=path.join(telemetryDir,telemetryMatch[1]+'.json');
        try{
          if(req.method==='DELETE'){await fsp.unlink(filename);reply(res,200,{deleted:true});}
          else{reply(res,200,JSON.parse(await fsp.readFile(filename,'utf8')));}
        }catch(e){reply(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'SERVER_ERROR'});}
        return;
      }
      if(u==='/api/admin/studies'&&req.method==='GET'){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        await trimStudies();
        const studies=[];
        for(const name of await fsp.readdir(studyDir)){
          if(!name.endsWith('.json')||!UUID.test(name.slice(0,-5)))continue;
          try{
            const file=JSON.parse(await fsp.readFile(path.join(studyDir,name),'utf8'));
            studies.push({id:file.session_id,mode:file.guard.mode,
              controller:file.controller,summary:file.comparison,
              survey:file.survey,before_frames:file.before.length,
              after_frames:file.derived_after.length,manual_review_required:true});
          }catch{}
        }
        reply(res,200,{studies,production_gate:'BLOCKED'});return;
      }
      const studyMatch=/^\/api\/admin\/studies\/([0-9a-f-]{36})$/.exec(u);
      if(studyMatch&&['GET','DELETE'].includes(req.method)){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        if(!UUID.test(studyMatch[1])){reply(res,400,{error:'INVALID_ID'});return;}
        const filename=path.join(studyDir,studyMatch[1]+'.json');
        try{
          if(req.method==='DELETE'){await fsp.unlink(filename);reply(res,200,{deleted:true});}
          else{const item=JSON.parse(await fsp.readFile(filename,'utf8'));reply(res,200,item);}
        }catch(e){reply(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'SERVER_ERROR'});}
        return;
      }
      if(u==='/api/admin/sessions'&&req.method==='GET'){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        await trim();const records=[];
        for(const n of await fsp.readdir(dataDir)){
          if(!n.endsWith('.json')||!UUID.test(n.slice(0,-5)))continue;
          try{const doc=JSON.parse(await fsp.readFile(path.join(dataDir,n),'utf8'));records.push(summary(doc));}catch{}
        }
        reply(res,200,{records});return;
      }
      const match=/^\/api\/admin\/sessions\/([0-9a-f-]{36})$/.exec(u);
      if(match&&['GET','DELETE'].includes(req.method)){
        if(!admin(req)){reply(res,401,{error:'AUTH_REQUIRED'});return;}
        if(!UUID.test(match[1])){reply(res,400,{error:'INVALID_ID'});return;}
        const fn=path.join(dataDir,match[1]+'.json');
        try{
          if(req.method==='DELETE'){await fsp.unlink(fn);reply(res,200,{deleted:true});}
          else{const item=JSON.parse(await fsp.readFile(fn,'utf8'));reply(res,200,item);}
        }catch(e){reply(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'SERVER_ERROR'});}
        return;
      }
      reply(res,404,{error:'NOT_FOUND'});
    }catch(e){if(!res.writableEnded&&!res.destroyed)reply(res,e.status||500,{error:e.code||'SERVER_ERROR'});}
  });
  server.requestTimeout=20000;server.headersTimeout=15000;
  const expireTimer=setInterval(()=>{trim().catch(()=>{});trimStudies().catch(()=>{});trimTelemetry().catch(()=>{});},3600000);
  expireTimer.unref();server.on('close',()=>clearInterval(expireTimer));
  return {server,listen:()=>new Promise((resolve,reject)=>server.listen(port,host,()=>resolve(server.address())).on('error',reject))};
}
if(require.main===module){const app=createServer();app.listen().then(a=>console.log('GA_WORKPC_REMOTE_LAB_READY http://'+a.address+':'+a.port)).catch(e=>{console.error(e.message);process.exitCode=1;});}
module.exports={createServer,valid,summary};
