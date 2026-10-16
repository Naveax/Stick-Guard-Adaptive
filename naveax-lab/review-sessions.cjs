'use strict';
// Offline, read-only quarantine report for volunteer recordings.
// NEVER promotes models or treats self-reported phases as physical ground truth.
const fs=require('node:fs/promises');
const path=require('node:path');
const crypto=require('node:crypto');
const {validateSubmission,MAX_JSON_BYTES}=require('./validate.cjs');
const {reviewSampleQuality}=require('./quality.cjs');

function digest(obj){
  // Exclude session UUID: identical uploaded signals under new IDs are duplicates.
  return crypto.createHash('sha256').update(JSON.stringify({
    protocol:obj.protocol,controller:obj.controller,client:obj.client,samples:obj.samples
  })).digest('hex').toUpperCase();
}
async function reviewDirectory(inputDir,outputFile){
  if(typeof inputDir!=='string'||typeof outputFile!=='string')throw Error('DIR_AND_OUTPUT_REQUIRED');
  const input=path.resolve(inputDir);
  const output=path.resolve(outputFile);
  if(input===output||output.startsWith(input+path.sep))throw Error('REPORT_OUTSIDE_INPUT_REQUIRED');
  const src=await fs.lstat(input);
  if(!src.isDirectory()||src.isSymbolicLink())throw Error('INPUT_DIR_MUST_BE_REAL_DIRECTORY');
  const entries=(await fs.readdir(input,{withFileTypes:true}))
    .filter(e=>/^[0-9a-f-]{36}\.json$/.test(e.name))
    .sort((a,b)=>a.name.localeCompare(b.name));
  if(entries.length>2000)throw Error('MAX_SESSION_FILES_EXCEEDED');
  const seen=new Map(), sessions=[];
  const quarantine=(session_id,result,reason=null)=>({
    session_id,result,reason,quality_flags:[],reviewer_decision:'NOT_EVALUATED',
    physical_intent_verified:false,allow_training:false,allow_blind_holdout:false
  });
  for(const entry of entries){
    const file=path.join(input,entry.name);
    let data;
    if(!entry.isFile()){sessions.push(quarantine(entry.name.slice(0,-5),'QUARANTINE_NOT_REGULAR_FILE'));continue;}
    const st=await fs.lstat(file);
    if(st.isSymbolicLink()||st.size>MAX_JSON_BYTES||st.size<2){
      sessions.push(quarantine(entry.name.slice(0,-5),'QUARANTINE_FILE_SIZE_OR_LINK'));continue;
    }
    try{data=JSON.parse(await fs.readFile(file,'utf8'));}
    catch{sessions.push(quarantine(entry.name.slice(0,-5),'QUARANTINE_INVALID_JSON'));continue;}
    const accepted=validateSubmission(data);
    if(!accepted.ok || data.session_id!==entry.name.slice(0,-5)){
      sessions.push(quarantine(entry.name.slice(0,-5),'QUARANTINE_INVALID_SCHEMA_OR_FILENAME',accepted.reason||'UUID_MISMATCH'));continue;
    }
    const fingerprint=digest(data);
    const quality=reviewSampleQuality(data.samples);
    const prior=seen.get(fingerprint);
    if(prior)quality.flags.push('DUPLICATE_SIGNALS_AND_TIMESTAMPS');
    else seen.set(fingerprint,data.session_id);
    sessions.push({
      session_id:data.session_id,
      result:prior?'QUARANTINE_DUPLICATE':'REQUIRES_HUMAN_REVIEW',
      duplicate_of:prior||null,
      content_sha256:fingerprint,
      claimed_condition:data.controller.condition,
      family:data.controller.family,
      sample_count:data.samples.length,
      quality_flags:quality.flags,
      phase_diagnostics:quality.phase_diagnostics,
      reviewer_decision:'NOT_EVALUATED',
      physical_intent_verified:false,
      allow_training:false,allow_blind_holdout:false
    });
  }
  const report={
    schema:1,source:'GuardAdaptive-RemoteLab-offline-session-triage-v1',
    session_count:sessions.length,
    unique_samples_seen:seen.size,
    quarantined_count:sessions.filter(s=>s.result.startsWith('QUARANTINE')).length,
    reviewer_verified_count:0,
    volunteer_reported_labels_not_ground_truth:true,
    no_auto_model_training:true,
    no_auto_holdout_admission:true,
    no_data_written_back_to_input:true,
    physical_controller_intent_proven:false,
    model_quality_original_shadow:'FAILED_PREVIOUSLY',
    production_gate:'BLOCKED',
    sessions
  };
  const handle=await fs.open(output,'wx',0o600);
  try {await handle.writeFile(JSON.stringify(report,null,2)+'\n');await handle.sync();}
  finally{await handle.close();}
  return report;
}
if(require.main===module){
  reviewDirectory(process.argv[2],process.argv[3]).then(report=>{
    console.log('GA_REMOTE_REVIEW=PASS TOTAL='+report.session_count+
      ' QUARANTINED='+report.quarantined_count+' TRAINING=BLOCKED PRODUCTION=BLOCKED');
  }).catch(err=>{console.error('GA_REMOTE_REVIEW_FAIL='+err.message);process.exitCode=1;});
}
module.exports={reviewDirectory,digest};
