'use strict';
/* GuardAdaptive v0.94. Read-only browser Gamepad API direction guide.
 * Manual instruction response is NOT verified physical intent or released neutral.
 * Never transmits, stores, remaps, calibrates or writes controller input. */
(function(root,factory){
 const core=factory();
 if(typeof module==='object'&&module.exports)module.exports=core;
 if(root&&root.document)install(root,core);
})(typeof window!=='undefined'?window:null,function(){
 const median=v=>{const a=[...v].sort((a,b)=>a-b),m=Math.floor(a.length/2);return a.length%2?a[m]:(a[m-1]+a[m])/2;};
 const finite=v=>typeof v==='number'&&Number.isFinite(v)&&v>=-1.001&&v<=1.001;
 function mappingOk(mapping,axesCount){
  if(![2,4].includes(mapping?.length)||!Number.isInteger(axesCount)||axesCount<2||axesCount>32)return false;
  return mapping.every(i=>Number.isInteger(i)&&i>=0&&i<axesCount)&&new Set(mapping).size===mapping.length;
 }
 function neutral(samples,mapping){
  if(samples.length<25||samples.length>512||!samples.every(r=>Array.isArray(r)&&mapping.every(i=>finite(r[i]))))
   return {ok:false,reason:'INSUFFICIENT_OR_INVALID_NEUTRAL'};
  const centers=mapping.map(i=>median(samples.map(r=>r[i])));
  const drift=mapping.map((i,j)=>Math.max(...samples.map(r=>Math.abs(r[i]-centers[j]))));
  return {ok:drift.every(x=>x<=0.035),reason:drift.some(x=>x>0.035)?'UNSTABLE_NEUTRAL':'STABLE_BUT_UNVERIFIED',
    centers:centers.map(x=>+x.toFixed(6)),maxJitter:drift.map(x=>+x.toFixed(6)),physicallyReleased:false};
 }
 const moves=[
 {side:'Sol',axis:0,sign:-1,name:'Sol analogu SOLA it ve basılı tut'},
 {side:'Sol',axis:0,sign:1,name:'Sol analogu SAĞA it ve basılı tut'},
 {side:'Sol',axis:1,sign:-1,name:'Sol analogu YUKARI it ve basılı tut'},
 {side:'Sol',axis:1,sign:1,name:'Sol analogu AŞAĞI it ve basılı tut'},
 {side:'Sağ',axis:2,sign:-1,name:'Sağ analogu SOLA it ve basılı tut'},
 {side:'Sağ',axis:2,sign:1,name:'Sağ analogu SAĞA it ve basılı tut'},
 {side:'Sağ',axis:3,sign:-1,name:'Sağ analogu YUKARI it ve basılı tut'},
 {side:'Sağ',axis:3,sign:1,name:'Sağ analogu AŞAĞI it ve basılı tut'}
 ];
 function analyzeStep(samples,mapping,centers,step){
  if(samples.length<20||samples.length>512||!samples.every(r=>Array.isArray(r)&&mapping.every(i=>finite(r[i]))))
   return {status:'INVALID_CAPTURE',physicallyVerified:false};
  const index=step.axis,axis=mapping[index],d=samples.map(r=>r[axis]-centers[index]);
  const expected=step.sign*median(d),active=d.filter(v=>v*step.sign>=0.18).length;
  const opposite=d.filter(v=>v*step.sign<=-0.18).length;
  const other=index%2===0?index+1:index-1;
  const wrong=median(samples.map(r=>Math.abs(r[mapping[other]]-centers[other])));
  const otherStick=index<2?[2,3]:[0,1];
  const competing=mapping.length===4?median(samples.map(r=>
   Math.hypot(...otherStick.map(j=>r[mapping[j]]-centers[j])))):0;
  let status='CONSISTENT_WITH_PROMPT';
  if(opposite>=samples.length*0.6 && expected<0)status='REVERSED_SIGN';
  else if(expected<0.18 || active<Math.ceil(samples.length*0.6))status='WEAK_OR_UNSTABLE_DIRECTION';
  else if(wrong>0.25||competing>0.25)status='CROSS_AXIS_ACTIVITY';
  return {status,medianRequestedPct:+(expected*100).toFixed(2),activeFrames:active,frames:samples.length,
    otherAxisPct:+(wrong*100).toFixed(2),otherStickPct:+(competing*100).toFixed(2),
    physicallyVerified:false};
 }
 return {mappingOk,neutral,analyzeStep,moves};
});
function install(root,core){
 const $=id=>root.document.getElementById(id);
 const trigger=$('dirStart'),stop=$('dirCancel'),message=$('dirStatus');
 if(!trigger||!stop||!message)return;
 let run=null,busy=false;
 root.GuardAdaptiveDirectionGuide={busy:()=>busy,cancel:()=>cancel('Yön testi sıfırlandı.')};
 function status(s){message.textContent=s;}
 function pads(){try{return Array.from(root.navigator.getGamepads?.()||[]).filter(g=>g&&g.connected);}catch{return [];}}
 function device(){
  const i=$('device')?.value;
  return pads().find(p=>String(p.index)===String(i))||null;
 }
 function mapping(){const g=device();if(!g)return null;const ids=['axisLX','axisLY','axisRX','axisRY'];
  const cap=g.axes.length>=4?4:g.axes.length>=2?2:0;
  const indices=ids.slice(0,cap).map(id=>Number($(id)?.value));
  return core.mappingOk(indices,g.axes.length)?indices:null;
 }
 function same(){
  const g=device();const map=mapping();
  return !!(run&&g&&map&&g.index===run.index&&g.id===run.id&&g.axes.length===run.axesCount&&
   map.join(',')===run.mapping.join(',')&&!root.document.hidden&&
   !root.GuardAdaptiveTestBusy?.());
 }
 function cancel(reason){
  busy=false;run=null;trigger.disabled=false;stop.classList.add('hidden');
  trigger.textContent='Yön testi başlat';status(reason);
 }
 function captureStep(){
  if(!same()){cancel('Kontrolcü/eşleme değişti veya başka test başladı. Yön testi iptal edildi.');return;}
  const stage=run.stage;
  run.collecting=true;busy=true;trigger.disabled=true;stop.classList.remove('hidden');
  const capture=[];const started=root.performance.now();
  function frame(now){
   if(!run||run.stage!==stage)return;
   if(!same()){cancel('Kontrolcü ayrıldı, sekme gizlendi veya eşleme değişti.');return;}
   const g=device();
   if(!g||g.axes.some(v=>!Number.isFinite(v)||Math.abs(v)>1.001)){cancel('Geçersiz eksen örneği.');return;}
   if(capture.length<512)capture.push(Array.from(g.axes));
   if(now-started<1300){root.requestAnimationFrame(frame);return;}
   run.collecting=false;busy=true;trigger.disabled=false;
   if(stage===-1){
    const rest=core.neutral(capture,run.mapping);
    if(!rest.ok){cancel('Nötr kararsız veya yetersiz. Kolu bırakıp tekrar başlat.');return;}
    run.centers=rest.centers;run.stage=0;
    status('Nötr sabit göründü; fiziksel bırakma doğrulanmadı. '+core.moves[0].name+'. Hazır olduğunda ölç.');
   }else{
    const result=core.analyzeStep(capture,run.mapping,run.centers,core.moves[stage]);
    run.results.push(result.status);
    run.stage++;
    if(run.stage>=run.moves.length){
     const pass=run.results.every(x=>x==='CONSISTENT_WITH_PROMPT');
     status((pass?'Yön adımları komutlarla tutarlı. ':'Bir veya daha fazla yön adımı şüpheli. ')+
       'Adımlar: '+run.results.join(' / ')+'. Fiziksel yön kanıtı değildir. Veri gönderilmedi.');
     busy=false;run=null;trigger.disabled=false;trigger.textContent='Yeniden başlat';stop.classList.add('hidden');
     return;
    }
    status('Sonuç: '+result.status+'. Sıradaki: '+run.moves[run.stage].name+'. Hazır olduğunda ölç.');
   }
   trigger.textContent='Bu hareketi ölç (1,3 sn)';
  }
  root.requestAnimationFrame(frame);
 }
 trigger.addEventListener('click',()=>{
  if(!run){
   if(root.GuardAdaptiveTestBusy?.()){status('Devam eden ölçüm tamamlanmalı veya durdurulmalı.');return;}
   const g=device(),map=mapping();
   if(!g||!map){status('Önce kontrolcü bağla ve geçerli eksen eşlemesini seç.');return;}
   run={index:g.index,id:g.id,axesCount:g.axes.length,mapping:map,stage:-1,
     moves:core.moves.slice(0,map.length===2?4:8),results:[],centers:null,collecting:false};
   busy=true;status('Bütün analogları bırak. 1,3 saniye nötr örneği alınıyor.');
   captureStep();
  }else if(!run.collecting){
   status(run.moves[run.stage].name+'; 1,3 saniye basılı tut.');
   captureStep();
  }
 });
 stop.addEventListener('click',()=>cancel('Yön testi iptal edildi; kayıt saklanmadı.'));
 $('device')?.addEventListener('change',()=>{if(run)cancel('Kontrolcü değişti; yön testi iptal edildi.');});
 ['axisLX','axisLY','axisRX','axisRY'].forEach(id=>$(id)?.addEventListener('change',()=>{if(run)cancel('Eksen eşlemesi değişti; yeniden başlat.');}));
 root.addEventListener('gamepaddisconnected',()=>{if(run)cancel('Kontrolcü ayrıldı.');});
 root.document.addEventListener('visibilitychange',()=>{if(run&&root.document.hidden)cancel('Sekme gizlendi; yön testi durduruldu.');});
 $('back')?.addEventListener('click',()=>{if(run)cancel('Yön testi kapatıldı.');});
}
