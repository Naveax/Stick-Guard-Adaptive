'use strict';
// Strict, anonymous, opt-in *browser-only* anti-drift visual preview.
const crypto=require('node:crypto');
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const severities=new Set(['none','regular','severe']);
const interference=new Set(['never','sometimes','often']);
const problems=new Set(['none','center','small_move','diagonal','full_sweep']);
const affected=new Set(['none','left','right','both']);
const latency=new Set(['none','slight','high']);
const keysAre=(x,keys)=>x&&typeof x==='object'&&!Array.isArray(x)&&
  Object.keys(x).length===keys.length&&keys.every(key=>Object.hasOwn(x,key));
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const invalid=reason=>({ok:false,reason});
const PREVIEW_PROFILE='radial-deadzone-demo-0.12-v1';
const threshold=0.12;
function filterPair(x,y){
  const r=Math.hypot(x,y);
  if(r<=threshold)return [0,0];
  const gain=Math.min(1,(r-threshold)/(1-threshold))/r;
  return [Math.max(-1,Math.min(1,x*gain)),Math.max(-1,Math.min(1,y*gain))];
}
function validatePreview(p){
  if(!keysAre(p,['schema','session_id','controller','mapping','profile','consent','samples','survey']))
    return invalid('SCHEMA_FIELDS');
  if(p.schema!=='ga-preview-v1'||typeof p.session_id!=='string'||!UUID.test(p.session_id))
    return invalid('SCHEMA_OR_ID');
  if(!keysAre(p.controller,['severity'])||!severities.has(p.controller.severity))
    return invalid('CONTROLLER_CLASS');
  const m=p.mapping;
  if(!keysAre(m,['mode','indices','axes_available'])||
    !['standard','custom'].includes(m.mode)||
    ![2,4].includes(m.axes_available)||!Array.isArray(m.indices)||m.indices.length!==4)
    return invalid('MAPPING_SHAPE');
  const relevant=m.axes_available===4?4:2;
  const active=m.indices.slice(0,relevant);
  if(active.some(x=>!Number.isInteger(x)||x<0||x>=32)||
    new Set(active).size!==active.length||
    (relevant===2&&!m.indices.slice(2).every(x=>x===-1)))
    return invalid('AXIS_MAPPING');
  if(m.mode==='standard' && (relevant!==4||m.indices.some((x,i)=>x!==i)))
    return invalid('STANDARD_MAPPING_MISMATCH');
  if(p.profile!==PREVIEW_PROFILE)return invalid('UNKNOWN_PREVIEW_PROFILE');
  if(!keysAre(p.consent,['information_ack','upload'])||
     p.consent.information_ack!==true||p.consent.upload!==true)return invalid('CONSENT_REQUIRED');
  const q=p.survey;
  if(!keysAre(q,['benefit','interference','problem','affected','latency','rating'])||
    !Number.isInteger(q.benefit)||q.benefit<0||q.benefit>5||
    !interference.has(q.interference)||!problems.has(q.problem)||
    !affected.has(q.affected)||!latency.has(q.latency)||
    !Number.isInteger(q.rating)||q.rating<1||q.rating>5)
    return invalid('SURVEY_CHOICES');
  const samples=p.samples;
  if(!Array.isArray(samples)||samples.length<80||samples.length>1600)
    return invalid('SAMPLE_COUNT');
  let previous=-1;
  for(const row of samples){
    if(!Array.isArray(row)||row.length!==5||
      !Number.isInteger(row[0])||row[0]<0||row[0]>40_000||
      row[0]<=previous||(previous>=0&&row[0]-previous>2500)||
      row.slice(1).some(x=>!finite(x)||x< -1||x>1))return invalid('SAMPLE_VALUE');
    if(relevant===2&&(row[3]!==0||row[4]!==0))return invalid('MISSING_RIGHT_STICK');
    previous=row[0];
  }
  if(previous-samples[0][0]<8000)return invalid('PREVIEW_TOO_SHORT');
  return {ok:true};
}
function summary(p){
  let suppressed=0,meanResidual=0;
  for(const row of p.samples){
    const pair=filterPair(row[1],row[2]);
    const pairR=filterPair(row[3],row[4]);
    if((Math.hypot(row[1],row[2])>0&&Math.hypot(...pair)===0)||
       (Math.hypot(row[3],row[4])>0&&Math.hypot(...pairR)===0))suppressed++;
    meanResidual+=Math.hypot(...pair)+Math.hypot(...pairR);
  }
  return {session_id:p.session_id,reported_severity:p.controller.severity,
    axes_available:p.mapping.axes_available,sample_count:p.samples.length,
    benefit:p.survey.benefit,interference:p.survey.interference,
    problem:p.survey.problem,affected:p.survey.affected,latency:p.survey.latency,
    rating:p.survey.rating,suppressed_frames:suppressed,
    preview_only:true,actual_game_input_modified:false,
    physical_intent_verified:false,production_gate:'BLOCKED'};
}
module.exports={PREVIEW_PROFILE,filterPair,validatePreview,summary};
