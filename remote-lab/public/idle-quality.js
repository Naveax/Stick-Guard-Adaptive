'use strict';
/* Offline/browser-only stability heuristic. It cannot verify physical stick release. */
(function(root, factory) {
  const api = Object.freeze(factory());
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.GuardAdaptiveIdleQuality = api;
})(typeof window !== 'undefined' ? window : {}, function() {
  const MIN_ROWS = 35;
  const MAX_ROWS = 4096;
  const OUTLIER_LIMIT = 0.012;
  const NOISE_RMS_LIMIT = 0.003;

  function median(values) {
    const sorted = [...values].sort((a,b)=>a-b);
    const i = Math.floor(sorted.length/2);
    return sorted.length%2 ? sorted[i] : (sorted[i-1]+sorted[i])*0.5;
  }
  function checkIdle(values, side) {
    const fail = (reason, n=0) => ({
      valid:false, reason, count:n, physicalReleaseVerified:false,
      eligibleForTraining:false, interpretation:'UNVERIFIED_OBSERVATION'
    });
    if (side !== 0 && side !== 1) return fail('INVALID_STICK');
    if (!Array.isArray(values) || values.length < MIN_ROWS || values.length > MAX_ROWS)
      return fail('INSUFFICIENT_SAMPLES', values?.length ?? 0);
    const ix = side*2;
    const xy = [];
    for (const value of values) {
      if (!Array.isArray(value) || value.length < ix+2) return fail('INVALID_SAMPLE', values.length);
      const x=value[ix],y=value[ix+1];
      if (!Number.isFinite(x) || !Number.isFinite(y) || Math.abs(x)>1 || Math.abs(y)>1)
        return fail('INVALID_SAMPLE', values.length);
      xy.push([x,y]);
    }
    const cx=median(xy.map(v=>v[0]));
    const cy=median(xy.map(v=>v[1]));
    let maxDeviation=0,squared=0;
    for (const [x,y] of xy) {
      const d=Math.hypot(x-cx,y-cy);
      maxDeviation=Math.max(maxDeviation,d);
      squared+=d*d;
    }
    const noiseRms=Math.sqrt(squared/xy.length);
    const valid=maxDeviation<=OUTLIER_LIMIT && noiseRms<=NOISE_RMS_LIMIT;
    return {
      valid, reason:valid?'STABLE_UNVERIFIED':'MOTION_OR_UNSTABLE_NEUTRAL',
      count:xy.length,
      observedCenterPct:Math.hypot(cx,cy)*100,
      maxDeviationPct:maxDeviation*100,
      noiseRmsPct:noiseRms*100,
      physicalReleaseVerified:false,
      eligibleForTraining:false,
      interpretation:'UNVERIFIED_OBSERVATION'
    };
  }
  return {checkIdle};
});
