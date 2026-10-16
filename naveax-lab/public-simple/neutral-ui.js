'use strict';
// Isolated opt-in browser preview. No samples leave memory; no controller writes.
(()=>{
 const $=id=>document.getElementById(id);
 const api=window.GuardAdaptiveNeutralPreview;
 if(!api)return;
 const controls=['startBtn','previewBtn','controllerSelect','backBtn','axisLX','axisLY','axisRX','axisRY'];
 let stage='idle',first=[],second=[],centers=null,moved=[false,false],started=0,last=-1,
     selectedId='',selectedIndex=-1,count=0,signature='',candidate=null,evidenceData=null;
 function gamepad(){
  try{
   const selected=$('controllerSelect').value;
   return [...(navigator.getGamepads?.()||[])].find(p=>p&&p.connected&&String(p.index)===selected)||null;
  }catch{return null;}
 }
 function read(p){
  if(!p||!Array.isArray(controls))return null;
  const c=p.axes.length>=4?4:p.axes.length>=2?2:0;
  if(!c||p.axes.length>32)return null;
  const axes=['axisLX','axisLY','axisRX','axisRY'].map(id=>Number($(id).value));
  if(axes.slice(0,c).some(i=>!Number.isInteger(i)||i<0||i>=p.axes.length)||new Set(axes.slice(0,c)).size!==c)return null;
  const result=axes.map((ix,i)=>i>=c?0:p.axes[ix]);
  if(result.some(x=>!Number.isFinite(x)||Math.abs(x)>1.001))return null;
  return {c,values:result.map(v=>Math.min(1,Math.max(-1,v))),key:axes.join(',')};
 }
 const say=v=>{$('neutralMessage').textContent=v;};
 function setBusy(on){
  controls.forEach(id=>{
   const el=$(id);
   if(on)el.disabled=true;
   else if(id==='axisRX'||id==='axisRY')el.disabled=!!gamepad()&&gamepad().axes.length<4;
   else el.disabled=false;
  });
  $('neutralStopBtn').disabled=!on;
  $('neutralStartBtn').disabled=on;
 }
 function reset(msg,retain=false){
  const wasBusy=stage==='first'||stage==='move'||stage==='second';
  stage='idle';first=[];second=[];centers=null;moved=[false,false];
  if(!retain){candidate=null;evidenceData=null;}
  $('neutralDownloadBtn').disabled=!evidenceData;
  if(wasBusy)setBusy(false);
  $('neutralStartBtn').disabled=false;
  $('neutralStartBtn').textContent='1. nötr ölçümünü başlat (5 sn)';
  $('neutralStopBtn').disabled=true;
  $('neutralTimer').textContent='Hazır';
  say(msg);
 }
 function start(){
  if(stage==='move'){
   if(!moved[0]||(count===4&&!moved[1]))return;
   stage='second';second=[];started=performance.now();last=-1;
   $('neutralStartBtn').disabled=true;
   $('neutralTimer').textContent='5 sn';
   say('İkinci nötr ölçüm başladı. Her iki analogu da bırak ve 5 saniye dokunma.');
   return;
  }
  if(stage!=='idle')return;
  if($('startBtn').disabled||$('previewStopBtn').disabled===false)return say('Önce devam eden testi tamamla.');
  const p=gamepad(),r=read(p);
  if(!r||document.hidden)return say('Kontrolcü/eşleme geçersiz veya sekme gizli. Kontrolcünün bir tuşuna basıp yeniden dene.');
  candidate=null;evidenceData=null;$('neutralDownloadBtn').disabled=true;
  stage='first';first=[];second=[];moved=[false,false];
  selectedId=p.id;selectedIndex=p.index;count=r.c;signature=r.key;
  started=performance.now();last=-1;
  setBusy(true);
  $('neutralTimer').textContent='5 sn';
  say('İlk nötr ölçüm: iki analogu da tamamen bırak ve 5 saniye dokunma.');
 }
 function compare(r){
  if(candidate&&r){
   const rows=api.compare(r.values,candidate.centers,0.12,candidate.count);
   for(const [i,name] of ['Left','Right'].entries()){
    const v=rows?.[i];
    $('neutral'+name+'Raw').textContent=v?v.rawPct.toFixed(2)+'%':'--';
    $('neutral'+name+'Deadzone').textContent=v?v.fixedPct.toFixed(2)+'%':'--';
    $('neutral'+name+'Center').textContent=v?v.centerOnlyPct.toFixed(2)+'%':'--';
   }
  }else{
   for(const name of ['Left','Right'])
    for(const part of ['Raw','Deadzone','Center'])$('neutral'+name+part).textContent='--';
  }
 }
 function frame(now){
  const p=gamepad(),r=read(p);
  const consistent=!!p&&!!r&&p.id===selectedId&&p.index===selectedIndex&&r.c===count&&r.key===signature;
  if(stage!=='idle'){
   if(document.hidden||!consistent){reset('Kontrolcü, sekme veya eksen eşlemesi değişti. Kalibrasyon iptal edildi.');}
   else if(stage==='move'){
    const seen=api.moveEvidence(r.values,centers,count);
    moved[0] ||= seen[0];
    if(count===4)moved[1] ||= seen[1];
    if(moved[0]&&(count===2||moved[1])){
     $('neutralStartBtn').disabled=false;
     $('neutralStartBtn').textContent='2. nötr ölçümünü başlat (5 sn)';
     say('Hareket algılandı. Analogları bırak ve ikinci ölçümü başlat. Gerçek bırakma bağımsız doğrulanamaz.');
    }
   }else{
    const elapsed=now-started;
    $('neutralTimer').textContent=Math.max(0,Math.ceil((5000-elapsed)/1000))+' sn';
    if(elapsed>=5000){
     if(stage==='first'){
      const firstReview=api.reviewFirst(first,count);
      if(!firstReview.ok)reset('İlk nötr ölçüm reddedildi ('+firstReview.reason+'). Baştan dene.');
      else{
       centers=firstReview.centers;stage='move';
       $('neutralTimer').textContent='Hareket';
       say('İlk ölçüm uygun. Şimdi iki analogu en az %7,5 hareket ettir ve bırak.');
      }
     }else{
      const res=api.review(first,second,moved,count);
      if(!res.ok)reset('Kalibrasyon reddedildi ('+res.reason+'). Nötr ölçümler uyuşmuyor.');
      else{
       const evidence=window.GuardAdaptiveEvidenceV87?.build({first,second,moved,axesCount:count});
       if(!evidence?.ok){reset('Yerel nötr veri doğrulanamadı; yeniden dene.');}
       else{
        evidenceData=evidence.data;
        candidate={id:selectedId,index:selectedIndex,signature,count,centers:res.centers};
        reset('İki gözlemsel nötr ölçüm uyuştu. Anonim veri isteğe bağlı indirilebilir. Fiziksel bırakma doğrulanmadı.',true);
       }
      }
     }
    }else if(now-last>=45){
     if(stage==='first')first.push([...r.values]);
     else second.push([...r.values]);
     last=now;
    }
   }
  }
  if(candidate&&(!p||!r||p.id!==candidate.id||p.index!==candidate.index||r.key!==candidate.signature||r.c!==candidate.count)){
   candidate=null;evidenceData=null;$('neutralDownloadBtn').disabled=true;say('Kontrolcü veya eşleme değişti. Önceki merkez karşılaştırması kaldırıldı.');
  }
  compare(r);
  requestAnimationFrame(frame);
 }
 function downloadEvidence(){
  if(stage!=='idle'||!candidate||!evidenceData)return;
  const verified=window.GuardAdaptiveEvidenceV87?.check(evidenceData);
  if(!verified?.ok){reset('Yerel nötr veri geçersiz; indirme iptal edildi.');return;}
  const url=URL.createObjectURL(new Blob([JSON.stringify(verified.record,null,2)],{type:'application/json'}));
  const link=document.createElement('a');
  link.href=url;link.download='guardadaptive-neutral-evidence-v87.json';
  document.body.append(link);link.click();link.remove();
  setTimeout(()=>URL.revokeObjectURL(url),1200);
 }
 $('neutralStartBtn').addEventListener('click',start);
 $('neutralDownloadBtn').addEventListener('click',downloadEvidence);
 $('neutralStopBtn').addEventListener('click',()=>reset('Kalibrasyon iptal edildi; eksik örnekler kullanılmadı.'));
 document.addEventListener('visibilitychange',()=>{if(document.hidden){if(stage!=='idle')reset('Sekme gizlendi; ölçüm iptal edildi.');candidate=null;evidenceData=null;$('neutralDownloadBtn').disabled=true;}});
 window.addEventListener('gamepaddisconnected',()=>{if(stage!=='idle')reset('Kontrolcü ayrıldı; ölçüm iptal edildi.');candidate=null;evidenceData=null;$('neutralDownloadBtn').disabled=true;});
 window.GuardAdaptiveNeutralUI=Object.freeze({busy:()=>stage!=='idle',clear:()=>{if(stage==='idle'){candidate=null;evidenceData=null;$('neutralDownloadBtn').disabled=true;}}});
 requestAnimationFrame(frame);
})();
