'use strict';
// Browser Gamepad API research only. No OS/controller/driver writes.
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const choice=(arr,v)=>arr.includes(v);
const exact=(x,keys)=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&
  Object.keys(x).length===keys.length&&keys.every(k=>Object.hasOwn(x,k));
const near=(a,b)=>Math.abs(a-b)<=0.00025;
const round=x=>Math.round(x*10000)/10000;
function filterPair(x,y,deadzone){
 const r=Math.hypot(x,y);
 if(r<=deadzone||r===0)return [0,0];
 const out=Math.min(1,(r-deadzone)/(1-deadzone))/r;
 return [round(x*out),round(y*out)];
}
function mapped(axes,indices,axesCount){
 const out=[];
 for(let i=0;i<4;i++)out.push(i>=axesCount?0:axes[indices[i]]);
 return out;
}
function filteredAxes(axes,indices,axesCount,dz){
 const a=mapped(axes,indices,axesCount);
 return [...filterPair(a[0],a[1],dz),...filterPair(a[2],a[3],dz)];
}
function validateTelemetry(d){
 if(!exact(d,['schema','session_id','condition','controller','filter','consent','samples','survey']))return 'TOP_LEVEL';
 if(d.schema!=='ga-controller-telemetry-v1'||typeof d.session_id!=='string'||!UUID.test(d.session_id))return 'SCHEMA';
 if(!choice(['none','regular','severe'],d.condition))return 'CONDITION';
 const c=d.controller;
 if(!exact(c,['axes_count','buttons_count','mapping','indices'])||
    !Number.isInteger(c.axes_count)||c.axes_count<2||c.axes_count>32||
    !Number.isInteger(c.buttons_count)||c.buttons_count<0||c.buttons_count>32||
    !choice(['standard','custom'],c.mapping)||
    !Array.isArray(c.indices)||c.indices.length!==4)return 'CONTROLLER';
 const capacity=c.axes_count>=4?4:2,active=c.indices.slice(0,capacity);
 if(active.some(x=>!Number.isInteger(x)||x<0||x>=c.axes_count)||
    new Set(active).size!==active.length||
    (capacity===2&&(c.indices[2]!==-1||c.indices[3]!==-1))||
    (c.mapping==='standard'&&(capacity!==4||c.indices.some((v,i)=>v!==i))))return 'AXES_MAPPING';
 const f=d.filter;
 if(!exact(f,['profile','deadzone'])||f.profile!=='radial-demo-v1'||
    !Number.isInteger(f.deadzone)||f.deadzone<0||f.deadzone>25)return 'FILTER';
 if(!exact(d.consent,['measurement_ack','upload'])||
    d.consent.measurement_ack!==true||d.consent.upload!==true)return 'NO_CONSENT';
 if(d.survey!==null){
  const s=d.survey;
  if(!exact(s,['benefit','interference','problem','affected','latency','rating','comment'])||
     !Number.isInteger(s.benefit)||s.benefit<0||s.benefit>5||
     !choice(['never','sometimes','often','unsure'],s.interference)||
     !choice(['none','center','small_move','diagonal','full_sweep','other'],s.problem)||
     !choice(['none','left','right','both'],s.affected)||
     !choice(['none','slight','high','unsure'],s.latency)||
     !Number.isInteger(s.rating)||s.rating<1||s.rating>5||
     typeof s.comment!=='string'||s.comment.length>500||
     /[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(s.comment))return 'SURVEY';
 }
 if(!Array.isArray(d.samples)||d.samples.length<720||d.samples.length>6000)return 'SAMPLES_COUNT';
 const count=Array(6).fill(0),first=Array(6).fill(-1),last=Array(6).fill(-1);
 let prior=-1,phase=0;
 const dz=f.deadzone/100;
 for(const row of d.samples){
  if(!Array.isArray(row)||row.length!==5)return 'ROW_SHAPE';
  const [p,t,axes,buttons,filtered]=row;
  if(!Number.isInteger(p)||p<phase||p>phase+1||p>5)return 'PHASE';
  if(!Number.isInteger(t)||t<=prior||t>180000||(prior>=0&&t-prior>6000))return 'TIMESTAMP';
  if(!Array.isArray(axes)||axes.length!==c.axes_count||
     axes.some(v=>typeof v!=='number'||!Number.isFinite(v)||v< -1||v>1))return 'RAW_AXES';
  if(!Array.isArray(buttons)||buttons.length!==c.buttons_count||
     buttons.some(v=>typeof v!=='number'||!Number.isFinite(v)||v<0||v>1))return 'BUTTONS';
  if(!Array.isArray(filtered)||filtered.length!==4||
     filtered.some(v=>typeof v!=='number'||!Number.isFinite(v)||v< -1||v>1))return 'FILTERED_SHAPE';
  const output=filteredAxes(axes,c.indices,capacity,dz);
  if(output.some((v,i)=>!near(v,filtered[i])))return 'FILTER_MISMATCH';
  if(first[p]===-1)first[p]=t;
  last[p]=t;count[p]++;phase=p;prior=t;
 }
 if(phase!==5||count.some(n=>n<100)||
    first.some((t,i)=>t<0||last[i]-t<8000||last[i]-t>18000))return 'PHASE_INCOMPLETE';
 return null;
}
function summarizeTelemetry(d){
 const axesCount=d.controller.axes_count>=4?4:2;
 const sides=[['left',0,0],['right',3,2]].map(([name,phase,offset])=>{
  const subset=d.samples.filter(s=>s[0]===phase);
  const raw=subset.map(s=>{
    const a=mapped(s[2],d.controller.indices,axesCount);
    return Math.hypot(a[offset],a[offset+1]);
  });
  const out=subset.map(s=>Math.hypot(s[4][offset],s[4][offset+1]));
  const rms=v=>v.length?round(Math.sqrt(v.reduce((s,x)=>s+x*x,0)/v.length)*100):null;
  return {stick:name,rest_raw_rms_percent:rms(raw),
    rest_filtered_rms_percent:rms(out),
    rest_peak_percent:raw.length?round(Math.min(1,Math.max(...raw))*100):null};
 });
 return {session_id:d.session_id,condition:d.condition,
  source:'browser_gamepad_api',sample_rate:'requestAnimationFrame_throttled_approx_20fps',
  sample_count:d.samples.length,axes_count:d.controller.axes_count,
  buttons_count:d.controller.buttons_count,filter_deadzone_percent:d.filter.deadzone,
  left:sides[0],right:sides[1],survey_completed:d.survey!==null,
  independent_ground_truth:false,device_identity_collected:false,
  actual_game_input_modified:false,auto_training:false,production_gate:'BLOCKED'};
}
module.exports={filterPair,filteredAxes,validateTelemetry,summarizeTelemetry};
