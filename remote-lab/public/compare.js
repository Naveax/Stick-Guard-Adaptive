'use strict';
// GA-RL-02 voluntary comparison: Gamepad API READ ONLY.
// Experimental deadzone visualization DOES NOT change games or controllers.
(()=>{
const $=id=>document.getElementById(id);
const phases=[
 ['Sol stick serbest','Sol analoga dokunma. Kontrolcüyü 12 saniye sabit tut.'],
 ['Sol stick küçük hareket','Sol analogu küçük ve bilinçli hareketlerle oynat.'],
 ['Sol stick tam tur','Sol analogu dış çember boyunca tam tur döndür.'],
 ['Sağ stick serbest','Sağ analoga dokunma. Kontrolcüyü 12 saniye sabit tut.'],
 ['Sağ stick küçük hareket','Sağ analogu küçük ve bilinçli hareketlerle oynat.'],
 ['Sağ stick tam tur','Sağ analogu dış çember boyunca tam tur döndür.']
];
const state={chosen:'',running:false,token:0,task:null,samples:[],first:null,second:null,
 mode:'simulation', session:null,working:false,uploadEnabled:false,afterPrepared:false,
 deviceId:'',startMs:0,lastTake:-1,lastAxes:[0,0,0,0]};
let frames=0;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const round=v=>Math.round(v*10000)/10000;
function pads(){try{return [...(navigator.getGamepads?.()||[])].filter(g=>g&&g.connected);}catch{return [];}}
const selected=()=>pads().find(p=>String(p.index)===state.chosen)||null;
function supported(g){return g&&g.mapping==='standard'&&g.axes.length>=4&&g.axes.slice(0,4).every(n=>Number.isFinite(n)&&Math.abs(n)<=1.0001);}
function connection(){
 const all=pads(),select=$('device'),current=state.chosen;
 select.replaceChildren();
 if(all.length){
  all.forEach(p=>{const o=document.createElement('option');o.value=String(p.index);o.textContent=p.id||'Oyun kontrolcüsü '+p.index;select.append(o);});
  if(!state.running){
   state.chosen=all.some(p=>String(p.index)===current)?current:String(all[0].index);
   select.value=state.chosen;
  }
 }else{
  const o=document.createElement('option');o.value='';o.textContent='Kontrolcü bulunamadı';select.append(o);
  if(!state.running)state.chosen='';
 }
 $('connection').textContent=supported(selected())?'Bağlandı':'Bağlantı bekleniyor';
}
function plot(el,x,y){
 const ctx=el.getContext('2d');
 if(!ctx)return;
 const mid=120,r=94;ctx.clearRect(0,0,240,240);
 ctx.lineWidth=1.8;ctx.strokeStyle='#426478';
 for(const size of [1,.66,.33]){ctx.beginPath();ctx.arc(mid,mid,r*size,0,2*Math.PI);ctx.stroke();}
 ctx.beginPath();ctx.moveTo(mid-r-5,mid);ctx.lineTo(mid+r+5,mid);ctx.moveTo(mid,mid-r-5);ctx.lineTo(mid,mid+r+5);ctx.stroke();
 ctx.fillStyle='#81e8ed';ctx.shadowColor='#54dfff';ctx.shadowBlur=12;ctx.beginPath();ctx.arc(mid+x*r,mid+y*r,8,0,2*Math.PI);ctx.fill();ctx.shadowBlur=0;
}
function stop(reason){
 state.token++;
 if(state.task){const c=state.task;state.task=null;c.reject(Error('STOPPED'));}
 state.running=false;state.samples=[];
 $('stop').disabled=true;$('startBefore').disabled=!!state.first;
 $('startAfter').disabled=false;
 $('device').disabled=false;
 $('mode').disabled=!!state.first;
 for(const name of ['family','condition','transport'])$(name).disabled=!!state.first;
 $('message').textContent=reason;
 $('phaseName').textContent='Ölçüm durduruldu';
 $('secondsLeft').textContent='00:00';
 $('progressFill').style.width='0%';
}
function tick(now){
 const g=selected();
 const a=supported(g)?g.axes.slice(0,4):[0,0,0,0];
 if(frames++%3===0){
  plot($('leftLive'),a[0],a[1]);plot($('rightLive'),a[2],a[3]);
  $('leftAxes').textContent='X '+a[0].toFixed(3)+' · Y '+a[1].toFixed(3);
  $('rightAxes').textContent='X '+a[2].toFixed(3)+' · Y '+a[3].toFixed(3);
  $('leftCurrent').textContent=Math.hypot(a[0],a[1]).toFixed(3);
  $('rightCurrent').textContent=Math.hypot(a[2],a[3]).toFixed(3);
 }
 const task=state.task;
 if(task){
  if(document.hidden||!supported(g)||g.id!==state.deviceId||g.index!==task.padIndex){
   stop('Sekme veya kontrolcü değişti. Eksik test verisi kullanılmadı.');
  }else{
   const elapsed=now-task.began;
   if(elapsed>=12000){state.task=null;task.resolve();}
   else{
    $('secondsLeft').textContent=Math.ceil((12000-elapsed)/1000)+' sn';
    $('progressFill').style.width=round((task.phase+elapsed/12000)/6*100)+'%';
    if(now-state.lastTake>=16){
     const ms=Math.round(now-state.startMs);
     if(ms>(state.samples.at(-1)?.[1]??-1)&&state.samples.length<9000){
      state.samples.push([task.phase,ms,...a.map(v=>round(Math.max(-1,Math.min(1,v))))]);
     }
     state.lastTake=now;
    }
   }
  }
 }
 requestAnimationFrame(tick);
}
async function capture(phase,token){
 if(token!==state.token)throw Error('STOPPED');
 const current=selected();
 return new Promise((resolve,reject)=>{
  state.task={phase,began:performance.now(),padIndex:current.index,resolve,reject};
  state.lastTake=-1;
 });
}
function phaseView(i){
 $('phaseCounter').textContent=(i+1)+'/6';
 $('phaseName').textContent=phases[i][0];
 $('phaseDesc').textContent=phases[i][1];
 [...document.querySelectorAll('.phase-tags span')].forEach((el,j)=>{el.className=j<i?'complete':j===i?'current':'';});
}
async function run(which){
 if(state.running||which==='before'&&state.first||which==='after'&&(!state.first||state.second))return;
 const gp=selected();
 if(!supported(gp))return $('message').textContent='Standart dört eksenli kontrolcü bağlantısı gerekli.';
 if(which==='before'){
  for(const key of ['family','condition','transport'])if(!$(key).value)return $('message').textContent='Kontrolcü bilgilerini seç.';
  if(!$('researchAck').checked)return $('message').textContent='Araştırma bilgilendirmesini onayla.';
 }else if(!$('externalAck').checked){
  return $('message').textContent='Kendi anti-drift aracını etkinleştirdiğini ayrıca işaretlemelisin.';
 }
 if(document.hidden)return $('message').textContent='Sekmeyi görünür tut.';
 state.running=true;state.samples=[];state.deviceId=gp.id;state.startMs=performance.now();
 const token=++state.token;
 $('stop').disabled=false;$('startBefore').disabled=true;$('startAfter').disabled=true;
 $('mode').disabled=true;$('device').disabled=true;
 for(const key of ['family','condition','transport'])$(key).disabled=true;
 $('runLabel').textContent=which==='before'?'İLK ÖLÇÜM / ANTİ-DRİFT KAPALI':'İKİNCİ ÖLÇÜM / KATILIMCI ANTİ-DRİFT AÇIK BEYANI';
 try{
  for(let phase=0;phase<6;phase++){
   phaseView(phase);
   for(let i=3;i>=1;i--){
    if(state.token!==token)throw Error('STOPPED');
    $('secondsLeft').textContent='Hazırlık '+i;
    await sleep(1000);
    if(token!==state.token||document.hidden||!supported(selected())||
      selected().id!==state.deviceId)throw Error('ABORT');
   }
   $('message').textContent='Kayıt aktif, yönergeyi uygula.';
   await capture(phase,token);
   if(token!==state.token||state.samples.filter(r=>r[0]===phase).length<80)throw Error('INCOMPLETE');
  }
  const ready=state.samples;
  if(which==='before')state.first=ready;
  else state.second=ready;
  state.samples=[];state.running=false;
  $('stop').disabled=true;$('device').disabled=false;
  $('progressFill').style.width='100%';$('phaseCounter').textContent='6/6';$('secondsLeft').textContent='Bitti';
  $('phaseName').textContent=which==='before'?'Filtresiz ölçüm tamamlandı':'İkinci ölçüm tamamlandı';
  $('afterPanel').classList.remove('hidden');
  if(state.mode==='simulation'){
   $('manualControls').classList.add('hidden');$('simulationControls').classList.remove('hidden');
   renderReport();showSurvey();
  }else{
   $('simulationControls').classList.add('hidden');$('manualControls').classList.remove('hidden');
   if(which==='after'){renderReport();showSurvey();}
   else{$('comparison').classList.add('hidden');$('message').textContent='Önceki ölçüm kaydedildi. Anti-drift aracını kendin etkinleştir ve ikinci ölçümü başlat.';}
  }
  $('afterPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
 }catch(e){if(state.token===token)stop('Ölçüm tamamlanmadı; eksik örnekler reddedildi. '+e.message);}
 finally{
  $('device').disabled=false;
  if(!state.first){$('mode').disabled=false;for(const key of ['family','condition','transport'])$(key).disabled=false;}
 }
}
function transform(rows,dz){
 return rows.map(r=>{
  function pair(x,y){
   const mag=Math.hypot(x,y);
   if(mag<=dz||mag===0)return [0,0];
   const k=Math.min(1,(mag-dz)/(mag*(1-dz)));
   return [round(x*k),round(y*k)];
  }
  const a=pair(r[2],r[3]),b=pair(r[4],r[5]);
  return [r[0],r[1],...a,...b];
 });
}
function rms(rows,phase,x,y){
 let count=0,sum=0;for(const r of rows){if(r[0]===phase){count++;sum+=r[x]**2+r[y]**2;}}
 return count?Math.sqrt(sum/count):null;
}
function metrics(before,after){
 return [['left',0,1,2,3],['right',3,4,4,5]].map(([name,idle,small,x,y])=>{
  const ib=rms(before,idle,x,y),ia=rms(after,idle,x,y),sb=rms(before,small,x,y),sa=rms(after,small,x,y);
  const gain=ib>=.005?100*(1-ia/ib):null;
  const loss=sb>=.01?100*(1-sa/sb):null;
  const finding=loss!==null&&loss>25?'Kasıtlı hareket kaybı riski':
    gain!==null&&gain>=30&&loss!==null&&loss<=15?'Ölçümde olası denge (kanıt değil)':'Sonuç belirsiz';
  return {name,ib,ia,sb,sa,gain,loss,finding};
 });
}
function cell(tbody,title,value){
 const row=document.createElement('tr'),th=document.createElement('th'),td=document.createElement('td');
 th.textContent=title;td.textContent=value;row.append(th,td);tbody.append(row);
}
const fmt=n=>n===null?'Ölçülemedi':n.toFixed(4);
const pct=n=>n===null?'Yorumlanamaz':(n>=0?'+':'')+n.toFixed(1)+'%';
function renderReport(){
 if(!state.first)return;
 const after=state.mode==='simulation'?transform(state.first,Number($('deadzone').value)):state.second;
 if(!after)return;
 const reports=metrics(state.first,after);
 for(let i=0;i<2;i++){
  const obj=reports[i],tbody=$(i===0?'leftReport':'rightReport');
  tbody.replaceChildren();
  cell(tbody,'Filtresiz serbest sapma',fmt(obj.ib));
  cell(tbody,'Anti-drift sonrası sapma',fmt(obj.ia));
  cell(tbody,'Serbest sapma azalması',pct(obj.gain));
  cell(tbody,'Küçük hareket önce',fmt(obj.sb));
  cell(tbody,'Küçük hareket sonra',fmt(obj.sa));
  cell(tbody,'Kasıtlı hareket kaybı',pct(obj.loss));
  $(i===0?'leftFinding':'rightFinding').textContent=obj.finding;
 }
 $('comparisonCaution').textContent=state.mode==='simulation'?
  'SADECE SİMÜLASYON: Aynı ham ölçüm radyal deadzone filtresinden geçirilmiştir. Oyun girişi veya gerçek kontrolcü değişmedi. Idle RMS azalması gerçek fayda kanıtı değildir; kasıtlı hareket kaybını da incele.':
  'KATILIMCI BEYANLI A/B: İkinci tarayıcı ölçümü farklı zamanda yapılmıştır. Hareket şekli/cihaz ayarı değişmiş olabilir; nedensellik ve gerçek fiziksel etki bağımsız doğrulanmadı.';
 $('comparison').classList.remove('hidden');
}
function showSurvey(){
 $('surveyPanel').classList.remove('hidden');
 document.querySelectorAll('.manualSurvey').forEach(el=>el.classList.toggle('hidden',state.mode!=='manual_external'));
 $('uploadStatus').textContent=state.uploadEnabled?'Gönüllü gönderim açık. İki ayrı onay gerekiyor.':'Sunucuda yükleme kapalı; yerel JSON indirme açık.';
 updateSend();
}
function survey(){
 const keys=['severity','usage','impact','recommend'];
 if(keys.some(id=>!$(id).value))return null;
 if(state.mode==='manual_external'&&(!$('effect').value||!$('motion').value))return null;
 return {
  baseline_severity:Number($('severity').value),
  use_context:$('usage').value,
  subjective_impact:$('impact').value,
  perceived_effect:state.mode==='simulation'?'not_tested':$('effect').value,
  micro_motion:state.mode==='simulation'?'not_tested':$('motion').value,
  recommendation:$('recommend').value
 };
}
function packet(){
 const answers=survey();if(!answers||!state.first||(state.mode==='manual_external'&&!state.second))return null;
 if(!state.session)state.session=crypto.randomUUID();
 return {
  schema:'ga-anti-drift-study-v1',protocol:'GA-RL-02',session_id:state.session,
  controller:{family:$('family').value,condition:$('condition').value,transport:$('transport').value},
  guard:{mode:state.mode,deadzone:state.mode==='simulation'?Number($('deadzone').value):null},
  before:state.first,after:state.mode==='simulation'?null:state.second,
  survey:answers,
  consent:{research_ack:$('researchAck').checked,upload:true,self_report_ack:$('selfReportAck').checked}
 };
}
function updateSend(){
 $('send').disabled=!state.uploadEnabled||!$('uploadConsent').checked||!$('selfReportAck').checked||!packet();
}
function download(){
 const p=packet();
 if(!p)return $('uploadStatus').textContent='Önce anketi tamamla.';
 const after=p.guard.mode==='simulation'?transform(p.before,p.guard.deadzone):p.after;
 const json=JSON.stringify({...p,consent:{...p.consent,upload:false},derived_after:after,export_note:'Local copy; no server submission. Simulation does not modify game inputs.'});
 const url=URL.createObjectURL(new Blob([json],{type:'application/json'}));
 const a=document.createElement('a');a.href=url;a.download='guardadaptive-study-'+p.session_id+'.json';
 document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),3000);
}
async function send(){
 const p=packet();
 if(!p||!state.uploadEnabled||!$('uploadConsent').checked||!$('selfReportAck').checked)return;
 const body=JSON.stringify(p);
 if(new Blob([body]).size>850000){$('uploadStatus').textContent='Dosya sunucu sınırını aşıyor. Yerel JSON indirilebilir.';return;}
 $('send').disabled=true;
 $('uploadStatus').textContent='Ölçüm ve anket Work PC’ye gönderiliyor…';
 try{
  const res=await fetch('/api/studies',{method:'POST',headers:{'Content-Type':'application/json'},
   cache:'no-store',credentials:'omit',body});
  const out=await res.json();
  if(!res.ok)throw Error(out.reason||out.error||'HTTP '+res.status);
  if(!out.accepted||out.id!==p.session_id)throw Error('Kayıt makbuzu uyuşmuyor');
  $('uploadStatus').textContent='KAYDEDİLDİ. Oturum: '+out.id+
    '. Filtre etkisi doğrulanmış fiziksel başarı değildir; silme için oturum numaranı sakla.';
  $('uploadConsent').checked=false;$('send').disabled=true;
 }catch(e){$('uploadStatus').textContent='Gönderim başarısız: '+e.message+' · JSON indirilebilir.';updateSend();}
}
$('device').addEventListener('change',e=>{if(!state.running)state.chosen=e.target.value;});
$('mode').addEventListener('change',e=>{
 if(state.first)return;
 state.mode=e.target.value;
 $('modeHelp').textContent=state.mode==='simulation'?
  'Sanal filtre yalnız ölçülen X/Y sayılarına uygulanır, oyun veya sürücü değiştirilmez.':
  'Bu seçenek kendi harici anti-drift aracını gerektirir. Araç otomatik açılmaz; iki ayrı 6-aşamalı tarayıcı kaydı alınır.';
});
$('startBefore').addEventListener('click',()=>run('before'));
$('startAfter').addEventListener('click',()=>run('after'));
$('stop').addEventListener('click',()=>stop('İsteğin üzerine test iptal edildi.'));
$('deadzone').addEventListener('input',e=>{
 $('deadzoneValue').textContent=(Number(e.target.value)*100).toFixed(1)+'%';
 renderReport();updateSend();
});
$('uploadConsent').addEventListener('change',updateSend);
$('selfReportAck').addEventListener('change',updateSend);
for(const id of ['severity','usage','impact','effect','motion','recommend'])$(id).addEventListener('change',updateSend);
$('download').addEventListener('click',download);
$('send').addEventListener('click',send);
window.addEventListener('gamepadconnected',connection);
window.addEventListener('gamepaddisconnected',()=>{if(state.running)stop('Kontrolcü ayrıldı, eksik ölçüm iptal edildi.');connection();});
document.addEventListener('visibilitychange',()=>{if(document.hidden&&state.running)stop('Sekme gizlendi, ölçüm iptal edildi.');});
setInterval(()=>{if(!state.running)connection();},1500);
fetch('/api/status',{cache:'no-store'}).then(r=>r.json()).then(j=>{
 state.uploadEnabled=j.uploads_enabled===true;
}).catch(()=>{state.uploadEnabled=false;});
connection();requestAnimationFrame(tick);
})();