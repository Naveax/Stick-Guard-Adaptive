'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const source=fs.readFileSync(path.join(__dirname,'../public-simple/app.js'),'utf8');
const start=source.indexOf('  function neutralQuality(');
const end=source.indexOf('  function finished(){',start);
assert.ok(start>=0&&end>start,'neutralQuality helper must exist before finished()');
const expr=source.slice(start,end)+'\nneutralQuality';
function check(rows,phase,side) {
  const context=vm.createContext({state:{samples:rows}});
  const gate=vm.runInContext(expr,context,{timeout:1000});
  return gate(phase,side);
}
function rows(x,y) {
 const output=[];
 for(let i=0;i<128;i++){
  const jitter=i%2===0?0.0002:-0.0002;
  output.push([0,i*20,x+jitter,y-jitter,0.0372+jitter,-jitter]);
 }
 for(let i=0;i<128;i++){
  const jitter=i%2===0?0.0002:-0.0002;
  output.push([3,10000+i*20,x+jitter,y-jitter,0.0372+jitter,-jitter]);
 }
 return output;
}
test('Naveax frontend stability checks 441 signed static drift positions',()=>{
 let passed=0;
 for(let x=-10;x<=10;x++)for(let y=-10;y<=10;y++){
   const data=rows(x/100,y/100);
   assert.equal(check(data,0,0),true,'left '+x+','+y);
   assert.equal(check(data,3,2),true,'right '+x+','+y);
   passed++;
 }
 assert.equal(passed,441);
});
test('Naveax frontend rejects contamination in the intended neutral stage',()=>{
 const data=rows(0.025,0.015);
 data[40][2]=0.84;data[40][3]=0.45;
 assert.equal(check(data,0,0),false);
 assert.equal(check(data,3,2),true);
});
test('Naveax frontend rejects drift ramps and truncated neutral stages',()=>{
 const data=rows(0.025,0.015);
 for(let i=0;i<128;i++)data[i][2]+=i*0.00023;
 assert.equal(check(data,0,0),false);
 const insufficient=rows(0,0).filter(x=>x[0]===3);
 assert.equal(check(insufficient,0,0),false);
});
test('Naveax frontend remains observational and does not claim verified release',()=>{
 const data=rows(0.01,0.0); // Could be held intended input, not real drift.
 assert.equal(check(data,0,0),true);
 assert.equal(source.includes('kalibrasyon fiziksel olarak kanıtlandı'),false);
});
