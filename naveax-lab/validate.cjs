'use strict';
// Server-side schema and limits. User instructions are self-reported, never physical proof.
const {reviewSampleQuality}=require('./quality.cjs');
const MAX_JSON_BYTES = 850_000;
const PHASES = Object.freeze([
  'rest_left','small_left','sweep_left','rest_right','small_right','sweep_right'
]);
const FAMILIES = new Set(['xbox','playstation','nintendo','generic','other']);
const CONDITIONS = new Set(['drift','healthy','unknown']);
const SEVERITIES = new Set(['none','regular','severe']);
const TRANSPORTS = new Set(['usb','bluetooth','unknown']);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const keysAre = (v,keys) => v && !Array.isArray(v) && typeof v==='object' &&
  Object.keys(v).length===keys.length && keys.every(k=>Object.hasOwn(v,k));
const number = n => typeof n === 'number' && Number.isFinite(n);
const invalid = reason=>({ok:false,reason});
function validateSubmission(obj) {
  if(!keysAre(obj,['schema','protocol','session_id','controller','client','consent','samples'])) return invalid('SCHEMA_FIELDS');
  if(obj.schema!=='ga-remote-lab-v1' || obj.protocol!=='GA-RL-01') return invalid('UNKNOWN_PROTOCOL');
  if(typeof obj.session_id!=='string' || !UUID.test(obj.session_id))return invalid('BAD_SESSION_ID');
  const c=obj.controller;
  const legacy=keysAre(c,['family','condition','transport']);
  const classified=keysAre(c,['family','condition','transport','severity']);
  if((!legacy&&!classified) || !FAMILIES.has(c.family) || !CONDITIONS.has(c.condition) ||
    !TRANSPORTS.has(c.transport)) return invalid('BAD_CONTROLLER_CATEGORY');
  // Optional severity preserves old Cloudflare exports; a new class must be consistent.
  if(classified && (!SEVERITIES.has(c.severity) ||
    (c.severity==='none' && c.condition!=='healthy') ||
    (c.severity!=='none' && c.condition!=='drift'))) return invalid('BAD_DRIFT_SEVERITY');
  const client=obj.client;
  const standard=keysAre(client,['mapping','version'])&&client.mapping==='standard';
  const custom=keysAre(client,['mapping','version','axes'])&&client.mapping==='custom';
  if((!standard&&!custom)||client.version!=='0.1.0')
    return invalid('UNSUPPORTED_GAMEPAD_MAPPING');
  if(custom && (!Array.isArray(client.axes)||client.axes.length!==4||
    client.axes.some(i=>!Number.isInteger(i)||i<0||i>=32)||
    new Set(client.axes).size!==4))return invalid('INVALID_CUSTOM_AXES');
  if(!keysAre(obj.consent,['information_ack','upload']) ||
    obj.consent.information_ack!==true || obj.consent.upload!==true)return invalid('EXPLICIT_CONSENT_REQUIRED');
  const ss=obj.samples;
  if(!Array.isArray(ss) || ss.length<480 || ss.length>9000)return invalid('SAMPLE_COUNT');
  let phase=0,lastT=-1;
  const counts=new Array(6).fill(0), firstT=new Array(6).fill(-1),lastPhaseT=new Array(6).fill(-1);
  for(const row of ss){
    if(!Array.isArray(row) || row.length!==6)return invalid('SAMPLE_SHAPE');
    if(!Number.isInteger(row[0]) || row[0]<0 || row[0]>5 || row[0]<phase || row[0]>phase+1)return invalid('PHASE_ORDER');
    phase=row[0];
    const t=row[1];
    if(!Number.isInteger(t) || t<0 || t>180_000 || t<=lastT ||
      (lastT>=0 && t-lastT>5000))return invalid('SAMPLE_TIME');
    lastT=t;
    if(firstT[phase]<0)firstT[phase]=t;
    lastPhaseT[phase]=t;
    counts[phase]++;
    for(let j=2;j<6;j++){
      if(!number(row[j]) || row[j]<-1 || row[j]>1)return invalid('AXIS_OUT_OF_RANGE');
    }
  }
  if(phase!==5 || lastT<48_000 || counts.some(n=>n<80) ||
    firstT.some((start,j)=>start<0 || lastPhaseT[j]-start<8000 || lastPhaseT[j]-start>22_000)){
    return invalid('INCOMPLETE_GUIDED_PHASES');
  }
  return {ok:true};
}
function stats(rows,phase,ax,ay){
  const values=rows.filter(r=>r[0]===phase).map(r=>Math.hypot(r[ax],r[ay]));
  if(!values.length)return {count:0,rms:null,p95:null,max:null};
  const sorted=[...values].sort((a,b)=>a-b);
  const round=n=>Math.round(n*10000)/10000;
  return {count:values.length,
    rms:round(Math.sqrt(values.reduce((sum,v)=>sum+v*v,0)/values.length)),
    p95:round(sorted[Math.ceil(values.length*.95)-1]),max:round(sorted[sorted.length-1])};
}
function summarize(obj) {
  const ss=obj.samples;
  return {
    session_id:obj.session_id, family:obj.controller.family,
    reported_condition:obj.controller.condition,
    reported_severity:obj.controller.severity ?? null,
    transport:obj.controller.transport,
    phases:PHASES,
    sample_count:ss.length,
    elapsed_ms:ss.at(-1)[1]-ss[0][1],
    left_idle:stats(ss,0,2,3),
    right_idle:stats(ss,3,4,5),
    quality_flags:reviewSampleQuality(ss).flags,
    quality_review_required:true,
    // These are descriptive distributions, NOT measurements of physical intent
    physical_intent_verified:false,drift_model_accepted:false,production_gate:'BLOCKED'
  };
}
module.exports={MAX_JSON_BYTES,PHASES,validateSubmission,summarize};
