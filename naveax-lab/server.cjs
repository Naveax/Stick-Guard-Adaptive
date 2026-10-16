'use strict';
// GuardAdaptive Remote Lab v0.85: opt-in gamepad-axis research collection.
// No controller commands, drivers, login, analytics, tracking, or outbound calls.
// Node 22 built-ins only. Bind localhost and use a TLS reverse proxy for the public site.
const http = require('node:http');
const net = require('node:net');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const { validateSubmission, summarize, MAX_JSON_BYTES } = require('./validate.cjs');
const { validatePreview, summary: previewSummary } = require('./preview.cjs');
const PUBLIC = path.join(__dirname, 'public');
const SIMPLE_PUBLIC = path.join(__dirname, 'public-simple');
const ASSETS = new Map([
  ['/', ['index.html','text/html; charset=utf-8']],
  ['/admin',['admin.html','text/html; charset=utf-8']],
  ['/styles.css',['styles.css','text/css; charset=utf-8']],
  ['/app.js',['app.js','text/javascript; charset=utf-8']],
  ['/idle-quality.js',['idle-quality.js','text/javascript; charset=utf-8']],
  ['/neutral-preview.js',['neutral-preview.js','text/javascript; charset=utf-8']],
  ['/neutral-evidence-v87.js',['neutral-evidence-v87.js','text/javascript; charset=utf-8']],
  ['/neutral-ui.js',['neutral-ui.js','text/javascript; charset=utf-8']],
  ['/admin.css',['admin.css','text/css; charset=utf-8']],
  ['/admin.js',['admin.js','text/javascript; charset=utf-8']]
]);
const RATE_WINDOW_MS = 60*60*1000;
const MAX_PER_HOUR = 8;
const MAX_FILES = 2000;
const MAX_TOTAL_BYTES = 256 * 1024 * 1024;
const RETENTION_MS = 30 * 24 * 60 * 60 * 1000;

