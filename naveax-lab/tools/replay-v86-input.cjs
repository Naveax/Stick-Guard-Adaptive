'use strict';
// Read-only, local converter. Standard output carries axis-only CSV for Rust.
// Never uploads, rewrites, or claims verified neutral conditions.
const fs=require('node:fs');
const file=process.argv[2];
if(!file){console.error('Usage: node replay-v86-input.cjs <existing local session.json>');process.exit(2);}
let data;
try {data=JSON.parse(fs.readFileSync(file,'utf8'));}
catch {console.error('SESSION_FILE_UNAVAILABLE_OR_INVALID');process.exit(2);}
if(data.schema!=='ga-remote-lab-v1'||data.protocol!=='GA-RL-01'||!Array.isArray(data.samples)){
 console.error('UNSUPPORTED_SESSION_SCHEMA');process.exit(2);
}
const rows=data.samples;
if(rows.length>9000||rows.length<480){console.error('INVALID_ROW_COUNT');process.exit(2);}
let t=-1,phase=0;const per=Array(6).fill(0);
for(const r of rows){
 if(!Array.isArray(r)||r.length!==6||!Number.isInteger(r[0])||r[0]<phase||
     r[0]>phase+1||r[0]>5||!Number.isInteger(r[1])||r[1]<=t||r[1]>180000||
     r.slice(2).some(v=>typeof v!=='number'||!Number.isFinite(v)||Math.abs(v)>1)){
    console.error('SESSION_REJECTED_INCOMPLETE_OR_INVALID');process.exit(2);
 }
 phase=r[0];t=r[1];per[phase]++;
}
if(per.some(n=>n<80)){console.error('SIX_PHASE_DATA_REQUIRED');process.exit(2);}
for(const r of rows)process.stdout.write(r.join(',')+'\n');
