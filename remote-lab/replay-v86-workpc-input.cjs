'use strict';
// Transport *only numeric anonymous axis series* to read-only offline replay.
// No device ID, consent fields, survey responses, controller labels, or buttons.
const fs=require('node:fs');
const src=process.argv[2];
if(!src){console.error('Provide a local telemetry JSON path');process.exit(2);}
let obj;
try{obj=JSON.parse(fs.readFileSync(src,'utf8'));}catch{console.error('TELEMETRY_UNREADABLE');process.exit(2);}
const rows=obj.samples,indices=obj.controller?.indices;
if(obj.schema!=='ga-controller-telemetry-v1'||!Array.isArray(rows)||
   rows.length<480||rows.length>9000||!Array.isArray(indices)||
   indices.length!==4||indices.some(v=>!Number.isInteger(v)||v<0||v>=32)||
   new Set(indices).size!==4){console.error('UNSUPPORTED_OR_INVALID_TELEMETRY');process.exit(2);}
const collected=[],counts=Array(6).fill(0);
let phase=0,time=-1;
for(const row of rows){
 if(!Array.isArray(row)||row.length!==5||!Number.isInteger(row[0])||
    row[0]<phase||row[0]>phase+1||row[0]>5||!Number.isInteger(row[1])||
    row[1]<=time||row[1]>180000||!Array.isArray(row[2])){console.error('BAD_TELEMETRY_ORDER');process.exit(2);}
 const v=indices.map(i=>row[2][i]);
 if(v.some(x=>typeof x!=='number'||!Number.isFinite(x)||Math.abs(x)>1)){
   console.error('INVALID_AXIS');process.exit(2);
 }
 collected.push([row[0],row[1],...v].join(','));
 phase=row[0];time=row[1];counts[phase]++;
}
if(counts.some(n=>n<80)){console.error('SIX_PHASE_RECORD_REQUIRED');process.exit(2);}
process.stdout.write(collected.join('\n')+'\n');
