'use strict';
// v0.87 self-reported, anonymous calibration evidence. Never a physical-truth label.
(function(root,factory){
 const api=Object.freeze(factory(root.GuardAdaptiveNeutralPreview));
 if(typeof module!=='undefined'&&module.exports)module.exports=api;
 else root.GuardAdaptiveEvidenceV87=api;
})(typeof window!=='undefined'?window:{},function(neutral){
 const bounded=rows=>Array.isArray(rows)&&rows.length>=35&&rows.length<=4096&&
  rows.every(r=>Array.isArray(r)&&r.length===4&&r.every(v=>typeof v==='number'&&Number.isFinite(v)&&Math.abs(v)<=1));
 function build({first,second,moved,axesCount}){
  if(!neutral||typeof neutral.review!=='function'||![2,4].includes(axesCount)||
     !bounded(first)||!bounded(second)||!Array.isArray(moved)||moved.length!==2)
   return {ok:false,reason:'INVALID_EVIDENCE'};
  const review=neutral.review(first,second,moved,axesCount);
  if(!review.ok)return {ok:false,reason:review.reason};
  const copy=rows=>rows.map(r=>r.map(v=>+v.toFixed(6)));
  const data={
   schema:'ga-neutral-evidence-v87',
   data_origin:'VOLUNTEER_SELF_REPORTED_NEUTRAL',
   physically_verified:false,
   independent_motion_ground_truth:false,
   eligible_for_training:false,
   eligible_for_holdout:false,
   production_gate:'BLOCKED',
   columns:['lx','ly','rx','ry'],
   axes_count:axesCount,
   user_instructed_two_releases:true,
   excursions_observed:[!!moved[0],!!moved[1]],
   first_window:copy(first),
   second_window:copy(second),
   observed_center:review.centers.map(p=>p.map(v=>+v.toFixed(6))),
   quality_diagnostics:review.diagnostics.map(x=>({
    disagreement_pct:+x.disagreementPct.toFixed(5),max_noise_pct:+x.maxNoisePct.toFixed(5)
   }))
  };
  return {ok:true,data};
 }
 function check(data){
  if(!data||typeof data!=='object'||data.schema!=='ga-neutral-evidence-v87'||
     data.physically_verified!==false||data.eligible_for_training!==false||
     data.eligible_for_holdout!==false||data.production_gate!=='BLOCKED'||
     data.data_origin!=='VOLUNTEER_SELF_REPORTED_NEUTRAL')return {ok:false,reason:'UNTRUSTED_LABEL'};
  const built=build({first:data.first_window,second:data.second_window,
   moved:data.excursions_observed,axesCount:data.axes_count});
  return built.ok?{ok:true,record:built.data}:{ok:false,reason:built.reason};
 }
 return {build,check};
});