function requireAdmin(req, token) {
  if (!token || token.length < 32) return false;
  const header = req.headers.authorization || '';
  if (!header.startsWith('Bearer ')) return false;
  const offered = Buffer.from(header.slice(7));
  const expected = Buffer.from(token);
  return offered.length === expected.length && crypto.timingSafeEqual(offered,expected);
}
function send(res, status, data, headers = {}) {
  const body = Buffer.from(JSON.stringify(data));
  res.writeHead(status, {
    'Content-Type':'application/json; charset=utf-8',
    'Content-Length':body.length,
    'Cache-Control':'no-store',
    ...headers
  });
  res.end(body);
}
function secureHeaders(res) {
  res.setHeader('X-Content-Type-Options','nosniff');
  res.setHeader('Referrer-Policy','no-referrer');
  res.setHeader('X-Frame-Options','DENY');
  res.setHeader('Permissions-Policy','gamepad=(self), camera=(), microphone=(), geolocation=()');
  res.setHeader('Cross-Origin-Resource-Policy','same-origin');
  res.setHeader('Cross-Origin-Opener-Policy','same-origin');
  res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; base-uri 'none'; object-src 'none'; form-action 'none'; frame-ancestors 'none'");
}
function allowedOrigin(req, publicOrigin='') {
  const origin=req.headers.origin;
  if(typeof origin!=='string')return false;
  if(publicOrigin)return origin===publicOrigin;
  // Development origin only: no arbitrary Host/header-controlled web origins.
  try {
    const url=new URL(origin);
    return url.protocol==='http:' && ['localhost','127.0.0.1','[::1]'].includes(url.hostname) &&
      url.host===req.headers.host && url.origin===origin;
  }catch{return false;}
}
function readJson(req) {
  return new Promise((resolve,reject) => {
    const len = Number(req.headers['content-length']);
    if(req.headers['content-type']?.split(';')[0].trim() !== 'application/json' ||
      !Number.isSafeInteger(len) || len < 2 || len > MAX_JSON_BYTES) {
      reject(Object.assign(new Error('JSON_CONTENT_LENGTH'),{status:413})); return;
    }
    let received = 0;
    const chunks=[];
    req.on('data',chunk => {
      received += chunk.length;
      if(received > MAX_JSON_BYTES || received > len){
        reject(Object.assign(new Error('BODY_TOO_LARGE'),{status:413}));
        req.destroy(); return;
      }
      chunks.push(chunk);
    });
    req.on('end',()=>{
      if(received !== len){reject(Object.assign(new Error('INCOMPLETE_BODY'),{status:400}));return;}
      try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))); }
      catch {reject(Object.assign(new Error('INVALID_JSON'),{status:400}));}
    });
    req.on('error',reject);
  });
}
async function pruneAndCount(dir){
  await fsp.mkdir(dir,{recursive:true,mode:0o700});
  let count=0, bytes=0;
  const now = Date.now();
  for(const entry of await fsp.readdir(dir,{withFileTypes:true})){
    if(!entry.isFile() || !/^[a-f0-9-]{36}\.json$/.test(entry.name)) continue;
    const f=path.join(dir,entry.name), s=await fsp.stat(f);
    if(now-s.mtimeMs > RETENTION_MS){await fsp.unlink(f);continue;}
    count++;bytes+=s.size;
  }
  return {count,bytes};
}
function startServer(options={}){
  const host=options.host ?? process.env.REMOTE_LAB_HOST ?? '127.0.0.1';
  const port=Number(options.port ?? process.env.REMOTE_LAB_PORT ?? 8787);
  const dataDir=path.resolve(options.dataDir ?? process.env.REMOTE_LAB_DATA_DIR ?? path.join(__dirname,'data'));
  const previewDir=path.join(dataDir,'preview-feedback');
  const uploadEnabled=options.uploadEnabled ?? (process.env.REMOTE_LAB_UPLOADS === 'enabled');
  const simpleUi=options.simpleUi ?? (process.env.REMOTE_LAB_SIMPLE_UI === 'enabled');
  const adminToken=options.adminToken ?? process.env.REMOTE_LAB_ADMIN_TOKEN ??
    (process.env.REMOTE_LAB_ADMIN_TOKEN_FILE ?
      fs.readFileSync(process.env.REMOTE_LAB_ADMIN_TOKEN_FILE,'utf8').trim() :
      (process.env.CREDENTIALS_DIRECTORY ?
        fs.readFileSync(path.join(process.env.CREDENTIALS_DIRECTORY,'admin_token'),'utf8').trim() : ''));
  const trustLoopbackProxy=options.trustLoopbackProxy ?? (process.env.REMOTE_LAB_TRUST_LOOPBACK_PROXY==='enabled');
  const trustCloudflareTunnel=options.trustCloudflareTunnel ?? (process.env.REMOTE_LAB_TRUST_CF_TUNNEL==='enabled');
  const publicOrigin=options.publicOrigin ?? process.env.REMOTE_LAB_PUBLIC_ORIGIN ?? '';
  if(host!=='127.0.0.1'&&host!=='::1')throw Error('LOOPBACK_BIND_ONLY');
  if(!Number.isInteger(port)||port<0||port>65535)throw Error('INVALID_LOCAL_PORT');
  if(uploadEnabled&&(!adminToken||adminToken.length<32))throw Error('STRONG_ADMIN_TOKEN_REQUIRED');
  if(publicOrigin){
    let u;
    try{u=new URL(publicOrigin);}catch{throw Error('INVALID_PUBLIC_ORIGIN');}
    if(u.origin!==publicOrigin||u.protocol!=='https:'||u.username||u.password||u.search||u.hash){
      throw Error('PUBLIC_ORIGIN_MUST_BE_HTTPS');
    }
  }
  if(trustLoopbackProxy&&trustCloudflareTunnel)throw Error('ONLY_ONE_TRUSTED_PROXY_MODE_ALLOWED');
  if(uploadEnabled&&(trustLoopbackProxy||trustCloudflareTunnel)&&!publicOrigin){
    throw Error('PUBLIC_HTTPS_ORIGIN_PIN_REQUIRED');
  }
  const rate=new Map();
  let writeQueue=Promise.resolve();
  function serialized(fn){
    const next=writeQueue.then(fn);
    writeQueue=next.catch(()=>{});
    return next;
  }
  function rateSource(req){
    const remote=req.socket.remoteAddress||'';
    if(trustLoopbackProxy||trustCloudflareTunnel){
      if(!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(remote))return null;
      const forwarded=trustCloudflareTunnel?req.headers['cf-connecting-ip']:req.headers['x-real-ip'];
      if(typeof forwarded!=='string'||net.isIP(forwarded)===0)return null;
      return forwarded;
    }
    return remote;
  }
  // Keep retention cleanup operating even when volunteers stop submitting.
  // No raw IP or request logs are written by this Node application.
  const retentionSweep=setInterval(()=>Promise.all(simpleUi?
    [pruneAndCount(dataDir),pruneAndCount(previewDir)]:[pruneAndCount(dataDir)]
    ).catch(()=>{}),60*60*1000);
  retentionSweep.unref();
  const requestServer=http.createServer(async (req,res)=>{
    secureHeaders(res);
    const url=req.url || '/';
    try {
      if(req.method === 'GET' && ASSETS.has(url)){
        const [file,type]=ASSETS.get(url);
        const root=simpleUi && ['/', '/styles.css', '/app.js', '/idle-quality.js', '/neutral-preview.js', '/neutral-evidence-v87.js', '/neutral-ui.js'].includes(url) ? SIMPLE_PUBLIC : PUBLIC;
        const f=path.join(root,file);
        const buf=await fsp.readFile(f);
        res.writeHead(200,{'Content-Type':type,'Content-Length':buf.length,
          'Cache-Control': 'no-store'});
        res.end(buf);return;
      }
      if(req.method==='GET' && url==='/api/status'){
        send(res,200,{schema:1,upload_enabled:uploadEnabled,local_export_available:true,
          browser_gamepad_support:'detect_on_client',max_upload_bytes:MAX_JSON_BYTES,
          retention_days:30, max_hourly_uploads:MAX_PER_HOUR,
          administered_online:false,preview_supported:simpleUi,
          preview_upload_enabled:simpleUi&&uploadEnabled});return;
      }
      if(req.method==='POST' && url==='/api/preview-feedback'){
        if(!simpleUi){send(res,404,{error:'NOT_FOUND'});return;}
        if(!uploadEnabled){send(res,503,{error:'UPLOADS_DISABLED',export_instead:true});return;}
        if(!allowedOrigin(req,publicOrigin)){send(res,403,{error:'ORIGIN_NOT_ALLOWED'});return;}
        if(!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(req.socket.remoteAddress||'')){
          send(res,403,{error:'DIRECT_NETWORK_DENIED'});return;
        }
        const source=rateSource(req);
        if(!source){send(res,403,{error:'TRUSTED_PROXY_CLIENT_REQUIRED'});return;}
        const payload=await readJson(req);
        const outcome=validatePreview(payload);
        if(!outcome.ok){send(res,422,{error:'INVALID_PREVIEW',detail:outcome.reason});return;}
        await serialized(async()=>{
          const ipKey=crypto.createHash('sha256').update(source).digest('hex');
          const now=Date.now(),previous=(rate.get(ipKey)||[]).filter(t=>now-t<RATE_WINDOW_MS);
          rate.set(ipKey,previous);
          if(previous.length>=MAX_PER_HOUR){send(res,429,{error:'RATE_LIMITED'});return;}
          const usage=await pruneAndCount(dataDir);
          const previewUsage=simpleUi?await pruneAndCount(previewDir):{count:0,bytes:0};
          const contents=Buffer.from(JSON.stringify(payload));
          if(usage.count+previewUsage.count>=MAX_FILES ||
             usage.bytes+previewUsage.bytes+contents.length>MAX_TOTAL_BYTES){
            send(res,507,{error:'DATA_CAPACITY_REACHED'});return;
          }
          // Prevent even cross-collection receipt ambiguity.
          if(fs.existsSync(path.join(dataDir,payload.session_id+'.json'))){
            send(res,409,{error:'DUPLICATE_SESSION'});return;
          }
          const file=path.join(previewDir,payload.session_id+'.json');
          let handle;
          try{
            handle=await fsp.open(file,'wx',0o600);
            await handle.writeFile(contents);await handle.sync();
          }catch(error){
            if(error.code==='EEXIST'){send(res,409,{error:'DUPLICATE_SESSION'});return;}
            if(handle)await fsp.unlink(file).catch(()=>{});
            throw error;
          }finally{if(handle)await handle.close();}
          previous.push(now);
          send(res,201,{ok:true,receipt_id:payload.session_id,
            frames:payload.samples.length,preview_only:true,
            actual_game_input_modified:false,production_gate:'BLOCKED'});
        });
        return;
      }
      if(req.method==='POST' && url==='/api/submissions'){
        if(!uploadEnabled){send(res,503,{error:'UPLOADS_DISABLED',export_instead:true});return;}
        if(!allowedOrigin(req,publicOrigin)){send(res,403,{error:'ORIGIN_NOT_ALLOWED'});return;}
        if(!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(req.socket.remoteAddress||'') &&
          host==='127.0.0.1') {send(res,403,{error:'DIRECT_NETWORK_DENIED'});return;}
        const source=rateSource(req);
        if(!source){send(res,403,{error:'TRUSTED_PROXY_CLIENT_REQUIRED'});return;}
        const payload=await readJson(req);
        const outcome=validateSubmission(payload);
        if(!outcome.ok){send(res,422,{error:'INVALID_SESSION',detail:outcome.reason});return;}
        await serialized(async()=>{
          // In-memory rate tracking only. For reverse proxies the IP must be set
          // by the trusted loopback proxy, never taken from arbitrary headers.
          const ipKey=crypto.createHash('sha256').update(source).digest('hex');
          const now=Date.now(), previous=(rate.get(ipKey)||[]).filter(t=>now-t<RATE_WINDOW_MS);
          rate.set(ipKey,previous);
          if(previous.length>=MAX_PER_HOUR){send(res,429,{error:'RATE_LIMITED'});return;}
          const usage=await pruneAndCount(dataDir);
          const previewUsage=simpleUi?await pruneAndCount(previewDir):{count:0,bytes:0};
          const contents=Buffer.from(JSON.stringify(payload));
          if(usage.count+previewUsage.count>=MAX_FILES ||
            usage.bytes+previewUsage.bytes+contents.length>MAX_TOTAL_BYTES){
            send(res,507,{error:'DATA_CAPACITY_REACHED'});return;
          }
          const fn=path.join(dataDir,payload.session_id+'.json');
          let handle;
          try{
            handle=await fsp.open(fn,'wx',0o600);
            await handle.writeFile(contents);await handle.sync();
          }catch(error){
            if(error.code==='EEXIST'){send(res,409,{error:'DUPLICATE_SESSION'});return;}
            if(handle){await fsp.unlink(fn).catch(()=>{});}
            throw error;
          }finally{if(handle)await handle.close();}
          previous.push(now);
          send(res,201,{ok:true,receipt_id:payload.session_id,
            frames:payload.samples.length,unverified_self_report:true,production_gate:'BLOCKED'});
        });
        return;
      }
      if(simpleUi && req.method==='GET' && url==='/api/admin/preview-feedback'){
        if(!requireAdmin(req,adminToken)){send(res,401,{error:'ADMIN_AUTH_REQUIRED'});return;}
        await pruneAndCount(previewDir);
        const items=[];
        for(const name of (await fsp.readdir(previewDir)).filter(n=>/^[a-f0-9-]{36}\.json$/.test(n)).slice(-1000)){
          try{
            const obj=JSON.parse(await fsp.readFile(path.join(previewDir,name),'utf8'));
            items.push(previewSummary(obj));
          }catch{}
        }
        send(res,200,{count:items.length,items});return;
      }
      const previewMatch=/^\/api\/admin\/preview-feedback\/([0-9a-f-]{36})$/.exec(url);
      if(simpleUi && previewMatch && ['GET','DELETE'].includes(req.method)){
        if(!requireAdmin(req,adminToken)){send(res,401,{error:'ADMIN_AUTH_REQUIRED'});return;}
        const file=path.join(previewDir,previewMatch[1]+'.json');
        if(req.method==='DELETE'){
          try{await fsp.unlink(file);send(res,200,{deleted:true,id:previewMatch[1]});}
          catch(e){send(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'DELETE_FAILED'});}
          return;
        }
        try{send(res,200,JSON.parse(await fsp.readFile(file,'utf8')));}
        catch(e){send(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'READ_FAILED'});}
        return;
      }
      if(url==='/api/admin/sessions' && req.method==='GET'){
        if(!requireAdmin(req,adminToken)){send(res,401,{error:'ADMIN_AUTH_REQUIRED'});return;}
        await pruneAndCount(dataDir);
        const all=await fsp.readdir(dataDir);
        const items=[];
        for(const name of all.filter(n=>/^[a-f0-9-]{36}\.json$/.test(n)).slice(-1000)){
          try {
            const content=JSON.parse(await fsp.readFile(path.join(dataDir,name),'utf8'));
            items.push(summarize(content));
          }catch{}
        }
        send(res,200,{count:items.length,items});return;
      }
      const match=/^\/api\/admin\/sessions\/([0-9a-f-]{36})$/.exec(url);
      if(match && ['GET','DELETE'].includes(req.method)){
        if(!requireAdmin(req,adminToken)){send(res,401,{error:'ADMIN_AUTH_REQUIRED'});return;}
        const f=path.join(dataDir,match[1]+'.json');
        if(req.method==='DELETE'){
          try{await fsp.unlink(f);send(res,200,{deleted:true,id:match[1]});}
          catch(e){send(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'DELETE_FAILED'});}
          return;
        }
        try{const doc=JSON.parse(await fsp.readFile(f,'utf8'));send(res,200,doc);}
        catch(e){send(res,e.code==='ENOENT'?404:500,{error:e.code==='ENOENT'?'NOT_FOUND':'READ_FAILED'});}
        return;
      }
      send(res,404,{error:'NOT_FOUND'});
    }catch(error){
      if(res.destroyed||res.writableEnded)return;
      const code=Number.isInteger(error.status)?error.status:500;
      send(res,code,{error:code===500?'INTERNAL_ERROR':error.message});
    }
  });
  requestServer.requestTimeout=20_000;
  requestServer.headersTimeout=15_000;
  requestServer.keepAliveTimeout=5_000;
  return new Promise((resolve,reject)=>{
    requestServer.once('error',error=>{clearInterval(retentionSweep);reject(error);});
    requestServer.on('close',()=>clearInterval(retentionSweep));
    requestServer.listen(port,host,()=>{
      // Startup health check; expiry is thereafter checked each hour.
      Promise.all(simpleUi?[pruneAndCount(dataDir),pruneAndCount(previewDir)]:
        [pruneAndCount(dataDir)]).catch(()=>{});
      resolve({server:requestServer,address:requestServer.address(),dataDir});
    });
  });
}
if(require.main===module){
 startServer().then(({address})=>console.log('GuardAdaptive Remote Lab local server: http://'+address.address+':'+address.port+
 '\nPublic hosting requires HTTPS reverse proxy. Uploads disabled unless REMOTE_LAB_UPLOADS=enabled.'))
 .catch(e=>{console.error(e.message);process.exitCode=1;});
}
module.exports={startServer,requireAdmin,allowedOrigin};
