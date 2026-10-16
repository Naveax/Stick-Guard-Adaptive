'use strict';
(()=>{
 const $=id=>document.getElementById(id);
 const AXIS_IDS=['axisLX','axisLY','axisRX','axisRY'];
 const phases=[
  ['Sol analog boşta','Sol analogu tamamen bırak. 12 saniye boyunca dokunma.'],
  ['Sol analog küçük hareket','Sol analogla kontrollü küçük hareketler yap.'],
  ['Sol analog tam tur','Sol analogu mümkün olduğu kadar tam tur döndür.'],
  ['Sağ analog boşta','Sağ analogu tamamen bırak. 12 saniye dokunma.'],
  ['Sağ analog küçük hareket','Sağ analogla kontrollü küçük hareketler yap.'],
  ['Sağ analog tam tur','Sağ analogla mümkün olduğunca tam tur döndür.']
 ];
 const state={
  kind:null,selected:null,axisKey:null,axes:[0,1,2,3],mappingMode:'standard',
  quick:null,quickSamples:[],preview:null,full:false,fullCancelled:false,fullPhase:-1,
  fullStarted:0,phaseStarted:0,fullResolver:null,fullLast:-1,fullSamples:[],
  fullDevice:null,fullAxesCount:0,fullButtonsCount:0,fullDeadzone:8,
  data:null,surveyMode:'pending',uploaded:false,restSample:null,restSeparate:null
 };
 const clamp=n=>Math.min(1,Math.max(-1,n));
 const round=n=>Math.round(n*10000)/10000;
 const percentage=(x,y)=>(Math.min(1,Math.hypot(x,y))*100).toFixed(2)+'%';
 const showText=(id,value)=>{if($(id).textContent!==value)$(id).textContent=value;};
 function filter(x,y,dz){
  const r=Math.hypot(x,y),threshold=dz/100;
  if(r<=threshold||r===0)return [0,0];
  const gain=Math.min(1,(r-threshold)/(1-threshold))/r;
  return [round(x*gain),round(y*gain)];
 }
 function pads(){try{return Array.from(navigator.getGamepads?.()||[]).filter(g=>g&&g.connected);}catch{return [];}}
 function chosen(){return pads().find(g=>String(g.index)===String(state.selected))||null;}
 function capacity(g){return !g?0:g.axes.length>=4?4:g.axes.length>=2?2:0;}
 function mapped(g){
  if(!g||capacity(g)===0)return null;
  const n=capacity(g);
  const live=state.axes.slice(0,n);
  if(live.some(x=>!Number.isInteger(x)||x<0||x>=Math.min(g.axes.length,32))||
     new Set(live).size!==live.length)return null;
  const values=state.axes.map((id,i)=>i>=n?0:g.axes[id]);
  if(values.some(x=>typeof x!=='number'||!Number.isFinite(x)||Math.abs(x)>1.001))return null;
  return values.map(clamp);
 }
 function mapping(g){
  state.axisKey=g?g.index+':'+g.axes.length:null;
  state.axes=capacity(g)===4?[0,1,2,3]:[0,1,-1,-1];
  state.mappingMode=(g&&g.mapping==='standard'&&capacity(g)===4)?'standard':'custom';
  for(const [i,id] of AXIS_IDS.entries()){
   const select=$(id);select.replaceChildren();
   for(let a=0;a<Math.min(32,g?.axes.length||0);a++)
     select.add(new Option('Eksen '+a,String(a)));
   if(i>=2&&capacity(g)===2)select.add(new Option('Sağ analog yok','-1'));
   if(!g)select.add(new Option('Kol bekleniyor','-1'));
   select.value=String(state.axes[i]);
   select.disabled=!g||(i>=2&&capacity(g)===2)||state.full||!!state.quick;
  }
  showText('mappingStatus',capacity(g)===2?
   'Bu kolda tarayıcı yalnız 2 eksen gösteriyor; sağ analog ölçülemiyor.':
   capacity(g)===4?(state.mappingMode==='standard'?
   'Standart eksenler seçili; analoglar ters/yanlışsa değiştir.':
   'Özel eksen düzeni. Gerekirse eksenleri elle seç.'):
   'En az iki analog ekseni olan bir kontrolcü gerekli.');
 }
 function refresh(){
  if(state.full||state.quick)return;
  const current=state.selected,select=$('device'),list=pads();
  select.replaceChildren();
  if(list.length===0){
   select.add(new Option('Kontrolcü bulunamadı',''));
   state.selected=null;
  }else{
   for(const g of list)select.add(new Option(g.id||'Kontrolcü '+g.index,String(g.index)));
   state.selected=list.some(g=>String(g.index)===String(current))?current:String(list[0].index);
   select.value=String(state.selected);
  }
  const g=chosen(),key=g?g.index+':'+g.axes.length:null;
  if(key!==state.axisKey)mapping(g);
  const ready=!!mapped(g);
  $('deviceLight').classList.toggle('on',ready);
  showText('deviceState',ready?'Kontrolcü bağlı':'Kontrolcü bekleniyor');
  showText('deviceNote',!g?'USB/Bluetooth ile bağla ve kolun bir tuşuna bas.':
   !ready?'En az iki geçerli analog ekseni gerekli; eşlemeyi kontrol et.':
   capacity(g)===2?'Tek analog algılandı; sağ analog yok.':'Analoglar algılandı, hareketlerini görebilirsin.');
 }
 function currentValues(){
  const g=chosen(),a=mapped(g);
  if(!a)return null;
  const dz=Number($('deadzone').value);
  return {g,a,f:[...filter(a[0],a[1],dz),...filter(a[2],a[3],dz)]};
 }
 function draw(canvas,x,y,fx,fy){
  const ctx=canvas.getContext('2d');if(!ctx)return;
  const c=130,r=101;ctx.clearRect(0,0,260,260);
  ctx.lineWidth=2;ctx.strokeStyle='#385365';
  for(const n of [1,.6,.25]){ctx.beginPath();ctx.arc(c,c,r*n,0,Math.PI*2);ctx.stroke();}
  ctx.beginPath();ctx.moveTo(c-r,c);ctx.lineTo(c+r,c);ctx.moveTo(c,c-r);ctx.lineTo(c,c+r);ctx.stroke();
  for(const [px,py,color,rad] of [[x,y,'#ffbf80',7],[fx,fy,'#70e8d0',5]]){
   ctx.beginPath();ctx.arc(c+px*r,c+py*r,rad,0,Math.PI*2);ctx.fillStyle=color;ctx.fill();
  }
 }
 function displayLive(v){
  const a=v?.a||[0,0,0,0],f=v?.f||[0,0,0,0];
  for(const side of [0,1]){
   const key=side===0?'left':'right',offset=side*2;
   draw($(key+'Canvas'),a[offset],a[offset+1],f[offset],f[offset+1]);
   showText(key+'LiveRaw',v?percentage(a[offset],a[offset+1]):'--');
   showText(key+'LiveFiltered',v?percentage(f[offset],f[offset+1]):'--');
   showText(key+'Coords',v?'X '+a[offset].toFixed(3)+' · Y '+a[offset+1].toFixed(3):'X 0.000 · Y 0.000');
   showText(key+'Status',v?'Canlı':'Bekleniyor');
  }
 }
 function clearRest(){
  state.quickSamples=[];state.restSample=null;state.restSeparate=null;
  for(const key of ['rawTotal','filteredTotal','leftRestRaw','leftRestFiltered','rightRestRaw','rightRestFiltered']){
   showText(key,'--');
  }
  showText('rawCaption','Boştaki drift ölçümü bekleniyor');
  showText('filteredCaption','Filtreli sonuç sanaldır, kol değiştirilmez');
 }
 function restFrom(values,deadzone){
  function one(side){
   const x=side*2,y=x+1;
   const raw=values.map(a=>Math.hypot(a[x],a[y]));
   const filtered=values.map(a=>Math.hypot(...filter(a[x],a[y],deadzone)));
   const rms=arr=>(Math.sqrt(arr.reduce((s,n)=>s+n*n,0)/arr.length)*100).toFixed(2)+'%';
   return {raw:rms(raw),filtered:rms(filtered),
    rawFloat:Math.sqrt(raw.reduce((s,n)=>s+n*n,0)/raw.length)*100,
    filteredFloat:Math.sqrt(filtered.reduce((s,n)=>s+n*n,0)/filtered.length)*100};
  }
  return [one(0),one(1)];
 }
 function updateRest(){
  if(!state.restSample&&!state.restSeparate)return;
  const dz=Number($('deadzone').value);
  const [l,r]=state.restSeparate?
   [restFrom(state.restSeparate.left,dz)[0],restFrom(state.restSeparate.right,dz)[1]]:
   restFrom(state.restSample,dz);
  showText('leftRestRaw',l.raw);showText('leftRestFiltered',l.filtered);
  const hasRight=capacity(chosen())===4;
  showText('rightRestRaw',hasRight?r.raw:'Yok');showText('rightRestFiltered',hasRight?r.filtered:'Yok');
  showText('rawTotal',Math.max(l.rawFloat,hasRight?r.rawFloat:0).toFixed(2)+'%');
  showText('filteredTotal',Math.max(l.filteredFloat,hasRight?r.filteredFloat:0).toFixed(2)+'%');
  showText('rawCaption','En yüksek boşta RMS · sol ve sağ analog');
  showText('filteredCaption','Simüle edilmiş en yüksek boşta RMS');
 }
 function quickCancel(reason){
  if(!state.quick)return;
  state.quick=null;state.quickSamples=[];
  $('measureBtn').disabled=false;$('cancelBtn').classList.add('hidden');
  showText('measureMessage',reason);
 }
 function quickEnd(){
  if(!state.quick)return;
  if(state.quickSamples.length<35){quickCancel('Yeterli örnek alınmadı. Tekrar dene.');return;}
  state.restSample=state.quickSamples;state.restSeparate=null;
  state.quick=null;
  $('measureBtn').disabled=false;$('cancelBtn').classList.add('hidden');
  showText('timer','Bitti');
  showText('measureMessage','5 saniyelik bırakma ölçümü tamamlandı. Ham ve deneme yüzdeleri üstte.');
  updateRest();
 }
 function previewEnd(message,completed=false){
  const p=state.preview;
  if(!p)return;
  state.preview=null;
  $('previewBtn').disabled=false;
  $('previewStopBtn').classList.add('hidden');
  $('measureBtn').disabled=false;
  $('fullTestBtn').disabled=false;
  $('deadzone').disabled=state.uploaded;
  showText('previewTimer',completed?'Bitti':'20 sn');
  if(completed&&p.rest.length>=35){
   state.restSample=p.rest;
   state.restSeparate=null;
   updateRest();
   showText('previewMessage','Deneme tamamlandı. İlk 5 saniyenin boşta drift sonuçları üstte; analog hareketleri boyunca yalnız sanal filtre uygulandı.');
  }else{
   showText('previewMessage',message||'Deneme tamamlanmadı. Sekmeyi açık tutup tekrar dene.');
  }
 }
 function previewBegin(){
  if(state.preview)return;
  if(state.full||state.quick){
   showText('previewMessage','Önce devam eden ölçümü tamamla veya durdur.');
   return;
  }
  const v=currentValues();
  if(!v){
   showText('previewMessage','Kontrolcü algılanmadı. USB/Bluetooth bağlantısını kontrol et, kolun bir tuşuna bas ve sayfayı Chrome/Edge üzerinden HTTPS ile aç.');
   return;
  }
  if(document.hidden){
   showText('previewMessage','Denemeyi başlatmak için bu sekmeyi görünür tut.');
   return;
  }
  state.preview={start:performance.now(),last:-1,id:v.g.id,index:v.g.index,rest:[]};
  clearRest();
  $('previewBtn').disabled=true;
  $('previewStopBtn').classList.remove('hidden');
  $('measureBtn').disabled=true;
  $('fullTestBtn').disabled=true;
  $('deadzone').disabled=true;
  showText('previewTimer','20 sn');
  showText('previewMessage','İlk 5 saniye iki analogu da bırak. Sonra küçük ve tam hareketler yap; iki renkli noktayı karşılaştır.');
 }
 function measure(){
  if(state.quick||state.full||state.preview)return;
  const v=currentValues();
  if(!v||document.hidden){showText('measureMessage','Önce analogları algılanan bir kol bağla.');return;}
  state.quick={time:performance.now(),last:-1,id:v.g.id,index:v.g.index};
  state.quickSamples=[];
  $('measureBtn').disabled=true;$('cancelBtn').classList.remove('hidden');
  showText('measureMessage','Her iki analogu bırak. Sonuçlar 5 saniye sonunda hesaplanacak.');
 }
 function abortFull(reason){
  if(!state.full)return;
  state.full=false;state.fullCancelled=true;state.fullPhase=-1;
  state.fullResolver?.();state.fullResolver=null;
  state.fullSamples=[];state.data=null;
  $('fullTestBtn').disabled=false;$('fullStopBtn').classList.add('hidden');
  $('deadzone').disabled=false;$('device').disabled=false;
  AXIS_IDS.forEach((id,i)=>$(id).disabled=capacity(chosen())===2&&i>=2);
  showText('phaseTitle','Test iptal edildi');
  showText('phaseMessage',reason);
 }
 function uiFullOff(){
  $('fullTestBtn').disabled=false;$('fullStopBtn').classList.add('hidden');
  $('deadzone').disabled=false;$('device').disabled=false;
  AXIS_IDS.forEach((id,i)=>$(id).disabled=capacity(chosen())===2&&i>=2);
 }
 async function startFull(){
  if(state.full||state.quick||state.preview)return;
  const v=currentValues();
  if(!v){showText('phaseMessage','Önce kontrolcünü bağla ve eksenleri kontrol et.');return;}
  if(!$('testAck').checked){showText('phaseMessage','Önce veri kaydı bilgilendirmesini onayla.');return;}
  if(document.hidden){showText('phaseMessage','Bu sekmeyi açık ve görünür tut.');return;}
  if(v.g.axes.length>32||v.g.buttons.length>32){showText('phaseMessage','Bu cihaz 32 üstünde eksen/tuş gösteriyor; yanlış veri oluşturmamak için test desteklenmiyor.');return;}
  state.full=true;state.fullCancelled=false;state.fullPhase=-1;state.fullSamples=[];
  state.data=null;state.uploaded=false;state.surveyMode='pending';
  $('surveyPrompt').classList.add('hidden');$('surveySection').classList.add('hidden');$('finishSection').classList.add('hidden');
  $('dataConsent').checked=false;$('sendDataBtn').disabled=true;
  $('fullTestBtn').disabled=true;$('fullStopBtn').classList.remove('hidden');
  $('deadzone').disabled=true;$('device').disabled=true;
  AXIS_IDS.forEach(id=>$(id).disabled=true);
  state.fullStarted=performance.now();
  state.fullDevice={id:v.g.id,index:v.g.index}; // only held in memory
  state.fullDeadzone=Number($('deadzone').value);
  state.fullAxesCount=v.g.axes.length;state.fullButtonsCount=v.g.buttons.length;
  try{
   for(let i=0;i<6;i++){
    state.fullPhase=-1;
    for(let n=3;n>=1;n--){
     if(!state.full)throw Error('ABORT');
     showText('phaseTitle','Adım '+(i+1)+' / 6 · '+phases[i][0]);
     showText('phaseMessage',phases[i][1]+' · '+n+' saniye sonra kayıt başlayacak.');
     await new Promise(resolve=>setTimeout(resolve,1000));
     if(!state.full||document.hidden)throw Error('ABORT');
    }
    state.fullPhase=i;state.phaseStarted=performance.now();state.fullLast=-1;
    await new Promise(resolve=>state.fullResolver=resolve);
    state.fullResolver=null;
    if(!state.full)throw Error('ABORT');
    if(state.fullSamples.filter(x=>x[0]===i).length<100)throw Error('LOW_FRAME_COUNT');
   }
   if(!state.full)throw Error('ABORT');
   state.full=false;state.fullPhase=-1;uiFullOff();
   const stored=state.fullSamples;
   state.data={
    schema:'ga-controller-telemetry-v1',session_id:crypto.randomUUID(),
    condition:state.kind,
    controller:{axes_count:state.fullAxesCount,buttons_count:state.fullButtonsCount,
     mapping:state.mappingMode,indices:[...state.axes]},
    filter:{profile:'radial-demo-v1',deadzone:state.fullDeadzone},
    consent:{measurement_ack:true,upload:false},
    samples:stored,survey:null
   };
   const restL=stored.filter(s=>s[0]===0).map(s=>[...toMapped(s[2])]);
   const restR=stored.filter(s=>s[0]===3).map(s=>[...toMapped(s[2])]);
   if(restL.length&&restR.length){state.restSample=null;state.restSeparate={left:restL,right:restR};}
   updateRest();
   showText('phaseTitle','6 / 6 · Tamamlandı');
   showText('phaseMessage',stored.length+' örnek kaydedildi. Her örnekte ham eksenler, tuşlar ve hesaplanan filtre çıkışı var.');
   $('surveyPrompt').classList.remove('hidden');
   $('surveyPrompt').scrollIntoView({behavior:'smooth',block:'center'});
  }catch(e){
   abortFull('Test yarıda kesildi veya yeterli veri okunamadı. Baştan dene.');
   // abortFull needs to clear state even when capture has already been cancelled.
   state.fullSamples=[];state.data=null;uiFullOff();
   showText('phaseTitle','Test tamamlanmadı');
   showText('phaseMessage','Test iptal edildi. Sekmeyi açık tutarak tekrar dene.');
  }
 }
 function toMapped(axes){
  return state.axes.map((idx,i)=>i>=Math.min(state.fullAxesCount,4)?0:axes[idx]);
 }
 function frame(now){
  const v=currentValues(),g=v?.g;
  displayLive(v);
  if(state.preview){
   const p=state.preview;
   if(!v||document.hidden||g.id!==p.id||g.index!==p.index){
    previewEnd('Kontrolcü bağlantısı kesildi veya sekme gizlendi. Deneme güvenli biçimde durduruldu.');
   }else{
    const elapsed=now-p.start;
    showText('previewTimer',Math.max(0,Math.ceil((20000-elapsed)/1000))+' sn');
    if(elapsed>=20000){
     previewEnd(null,true);
    }else if(elapsed<5000&&now-p.last>=45){
     p.rest.push([...v.a]);
     p.last=now;
    }
   }
  }
  if(state.quick){
   if(!v||document.hidden||g.id!==state.quick.id||g.index!==state.quick.index){
    quickCancel('Kontrolcü değişti veya sekme kapandı. Ölçüm iptal edildi.');
   }else{
    const elapsed=now-state.quick.time;
    showText('timer',Math.max(0,Math.ceil((5000-elapsed)/1000))+' sn');
    if(elapsed>=5000)quickEnd();
    else if(now-state.quick.last>=45){
     state.quickSamples.push([...v.a]);state.quick.last=now;
    }
   }
  }
  if(state.full&&state.fullPhase>=0){
   if(!v||document.hidden||g.id!==state.fullDevice.id||
      g.index!==state.fullDevice.index||
      g.axes.length!==state.fullAxesCount||g.buttons.length!==state.fullButtonsCount){
    abortFull('Kontrolcü değişti veya sekme kapandı; eksik kayıt tutulmadı.');
   }else{
    const elapsed=now-state.phaseStarted;
    showText('phaseMessage',phases[state.fullPhase][1]+' · '+Math.max(0,Math.ceil((12000-elapsed)/1000))+' sn');
    if(elapsed>=12000){
     state.fullPhase=-1;state.fullResolver?.();
    }else if(now-state.fullLast>=48&&state.fullSamples.length<6000){
     const t=Math.round(now-state.fullStarted);
     const raw=Array.from(g.axes).map(x=>round(clamp(x)));
     const buttons=Array.from(g.buttons).map(b=>round(Math.max(0,Math.min(1,Number(b.value??(b.pressed?1:0))))));
     const mapped=toMapped(raw),dz=state.fullDeadzone;
     const fixed=[...filter(mapped[0],mapped[1],dz),...filter(mapped[2],mapped[3],dz)];
     if(t>(state.fullSamples.at(-1)?.[1]??-1)){
      state.fullSamples.push([state.fullPhase,t,raw,buttons,fixed]);
     }
     state.fullLast=now;
    }
   }
  }
  requestAnimationFrame(frame);
 }
 function survey(){
  if(state.surveyMode!=='yes')return null;
  const keys=['surveyBenefit','surveyInterference','surveyProblem','surveyAffected','surveyLag','surveyRating'];
  const values=keys.map(id=>$(id).value);
  if(values.some(x=>!x)){showText('sendStatus','Lütfen anketin altı sorusunu yanıtla.');return undefined;}
  return {benefit:Number(values[0]),interference:values[1],
   problem:values[2],affected:values[3],latency:values[4],
   rating:Number(values[5]),comment:$('surveyComment').value.trim()};
 }
 function payload(upload){
  if(!state.data)return null;
  const s=survey();if(s===undefined)return null;
  return {...state.data,consent:{measurement_ack:true,upload:!!upload},survey:s};
 }
 function download(){
  const d=payload(false);if(!d)return;
  const url=URL.createObjectURL(new Blob([JSON.stringify(d)],{type:'application/json'}));
  const a=document.createElement('a');a.href=url;a.download='guardadaptive-telemetry-'+d.session_id+'.json';
  document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),3000);
  showText('sendStatus','JSON dosyası indirildi. Sunucuya gönderilmedi.');
 }
 async function send(){
  if(!state.data||!$('dataConsent').checked||state.uploaded)return;
  const d=payload(true);if(!d)return;
  const text=JSON.stringify(d);
  if(new Blob([text]).size>850000){
   showText('sendStatus','Kayıt sunucunun 850 KB sınırını aşıyor. JSON indirilebilir.');return;
  }
  $('sendDataBtn').disabled=true;showText('sendStatus','Ölçüm ve anket work-pc sunucusuna gönderiliyor…');
  try{
   const r=await fetch('/api/telemetry',{method:'POST',
    headers:{'Content-Type':'application/json'},credentials:'omit',
    cache:'no-store',body:text,referrerPolicy:'no-referrer'});
   const out=await r.json();
   if(!r.ok)throw Error(out.error||'HTTP '+r.status);
   if(out.receipt_id!==d.session_id||out.production_gate!=='BLOCKED')throw Error('RECEIPT_NOT_VERIFIED');
   state.uploaded=true;$('dataConsent').checked=false;$('deadzone').disabled=true;
   showText('sendStatus','Kaydedildi. Silme talebi için kayıt numarası: '+out.receipt_id);
  }catch(e){
   showText('sendStatus','Gönderilemedi ('+e.message+'). JSON indirebilirsin.');
   $('sendDataBtn').disabled=false;
  }
 }
 function choose(kind){
  if(state.full||state.quick||state.preview)return;
  state.kind=kind;state.surveyMode='pending';
  state.data=null;state.uploaded=false;$('deadzone').disabled=false;clearRest();
  $('home').classList.add('hidden');$('test').classList.remove('hidden');
  showText('choice',kind==='none'?'Stick driftsiz kol':kind==='regular'?'Stick driftli kol':'Ağır stick driftli kol');
  $('surveyPrompt').classList.add('hidden');$('surveySection').classList.add('hidden');$('finishSection').classList.add('hidden');
  refresh();window.scrollTo({top:0,behavior:'instant'});
 }
 function back(){
  if(state.full||state.quick||state.preview)return;
  $('home').classList.remove('hidden');$('test').classList.add('hidden');
  state.kind=null;state.data=null;state.fullSamples=[];state.surveyMode='pending';
  window.scrollTo({top:0,behavior:'instant'});
 }
 document.querySelectorAll('[data-kind]').forEach(b=>b.addEventListener('click',()=>choose(b.dataset.kind)));
 $('back').addEventListener('click',back);
 $('device').addEventListener('change',e=>{if(state.full||state.quick)return;state.selected=e.target.value;mapping(chosen());clearRest();refresh();});
 AXIS_IDS.forEach((id,i)=>$(id).addEventListener('change',()=>{
  if(state.full||state.quick)return;
  state.axes[i]=Number($(id).value);state.mappingMode='custom';
  showText('mappingStatus',mapped(chosen())?'Özel eksen düzeni seçildi.':
    'Geçersiz eşleme: sol/sağ yönlere farklı eksenler seç.');
  clearRest();
 }));
 $('deadzone').addEventListener('input',()=>{
  const dz=Number($('deadzone').value);
  showText('deadzoneValue',dz+'%');
  // Recalculate every derived value when preview settings change after recording.
  // Raw Gamepad samples are immutable; the archived preview must match the UI.
  if(state.data&&!state.uploaded){
   const ctrl=state.data.controller,cap=ctrl.axes_count>=4?4:2;
   state.data.filter.deadzone=dz;
   for(const row of state.data.samples){
    const a=ctrl.indices.map((idx,i)=>i>=cap?0:row[2][idx]);
    row[4]=[...filter(a[0],a[1],dz),...filter(a[2],a[3],dz)];
   }
  }
  updateRest();
 });
 $('previewBtn').addEventListener('click',previewBegin);
 $('previewStopBtn').addEventListener('click',()=>previewEnd('Deneme durduruldu; eksik ölçümler saklanmadı.'));
 $('measureBtn').addEventListener('click',measure);
 $('cancelBtn').addEventListener('click',()=>quickCancel('Ölçüm durduruldu.'));
 $('fullTestBtn').addEventListener('click',startFull);
 $('fullStopBtn').addEventListener('click',()=>abortFull('Ölçüm kullanıcı isteğiyle durduruldu.'));
 $('surveyYes').addEventListener('click',()=>{
  state.surveyMode='yes';$('surveySection').classList.remove('hidden');
  $('finishSection').classList.remove('hidden');
  $('surveySection').scrollIntoView({behavior:'smooth',block:'center'});
 });
 $('surveyNo').addEventListener('click',()=>{
  state.surveyMode='no';$('surveySection').classList.add('hidden');
  $('finishSection').classList.remove('hidden');
  $('finishSection').scrollIntoView({behavior:'smooth',block:'center'});
 });
 $('dataConsent').addEventListener('change',()=>{
  $('sendDataBtn').disabled=!($('dataConsent').checked&&state.data&&!state.uploaded);
 });
 $('sendDataBtn').addEventListener('click',send);
 $('exportDataBtn').addEventListener('click',download);
 window.addEventListener('gamepadconnected',refresh);
 window.addEventListener('gamepaddisconnected',()=>{
  if(state.preview)previewEnd('Kontrolcü bağlantısı koptu. Deneme durduruldu.');
  if(state.full)abortFull('Kontrolcü bağlantısı koptu.');
  if(state.quick)quickCancel('Kontrolcü bağlantısı koptu.');
  refresh();
 });
 document.addEventListener('visibilitychange',()=>{
  if(!document.hidden)return;
  if(state.preview)previewEnd('Sekme gizlendi; deneme durduruldu.');
  if(state.full)abortFull('Sekme gizlendi; kayıt iptal edildi.');
  if(state.quick)quickCancel('Sekme gizlendi; kısa test iptal edildi.');
 });
 setInterval(refresh,1200);
 refresh();requestAnimationFrame(frame);
})();
