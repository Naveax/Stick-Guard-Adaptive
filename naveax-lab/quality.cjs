'use strict';
// Descriptive signals for self-reported browser gamepad sessions.
// NEVER validates physical stick release, user intent, or model safety.
// A machine-readable quality flag is not an authorization to train.
function round(v){return Math.round(v*10000)/10000;}
function phase(rows,idx,x,y){
  const sample=rows.filter(r=>r[0]===idx);
  if(!sample.length)return {count:0,rms:null,p95:null,max:null,sample_hz:null,max_gap_ms:null,quadrants:0,unique_pairs:0};
  let max=0,sum=0,maximumGap=0,last=null;
  const radii=[],unique=new Set(),quadrants=new Set();
  for(const row of sample){
    const px=row[x],py=row[y],m=Math.hypot(px,py);
    radii.push(m);sum+=m*m;max=Math.max(max,m);
    unique.add(px.toFixed(3)+','+py.toFixed(3));
    if(m>=0.35)quadrants.add((px>=0?1:0)+(py>=0?2:0));
    if(last!==null)maximumGap=Math.max(maximumGap,row[1]-last);
    last=row[1];
  }
  radii.sort((a,b)=>a-b);
  const duration=sample.at(-1)[1]-sample[0][1];
  return {
    count:sample.length,
    rms:round(Math.sqrt(sum/sample.length)),
    p95:round(radii[Math.ceil(sample.length*.95)-1]),
    max:round(max),
    sample_hz:duration>0?round((sample.length-1)*1000/duration):0,
    max_gap_ms:maximumGap,
    quadrants:quadrants.size,
    unique_pairs:unique.size
  };
}
function reviewSampleQuality(rows){
  const measurements=[
    phase(rows,0,2,3),phase(rows,1,2,3),phase(rows,2,2,3),
    phase(rows,3,4,5),phase(rows,4,4,5),phase(rows,5,4,5)
  ];
  const flags=[];
  for(let i=0;i<6;i++){
    const p=measurements[i];
    if(p.sample_hz<20)flags.push('LOW_SAMPLE_RATE_PHASE_'+i);
    if(p.max_gap_ms>400)flags.push('LARGE_FRAME_GAP_PHASE_'+i);
  }
  for(const [side,idle,small,sweep] of [
    ['LEFT',measurements[0],measurements[1],measurements[2]],
    ['RIGHT',measurements[3],measurements[4],measurements[5]]
  ]){
    if(sweep.max<0.5)flags.push('SWEEP_RADIUS_SMALL_'+side);
    if(sweep.quadrants<3)flags.push('SWEEP_QUADRANTS_MISSING_'+side);
    if(sweep.unique_pairs<6)flags.push('SWEEP_SIGNAL_STATIC_'+side);
    if(small.rms<=idle.rms+0.01)flags.push('SMALL_MOTION_INDETERMINATE_'+side);
  }
  return {
    schema:1,source:'GA-RL-01-unverified-browser-axis-quality',
    flags,flag_count:flags.length,
    phase_diagnostics:measurements,
    sample_protocol_complete:measurements.every(p=>p.count>=80),
    volunteer_instructions_observed_independently:false,
    browser_axes_are_raw_hardware_adc:false,
    signals_independently_verified:false,
    manual_review_required:true,
    eligible_for_model_training:false,
    eligible_for_blind_holdout:false,
    physical_release_intent_verified:false,
    production_gate:'BLOCKED'
  };
}
module.exports={reviewSampleQuality};
