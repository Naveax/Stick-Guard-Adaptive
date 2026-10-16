'use strict';
// GuardAdaptive Remote Lab GA-RL-02: isolated research preview, NOT the GuardAdaptive driver/model.
// Browser-only radial deadzone emulation never changes gamepad or game inputs.
// This module is deliberately independent of the HTTP server and any device APIs.
const FAMILIES=new Set(['xbox','playstation','nintendo','generic','other']);
const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const exact=(o,ks)=>o!==null&&typeof o==='object'&&!Array.isArray(o)&&Object.keys(o).length===ks.length&&ks.every(k=>Object.hasOwn(o,k));
const possible={
  use_context:new Set(['aim','steering','camera','menu','mixed','not_tried']),
  subjective_impact:new Set(['none','small','medium','severe','unplayable','unknown']),
  perceived_effect:new Set(['better','same','worse','not_tested','unsure']),
  micro_motion:new Set(['preserved','partly_lost','lost','not_tested','unsure']),
  recommendation:new Set(['yes','no','unsure','not_tested'])
};
const round=x=>Math.round(x*10000)/10000;
function soften(x,y,deadzone){
  const r=Math.hypot(x,y);
  if(r<=deadzone||r===0)return [0,0];
  const factor=Math.min(1,(r-deadzone)/(r*(1-deadzone)));
  return [round(x*factor),round(y*factor)];
}
function simulate(rows,dz){
  return rows.map(r=>{
    const l=soften(r[2],r[3],dz),right=soften(r[4],r[5],dz);
    return [r[0],r[1],...l,...right];
  });
}
function rms(rows,phase,x,y){
  let n=0,sum=0;
  for(const r of rows){if(r[0]!==phase)continue;n++;sum+=r[x]**2+r[y]**2;}
  return n?Math.sqrt(sum/n):null;
}
function metrics(before,after){
  const sides=[['left',0,1,2,3],['right',3,4,4,5]].map(([name,idle,small,x,y])=>{
    const idleB=rms(before,idle,x,y),idleA=rms(after,idle,x,y);
    const smallB=rms(before,small,x,y),smallA=rms(after,small,x,y);
    const reduction=idleB>=.005?100*(1-idleA/idleB):null;
    const motionLoss=smallB>=.01?100*(1-smallA/smallB):null;
    // Descriptive labels are cautious. A lower idle RMS alone proves nothing.
    let finding='BELIRSIZ';
    if(motionLoss!==null&&motionLoss>25)finding='KASITLI_HAREKET_KAYBI_RISKI';
    else if(reduction!==null&&reduction>=30&&motionLoss!==null&&motionLoss<=15)finding='OLASI_DENGE_SADECE_OLCUM';
    return {stick:name,without_idle_rms:round(idleB),with_idle_rms:round(idleA),
      idle_reduction_percent:reduction===null?null:round(reduction),
      without_small_motion_rms:round(smallB),with_small_motion_rms:round(smallA),
      intentional_motion_loss_percent:motionLoss===null?null:round(motionLoss),
      finding};
  });
  return {
    source:'GA-RL-02-unverified-browser-research',
    comparison_type:'simulation_or_participant_reported',
    stick_metrics:sides,physical_repair_proven:false,
    independent_ground_truth:false,manual_review_required:true,
    original_guardadaptive_model_used:false,game_input_modified:false,
    auto_training:false,production_gate:'BLOCKED'
  };
}
function validateRows(rows,controller){
  if(!Array.isArray(rows)||rows.length<480||rows.length>9000)return 'COUNT';
  let p=0,t=-1;const count=[0,0,0,0,0,0],first=[-1,-1,-1,-1,-1,-1],last=[-1,-1,-1,-1,-1,-1];
  for(const row of rows){
    if(!Array.isArray(row)||row.length!==6)return 'SHAPE';
    const phase=row[0],time=row[1];
    if(!Number.isInteger(phase)||phase<0||phase>5||phase<p||phase>p+1)return 'PHASE';
    if(!Number.isInteger(time)||time<0||time>180000||time<=t||(t>=0&&time-t>5000))return 'TIME';
    if(row.slice(2).some(v=>typeof v!=='number'||!Number.isFinite(v)||v< -1||v>1))return 'AXIS';
    p=phase;t=time;count[p]++;if(first[p]<0)first[p]=time;last[p]=time;
  }
  if(p!==5||t<48000||count.some(v=>v<80)||first.some((v,i)=>v<0||last[i]-v<8000||last[i]-v>22000))return 'INCOMPLETE';
  return null;
}
function validateStudy(v){
  if(!exact(v,['schema','protocol','session_id','controller','guard','before','after','survey','consent']))return 'STUDY_FIELDS';
  if(v.schema!=='ga-anti-drift-study-v1'||v.protocol!=='GA-RL-02'||typeof v.session_id!=='string'||!uuid.test(v.session_id))return 'PROTOCOL_OR_ID';
  if(!exact(v.controller,['family','condition','transport'])||
    !FAMILIES.has(v.controller.family)||!['drift','severe','healthy','unknown'].includes(v.controller.condition)||
    !['usb','bluetooth','unknown'].includes(v.controller.transport))return 'CONTROLLER';
  if(!exact(v.guard,['mode','deadzone']))return 'GUARD_FIELDS';
  if(!['simulation','manual_external'].includes(v.guard.mode))return 'GUARD_MODE';
  if(v.guard.mode==='simulation'){
    if(typeof v.guard.deadzone!=='number'||!Number.isFinite(v.guard.deadzone)||
      v.guard.deadzone<0||v.guard.deadzone>.25||Math.round(v.guard.deadzone*1000)/1000!==v.guard.deadzone||
      v.after!==null)return 'SIMULATED_PARAMETERS';
  }else if(v.guard.deadzone!==null)return 'EXTERNAL_PARAMETERS';
  if(!exact(v.consent,['research_ack','upload','self_report_ack'])||
    v.consent.research_ack!==true||v.consent.upload!==true||v.consent.self_report_ack!==true)return 'CONSENT';
  if(!exact(v.survey,['baseline_severity','use_context','subjective_impact','perceived_effect','micro_motion','recommendation']))return 'SURVEY_FIELDS';
  if(!Number.isInteger(v.survey.baseline_severity)||v.survey.baseline_severity<0||v.survey.baseline_severity>10||
    Object.entries(possible).some(([k,set])=>!set.has(v.survey[k])))return 'SURVEY_CHOICES';
  if(v.guard.mode==='simulation'&&v.survey.perceived_effect!=='not_tested')return 'SIMULATION_IS_NOT_A_REAL_EFFECT';
  if(v.guard.mode==='simulation'&&v.survey.micro_motion!=='not_tested')return 'SIMULATION_IS_NOT_A_REAL_MOTION_REPORT';
  const pre=validateRows(v.before,v.controller);
  if(pre)return 'BEFORE_'+pre;
  if(v.guard.mode==='manual_external'){
    if(!Array.isArray(v.after))return 'AFTER_REQUIRED';
    const post=validateRows(v.after,v.controller);
    if(post)return 'AFTER_'+post;
  }
  return null;
}
function analyzeStudy(study){
  const after=study.guard.mode==='simulation'?simulate(study.before,study.guard.deadzone):study.after;
  const comparison=metrics(study.before,after);
  comparison.comparison_type=study.guard.mode==='simulation'?'SAME_TRACE_VIRTUAL_RADIAL_DEADZONE':'TWO_SELF_REPORTED_SESSIONS';
  comparison.actual_guardactive_driver_tested=false;
  comparison.real_controller_behavior_proven=false;
  return {derived_after:after,comparison};
}
module.exports={soften,simulate,metrics,validateStudy,analyzeStudy};
