'use strict';
/* Research-only local telemetry audit. Never emits raw axes, identities or survey comments. */
const fs=require('node:fs'),path=require('node:path');
const {validateTelemetry,summarizeTelemetry}=require('./telemetry.cjs');
const {checkIdle}=require('./public/idle-quality.js');
const STAGES=[['left',0,0],['left',1,0],['left',2,0],['right',3,2],['right',4,2],['right',5,2]];
const STORED_KEYS=['schema','session_id','condition','controller','filter','consent','samples','survey','analysis','independent_ground_truth','production_gate'];
const INPUT_KEYS=['schema','session_id','condition','controller','filter','consent','samples','survey'];
const SWEEP_EVIDENCE_MIN=0.075;
const CROSS_ACTIVITY_MAX=0.03;
function median(values){
 const a=[...values].sort((x,y)=>x-y),half=Math.floor(a.length/2);
 return a.length%2?a[half]:(a[half-1]+a[half])/2;
}
/* A phase-consistency hint, NOT physical direction verification or release proof.
 * Uses all axis values so wrongly selected or competing axes remain observable. */
function mappingEvidence(saved){
 const ix=saved.controller.indices,raw=saved.samples;
 const phases=Array.from({length:6},(_,p)=>raw.filter(r=>r[0]===p));
 const sides=saved.controller.axes_count>=4?2:1;
 const proof=[];
 for(let side=0;side<sides;side++){
  const neutral=side?3:0,sweep=side?5:2,opposite=side?2:5;
  const selected=ix.slice(side*2,side*2+2);
  const centers=Array.from({length:saved.controller.axes_count},(_,i)=>
   median(phases[neutral].map(row=>row[2][i])));
  const excursion=(axis,phase)=>
   Math.max(...phases[phase].map(row=>Math.abs(row[2][axis]-centers[axis])));
  const swept=selected.map(i=>excursion(i,sweep));
  const cross=selected.map(i=>excursion(i,opposite));
  // A lone spike must not masquerade as intentional axis exercise.
  const minActive=Math.max(12,Math.ceil(phases[sweep].length*0.05));
  const active=selected.map(i=>phases[sweep]
   .filter(row=>Math.abs(row[2][i]-centers[i])>=SWEEP_EVIDENCE_MIN).length);
  const noise=checkIdle(phases[neutral].map(row=>mapped(saved,row)),side);
  const extras=Array.from({length:saved.controller.axes_count},(_,i)=>i)
   .filter(i=>!ix.slice(0,sides*2).includes(i))
   .filter(i=>excursion(i,sweep)>=SWEEP_EVIDENCE_MIN).length;
  const status=!noise.valid?'NEUTRAL_UNSTABLE':
   swept.some((x,i)=>x<SWEEP_EVIDENCE_MIN||active[i]<minActive)?'SWEEP_NOT_DEMONSTRATED':
   cross.some(x=>x>CROSS_ACTIVITY_MAX)?'CROSS_ACTIVITY_IN_OTHER_STICK_PHASE':
   extras>0?'OTHER_UNMAPPED_AXIS_ACTIVE':
   'PHASE_ISOLATION_OBSERVED';
  proof.push({
   stick:side?'right':'left',axes:selected,
   sweep_max_pct:swept.map(x=>+(x*100).toFixed(2)),
   sweep_active_frames_per_axis:active,
   min_sustained_frames:minActive,
   cross_phase_max_pct:cross.map(x=>+(x*100).toFixed(2)),
   unselected_axes_active_during_sweep:extras,
   observational_status:status,physical_axis_direction_verified:false,
   confirmed_released:false
  });
 }
 return proof;
}
const mapped=(record,row)=>{
  const ix=record.controller.indices;
  return ix.map((v,i)=>i>=2 && record.controller.axes_count===2?0:row[2][v]);
};
function auditRecord(saved) {
 if(!saved||Array.isArray(saved)||typeof saved!=='object'||
    Object.keys(saved).length!==STORED_KEYS.length||
    STORED_KEYS.some(k=>!Object.hasOwn(saved,k)))return {ok:false,reason:'STORED_SHAPE_INVALID'};
 const payload=Object.fromEntries(INPUT_KEYS.map(k=>[k,saved[k]]));
 const validation=validateTelemetry(payload);
 if(validation!==null)return {ok:false,reason:'TELEMETRY_'+validation};
 if(saved.production_gate!=='BLOCKED'||saved.independent_ground_truth!==false||
   !saved.analysis||saved.analysis.production_gate!=='BLOCKED'||
   saved.analysis.independent_ground_truth!==false||
   saved.analysis.auto_training!==false||saved.analysis.actual_game_input_modified!==false)
   return {ok:false,reason:'UNSAFE_STORED_FLAGS'};
 const expected=summarizeTelemetry(payload);
 // Stored summary must agree with the immutable accepted signal (no forged stats).
 const keys=['session_id','condition','source','sample_rate','sample_count',
   'axes_count','buttons_count','filter_deadzone_percent','left','right'];
 for(const key of keys){
   if(JSON.stringify(saved.analysis[key])!==JSON.stringify(expected[key]))
     return {ok:false,reason:'ANALYSIS_MISMATCH'};
 }
 const samples=saved.samples,capacity=saved.controller.axes_count>=4?4:2;
 const outcome={
  ok:true,condition:saved.condition,
  controller_axes:saved.controller.axes_count,
  controller_buttons:saved.controller.buttons_count,
  mapping:saved.controller.mapping,
  indices:saved.controller.indices,
  phase_counts:[],
  sample_count:samples.length,
  neutral:[],micro_comparator:[],
  axis_activity:mappingEvidence(saved),
  duration_seconds:+((samples.at(-1)[1]-samples[0][1])/1000).toFixed(3),
  physical_release_verified:false,independent_ground_truth:false,
  eligible_for_training:false,eligible_for_blind_holdout:false,production_gate:'BLOCKED'
 };
 for(let phase=0;phase<6;phase++){
  const [side,expected,offset]=STAGES[phase];
  if(expected!==phase)throw Error('phase configuration error');
  const rows=samples.filter(s=>s[0]===phase);
  outcome.phase_counts.push(rows.length);
  const mapping=rows.map(r=>mapped(saved,r));
  const unique=new Set(mapping.map(a=>a.slice(offset,offset+2).join(','))).size;
  if(phase===0||phase===3){
   if(phase===3&&capacity===2)continue;
   const q=checkIdle(mapping,phase===0?0:1);
   outcome.neutral.push({side,stable_but_unverified:q.valid,
    quality:q.reason,center_observation_pct:q.observedCenterPct===undefined?null:+q.observedCenterPct.toFixed(3),
    max_deviation_pct:q.maxDeviationPct===undefined?null:+q.maxDeviationPct.toFixed(3),
    noise_rms_pct:q.noiseRmsPct===undefined?null:+q.noiseRmsPct.toFixed(3),
    distinct_positions:unique,constant_plateau_unverified:unique===1});
  }
  if(phase===1||phase===4){
   if(phase===4&&capacity===2)continue;
   let nonzero=0,zeroed=0;
   for(const row of rows){
    const a=mapped(saved,row);
    const rawMagnitude=Math.hypot(a[offset],a[offset+1]);
    const filteredMagnitude=Math.hypot(row[4][offset],row[4][offset+1]);
    if(rawMagnitude>0.0001){
     nonzero++;
     if(filteredMagnitude<=0.000001)zeroed++;
    }
   }
   outcome.micro_comparator.push({side,
    nonzero_samples:nonzero,zeroed_by_fixed_deadzone:zeroed,
    percent_zeroed:nonzero?+(100*zeroed/nonzero).toFixed(2):null,
    tested_deadzone_percent:saved.filter.deadzone,
    actual_intent_loss_verified:false});
  }
 }
 return outcome;
}
function summarizeDirectory(dir){
 const names=fs.readdirSync(dir).filter(s=>/^[0-9a-f-]{36}\.json$/.test(s));
 const summaries=[];
 for(const name of names){
  const file=path.join(dir,name),stat=fs.lstatSync(file);
  if(!stat.isFile()||stat.size>8_000_000||stat.size===0)throw Error('BAD_FILE_SIZE_OR_TYPE');
  const source=JSON.parse(fs.readFileSync(file,'utf8'));
  const audit=auditRecord(source);
  if(!audit.ok)throw Error('INVALID_SESSION:'+audit.reason);
  summaries.push({time:stat.mtime.toISOString(),audit});
 }
 summaries.sort((a,b)=>a.time.localeCompare(b.time));
 return summaries.map((x,i)=>({run:i+1,utc:x.time,...x.audit}));
}
function main(){
 const args=process.argv.slice(2);
 if(args.length!==2||args[0]!=='--data-dir')throw Error('Usage: node field-session-audit-v93.cjs --data-dir <PRIVATE_LOCAL_DIRECTORY>');
 const result=summarizeDirectory(args[1]);
 for(const item of result)console.log(JSON.stringify(item));
 console.log('TOTAL_RUNS='+result.length+' PHYSICAL_ACCEPTANCE=BLOCKED MODEL_TRAINING=BLOCKED');
}
if(require.main===module){try{main()}catch(error){console.error('AUDIT_REJECTED='+error.message);process.exitCode=2}}
module.exports={auditRecord,summarizeDirectory,mappingEvidence};
