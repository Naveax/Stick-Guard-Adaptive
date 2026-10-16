'use strict';
// Simple, opt-in browser-only gamepad observation. Never sends input to hardware.
(() => {
  const $ = id => document.getElementById(id);
  const LABELS = Object.freeze({none:'Stick driftsiz kol',regular:'Stick driftli kol',severe:'Ağır stick driftli kol'});
  const PHASES = [
    {name:'Sol analogu bırak',tip:'Sol analoga 12 saniye boyunca dokunma.',time:12000},
    {name:'Sol analogda küçük hareketler',tip:'Sol analogu hafifçe ve bilinçli şekilde hareket ettir.',time:12000},
    {name:'Sol analogla tam tur',tip:'Sol analogla dış çemberde yavaşça tam tur yap.',time:12000},
    {name:'Sağ analogu bırak',tip:'Sağ analoga 12 saniye boyunca dokunma.',time:12000},
    {name:'Sağ analogda küçük hareketler',tip:'Sağ analogu hafifçe ve bilinçli şekilde hareket ettir.',time:12000},
    {name:'Sağ analogla tam tur',tip:'Sağ analogla dış çemberde yavaşça tam tur yap.',time:12000}
  ];
  const state = {
    kind:null,index:null,mode:'idle',capture:null,runId:0,samples:[],
    started:0,deviceId:null,receipt:null,uploadEnabled:false,uploaded:false,
    axes:[0,1,2,3],axisKey:null,mappingMode:'standard',
    previewMode:'idle',previewStarted:0,previewLast:0,previewSamples:[],
    previewSnapshot:null,previewDeviceId:null,previewUploaded:false
  };
  const steps=[...$('phaseSteps').children];
  const AXIS_IDS=['axisLX','axisLY','axisRX','axisRY'];
  const PREVIEW_PROFILE='radial-deadzone-demo-0.12-v1';
  function filterPair(x,y){
    const radius=Math.hypot(x,y);
    if(radius<=0.12)return [0,0];
    const gain=Math.min(1,(radius-0.12)/0.88)/radius;
    return [Math.max(-1,Math.min(1,x*gain)),Math.max(-1,Math.min(1,y*gain))];
  }
  function axisCount(g){return g ? Math.min(32,g.axes.length)>=4?4:Math.min(32,g.axes.length)>=2?2:0:0;}
  function defaultMapping(g){
    state.axes=axisCount(g)===4?[0,1,2,3]:[0,1,-1,-1];
    state.axisKey=g?String(g.index)+':'+g.axes.length:null;
    state.mappingMode=g?.mapping==='standard'&&axisCount(g)===4?'standard':'custom';
    AXIS_IDS.forEach((id,i)=>{
      const el=$(id);el.replaceChildren();
      const limit=Math.min(g?.axes.length||0,32);
      for(let n=0;n<limit;n++)el.add(new Option('Eksen '+n,String(n)));
      if(i>=2&&axisCount(g)===2)el.add(new Option('Yok','-1'));
      el.disabled=!g||(i>=2&&axisCount(g)===2)||state.previewMode==='capturing'||state.mode==='running';
      el.value=String(state.axes[i]);
    });
    $('mappingStatus').textContent=axisCount(g)===2?
      '2 analog ekseni bulundu. Tek analogla önizleme yapılabilir; altı aşamalı test için iki stick gerekir.':
      axisCount(g)===4?
      (g.mapping==='standard'?'Standart eksenler seçili. Gerekirse değiştir.':
       'Standart olmayan kol. Analoglar doğru tepki vermiyorsa eksenleri değiştir.'):
      'Bu kontrolcüde tarayıcının okuyabildiği yeterli analog ekseni yok.';
  }
  function mappedAxes(g){
    if(!g||axisCount(g)<2)return null;
    const use=axisCount(g);
    const active=state.axes.slice(0,use);
    if(active.some(x=>!Number.isInteger(x)||x<0||x>=Math.min(g.axes.length,32))||
       new Set(active).size!==active.length)return null;
    const out=state.axes.map((index,i)=>i>=use?0:g.axes[index]);
    if(out.some(v=>!Number.isFinite(v)||Math.abs(v)>1.001))return null;
    return out.map(v=>Math.max(-1,Math.min(1,v)));
  }
  function previewReady(g){return !!mappedAxes(g);}

  function pads(){
    if(typeof navigator.getGamepads!=='function')return [];
    try{return [...(navigator.getGamepads()||[])].filter(g=>g&&g.connected);}
    catch{return [];}
  }
  function chosen(){return pads().find(g=>String(g.index)===String(state.index))||null;}
  function compatible(g){return axisCount(g)===4&&!!mappedAxes(g);}
  function feedback(message,error=false){
    $('testMessage').textContent=message;
    $('testMessage').classList.toggle('error',error);
  }
  function refreshDevices(){
    if(state.mode==='running'||state.previewMode==='capturing'||window.GuardAdaptiveNeutralUI?.busy())return;
    const list=pads();
    const select=$('controllerSelect');
    const previous=state.index;
    select.replaceChildren();
    if(!list.length){
      const opt=new Option('Kontrolcü bulunamadı','');
      select.add(opt);state.index=null;
    }else{
      for(const g of list)select.add(new Option(g.id||('Kontrolcü '+(g.index+1)),String(g.index)));
      state.index=list.some(g=>String(g.index)===String(previous))?previous:String(list[0].index);
      select.value=String(state.index);
    }
    const g=chosen();
    const key=g?String(g.index)+':'+g.axes.length:null;
    if(key!==state.axisKey)defaultMapping(g);
    updateDeviceInfo();
  }
  function updateDeviceInfo(){
    const g=chosen(),ok=previewReady(g),badge=$('deviceBadge');
    badge.textContent=ok?'Kol bağlı':'Bağlantı bekleniyor';
    badge.classList.toggle('ok',ok);
    const note=$('compatNotice');
    if(!window.isSecureContext)note.textContent='Lütfen siteyi HTTPS üzerinden aç.';
    else if(typeof navigator.getGamepads!=='function')note.textContent='Tarayıcın oyun kolunu desteklemiyor. Chrome veya Edge kullan.';
    else if(!g)note.textContent='Kolunu USB/Bluetooth ile bağla ve bir tuşuna bas.';
    else if(!ok)note.textContent='Tarayıcı bu kolda en az iki geçerli analog ekseni göstermiyor veya eksenler tekrarlanıyor.';
    else if(axisCount(g)===2)note.textContent='Tek analog erişilebilir. Anti-stick drift önizlemesi kullanılabilir. Altı aşamalı test için iki analog gerekir.';
    else note.textContent='Kol algılandı. Ekseni yanlış hareket ediyorsa eşlemeyi değiştir.';
    note.classList.toggle('error',!!g&&!ok);
  }
  function draw(canvas,x,y){
    const ctx=canvas.getContext('2d');
    if(!ctx)return;
    const size=canvas.width,c=size/2,r=size*.38;
    ctx.clearRect(0,0,size,size);
    ctx.strokeStyle='#31506a';ctx.lineWidth=2;
    for(const scale of [1,.6,.2]){
      ctx.beginPath();ctx.arc(c,c,r*scale,0,Math.PI*2);ctx.stroke();
    }
    ctx.beginPath();ctx.moveTo(c-r-7,c);ctx.lineTo(c+r+7,c);
    ctx.moveTo(c,c-r-7);ctx.lineTo(c,c+r+7);ctx.stroke();
    const px=c+x*r,py=c+y*r;
    ctx.strokeStyle='#7de2ed';ctx.lineWidth=3;
    ctx.beginPath();ctx.moveTo(c,c);ctx.lineTo(px,py);ctx.stroke();
    ctx.fillStyle='#a0f5ee';ctx.beginPath();ctx.arc(px,py,7,0,Math.PI*2);ctx.fill();
  }
  function progress(p,ratio=0){
    $('progressFill').style.width=(100*Math.max(0,Math.min(1,(p+ratio)/6))).toFixed(1)+'%';
    $('stepMarker').textContent=(p===6?6:p+1)+' / 6';
    steps.forEach((el,i)=>{el.classList.toggle('active',i===p);el.classList.toggle('done',i<p);});
  }
  function choose(kind){
    if(state.mode==='running'||window.GuardAdaptiveNeutralUI?.busy())return;
    if(!Object.hasOwn(LABELS,kind))return;
    state.kind=kind;state.mode='idle';state.samples=[];state.receipt=null;state.uploaded=false;
    state.previewMode='idle';state.previewSamples=[];state.previewSnapshot=null;state.previewUploaded=false;
    $('previewPanel').classList.add('hidden');$('surveyPanel').classList.add('hidden');
    $('previewBtn').disabled=false;$('previewStopBtn').disabled=true;
    $('choiceBadge').textContent=LABELS[kind];
    $('choiceScreen').classList.add('hidden');
    $('testScreen').classList.remove('hidden');
    $('resultPanel').classList.add('hidden');
    $('ackConsent').checked=false;
    $('uploadConsent').checked=false;
    $('uploadBtn').disabled=true;
    $('startBtn').textContent='Testi başlat';
    $('phaseTitle').textContent='Başlamaya hazır';
    $('phaseDescription').textContent='6 kısa adımda sol ve sağ analogları ölçeceğiz.';
    $('timer').textContent='00:00';$('progressFill').style.width='0%';
    $('stepMarker').textContent='0 / 6';
    steps.forEach(s=>s.classList.remove('active','done'));
    feedback('Kolunu bağladıktan sonra testi başlat.');
    refreshDevices();
    window.scrollTo({top:0,behavior:'instant'});
  }
  function abort(message){
    state.runId++;
    if(state.capture){const t=state.capture;state.capture=null;t.reject(new Error('STOPPED'));}
    state.mode='idle';state.samples=[];state.receipt=null;state.uploaded=false;
    $('stopBtn').disabled=true;$('startBtn').disabled=false;
    $('controllerSelect').disabled=false;$('backBtn').disabled=false;
    $('previewBtn').disabled=false;AXIS_IDS.forEach((id,i)=>$(id).disabled=axisCount(chosen())===2&&i>=2);
    $('resultPanel').classList.add('hidden');
    $('timer').textContent='00:00';$('phaseTitle').textContent='Başlamaya hazır';
    $('phaseDescription').textContent='Testi yeniden başlatabilirsin.';
    progress(0,0);
    feedback(message,true);
    refreshDevices();
  }
  function frame(now){
    const g=chosen(),axis=mappedAxes(g)||[0,0,0,0];
    if(state.previewMode==='capturing'){
      if(!previewReady(g)||g.id!==state.previewDeviceId||document.hidden){
        stopPreview('Kontrolcü bağlantısı veya sekme değişti. Deneme iptal edildi.');
      }else{
        const elapsed=now-state.previewStarted;
        $('previewTimer').textContent=Math.max(0,Math.ceil((20000-elapsed)/1000))+' sn';
        drawComparison($('previewLeft'),axis[0],axis[1]);
        drawComparison($('previewRight'),axis[2],axis[3]);
        if(elapsed>=20000){completePreview();}
        else if(now-state.previewLast>=22&&state.previewSamples.length<1600){
          const t=Math.round(elapsed);
          if(t>(state.previewSamples.at(-1)?.[0]??-1)){
            state.previewSamples.push([t,...axis.map(v=>Math.round(v*10000)/10000)]);
          }
          state.previewLast=now;
        }
      }
    }
    draw($('leftCanvas'),axis[0],axis[1]);
    draw($('rightCanvas'),axis[2],axis[3]);
    $('leftValue').textContent=Math.hypot(axis[0],axis[1]).toFixed(3);
    $('rightValue').textContent=Math.hypot(axis[2],axis[3]).toFixed(3);
    const capture=state.capture;
    if(capture){
      if(!compatible(g)||g.id!==state.deviceId||document.hidden){
        abort('Kol bağlantısı veya sekme değişti. Eksik test kaydedilmedi.');
      }else{
        const elapsed=now-capture.started;
        if(elapsed>=capture.time){
          state.capture=null;capture.resolve();
        }else{
          $('timer').textContent=Math.ceil((capture.time-elapsed)/1000)+' sn';
          progress(capture.phase,elapsed/capture.time);
          if(now-capture.last>=16&&state.samples.length<9000){
            const t=Math.round(now-state.started);
            if(t>(state.samples.at(-1)?.[1]??-1)){
              const values=mappedAxes(g).map(v=>Math.round(v*10000)/10000);
              state.samples.push([capture.phase,t,...values]);
            }
            capture.last=now;
          }
        }
      }
    }
    requestAnimationFrame(frame);
  }
  function sleep(ms){return new Promise(resolve=>setTimeout(resolve,ms));}
  async function countdown(phase,runId){
    $('phaseTitle').textContent=PHASES[phase].name;
    $('phaseDescription').textContent=PHASES[phase].tip;
    progress(phase,0);
    for(let sec=3;sec>0;sec--){
      if(runId!==state.runId||document.hidden)throw new Error('STOPPED');
      $('timer').textContent=sec+' sn';
      feedback('Hazırlan: '+sec+' saniye.');
      await sleep(1000);
    }
    if(runId!==state.runId||document.hidden)throw new Error('STOPPED');
  }
  function capturePhase(phase,runId){
    if(runId!==state.runId)return Promise.reject(new Error('STOPPED'));
    return new Promise((resolve,reject)=>{
      const now=performance.now();
      state.capture={phase,started:now,last:now-20,time:PHASES[phase].time,resolve,reject};
      feedback('Ölçüm sürüyor. Yönergeyi uygula.');
    });
  }
  function rms(phase,a,b){
    const vals=state.samples.filter(row=>row[0]===phase).map(row=>Math.hypot(row[a],row[b]));
    return Math.sqrt(vals.reduce((s,v)=>s+v*v,0)/vals.length).toFixed(4);
  }
  // Display-only observational neutral stability check. Does not prove release.
  function neutralQuality(phase,axisOffset){
    const rows=state.samples.filter(row=>row[0]===phase);
    if(rows.length<35||rows.length>4096)return false;
    const xy=rows.map(r=>[r[2+axisOffset],r[3+axisOffset]]);
    if(xy.some(p=>p.some(v=>!Number.isFinite(v)||Math.abs(v)>1)))return false;
    const median=values=>{
      const sorted=values.sort((a,b)=>a-b),i=Math.floor(sorted.length/2);
      return sorted.length%2?sorted[i]:(sorted[i-1]+sorted[i])*0.5;
    };
    const cx=median(xy.map(p=>p[0])),cy=median(xy.map(p=>p[1]));
    let max=0,sse=0;
    for(const [x,y] of xy){
      const deviation=Math.hypot(x-cx,y-cy);
      max=Math.max(max,deviation);
      sse+=deviation*deviation;
    }
    return max<=0.012&&Math.sqrt(sse/xy.length)<=0.003;
  }
  function finished(){
    state.mode='done';
    $('stopBtn').disabled=true;$('startBtn').disabled=false;
    $('startBtn').textContent='Yeniden test et';$('backBtn').disabled=false;
    $('controllerSelect').disabled=false;
    progress(6);
    $('timer').textContent='Bitti';$('phaseTitle').textContent='Test tamamlandı';
    $('phaseDescription').textContent='Sonuçları görebilirsin.';
    state.receipt={
      schema:'ga-remote-lab-v1',protocol:'GA-RL-01',
      session_id:crypto.randomUUID(),
      controller:{family:'generic',condition:state.kind==='none'?'healthy':'drift',
        transport:'unknown',severity:state.kind},
      client:state.mappingMode==='standard'?{mapping:'standard',version:'0.1.0'}:
        {mapping:'custom',version:'0.1.0',axes:[...state.axes]},
      consent:{information_ack:true,upload:false},
      samples:state.samples
    };
    const leftValid=neutralQuality(0,0),rightValid=neutralQuality(3,2);
    $('leftRms').textContent=leftValid?rms(0,2,3):'Geçersiz';
    $('rightRms').textContent=rightValid?rms(3,4,5):'Geçersiz';
    $('neutralQualityResult').textContent=leftValid&&rightValid?
      'İki nötr bölüm istikrarlı gözlendi; analogların gerçekten bırakıldığı bağımsız doğrulanmadı.':
      'Boşta ölçüm kalite kontrolü başarısız: '+
      [!leftValid?'sol analog':null,!rightValid?'sağ analog':null].filter(Boolean).join(' ve ')+
      ' bölümünde hareket veya sıçrama var. Gösterilen değerleri drift başarısı sayma; yeniden ölç.';
    $('sampleCount').textContent=state.samples.length.toLocaleString('tr-TR');
    $('resultPanel').classList.remove('hidden');
    $('uploadConsent').checked=false;
    $('uploadBtn').disabled=true;
    $('uploadStatus').textContent=state.uploadEnabled?'İstersen sonucu gönderebilirsin.':'Gönderim kapalı; sonucu indirebilirsin.';
    feedback('Test tamamlandı. Sonucun aşağıda.');
    $('resultPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
  }
  async function begin(){
    if(state.mode==='running'||state.previewMode==='capturing'||window.GuardAdaptiveNeutralUI?.busy()||!state.kind)return;
    const g=chosen();
    if(!compatible(g)){feedback('Altı aşamalı test için iki analog ve dört eşlenmiş eksen gerekir. Tek analoglu kol için önizlemeyi kullanabilirsin.',true);return;}
    if(!$('ackConsent').checked){feedback('Devam etmek için test bilgilendirmesini onayla.',true);return;}
    if(document.hidden){feedback('Test için bu sekmeyi açık tut.',true);return;}
    const runId=++state.runId;
    state.mode='running';state.samples=[];state.receipt=null;state.uploaded=false;
    state.deviceId=g.id;state.started=performance.now();
    $('resultPanel').classList.add('hidden');
    $('startBtn').disabled=true;$('stopBtn').disabled=false;
    $('controllerSelect').disabled=true;$('backBtn').disabled=true;
    $('previewBtn').disabled=true;AXIS_IDS.forEach(id=>$(id).disabled=true);
    try{
      for(let i=0;i<PHASES.length;i++){
        await countdown(i,runId);
        await capturePhase(i,runId);
        if(state.samples.filter(r=>r[0]===i).length<80)throw new Error('LOW_SAMPLE_COUNT');
      }
      if(runId!==state.runId)throw new Error('STOPPED');
      finished();
    }catch(e){
      if(runId===state.runId)abort(e.message==='LOW_SAMPLE_COUNT'?
        'Yetersiz örnek alındı. Sekmeyi açık tutarak yeniden dene.':
        'Test durdu. Eksik veri kaydedilmedi.');
    }
  }
  function drawComparison(canvas,x,y){
    const ctx=canvas.getContext('2d');if(!ctx)return;
    const c=canvas.width/2,r=canvas.width*.38;
    ctx.clearRect(0,0,canvas.width,canvas.height);
    ctx.strokeStyle='#36516a';ctx.lineWidth=2;
    for(const scale of [1,.6,.2]){ctx.beginPath();ctx.arc(c,c,r*scale,0,2*Math.PI);ctx.stroke();}
    ctx.beginPath();ctx.moveTo(c-r,c);ctx.lineTo(c+r,c);ctx.moveTo(c,c-r);ctx.lineTo(c,c+r);ctx.stroke();
    const filtered=filterPair(x,y);
    for(const [axes,color,rad] of [[[x,y],'#f2bb70',7],[filtered,'#8aeedd',5]]){
      ctx.beginPath();ctx.arc(c+axes[0]*r,c+axes[1]*r,rad,0,Math.PI*2);
      ctx.fillStyle=color;ctx.fill();
    }
  }
  function previewMessage(text,error=false){
    $('previewMessage').textContent=text;
    $('previewMessage').classList.toggle('error',error);
  }
  function unlockPreview(){
    $('previewBtn').disabled=false;$('previewStopBtn').disabled=true;
    $('startBtn').disabled=false;$('controllerSelect').disabled=false;$('backBtn').disabled=false;
    AXIS_IDS.forEach((id,i)=>$(id).disabled=axisCount(chosen())===2&&i>=2);
  }
  function stopPreview(text){
    if(state.previewMode!=='capturing')return;
    state.previewMode='idle';state.previewSamples=[];state.previewSnapshot=null;
    $('surveyPanel').classList.add('hidden');$('previewTimer').textContent='20 sn';
    previewMessage(text,true);unlockPreview();
  }
  function completePreview(){
    if(state.previewMode!=='capturing')return;
    if(state.previewSamples.length<80||
       state.previewSamples.at(-1)[0]-state.previewSamples[0][0]<8000){
      stopPreview('Yeterli örnek alınamadı. Sekmeyi açık tutarak yeniden dene.');
      return;
    }
    state.previewMode='done';
    state.previewSnapshot={
      schema:'ga-preview-v1',session_id:crypto.randomUUID(),
      controller:{severity:state.kind},
      mapping:{mode:state.mappingMode,indices:[...state.axes],
        axes_available:axisCount(chosen())},
      profile:PREVIEW_PROFILE,
      consent:{information_ack:true,upload:false},
      samples:state.previewSamples
    };
    $('previewTimer').textContent='Bitti';
    $('surveyPanel').classList.remove('hidden');
    $('previewConsent').checked=false;
    $('previewSendBtn').disabled=true;
    $('previewSendStatus').textContent='';
    for(const key of ['benefit','interference','problem','affected','latency','rating']){
      $(key).selectedIndex=0;
    }
    previewMessage('Önizleme tamamlandı. Kısa anketi doldurabilir veya veriyi yerel indirebilirsin.');
    unlockPreview();
    $('surveyPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
  }
  function beginPreview(){
    if(state.mode==='running'||state.previewMode==='capturing'||window.GuardAdaptiveNeutralUI?.busy()||!state.kind)return;
    const g=chosen();
    if(!previewReady(g)){
      $('previewPanel').classList.remove('hidden');
      $('surveyPanel').classList.add('hidden');
      previewMessage('Önce en az iki analog ekseni olan bir kontrolcü bağla. Tarayıcı görmüyorsa kolun bir tuşuna bas veya analog eşlemesini değiştir.',true);
      $('previewPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
      return;
    }
    if(document.hidden){feedback('Deneme için bu sekmeyi görünür tut.',true);return;}
    state.previewMode='capturing';state.previewSamples=[];state.previewSnapshot=null;
    state.previewUploaded=false;state.previewDeviceId=g.id;
    state.previewStarted=performance.now();state.previewLast=state.previewStarted-25;
    $('previewPanel').classList.remove('hidden');
    $('surveyPanel').classList.add('hidden');
    $('previewTimer').textContent='20 sn';
    $('previewBtn').disabled=true;$('previewStopBtn').disabled=false;
    $('startBtn').disabled=true;$('controllerSelect').disabled=true;$('backBtn').disabled=true;
    AXIS_IDS.forEach(id=>$(id).disabled=true);
    previewMessage('Önce bırak, sonra yavaş/küçük hareketlerle filtreyi karşılaştır.');
    $('previewPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
  }
  function createPreviewPayload(upload){
    if(!state.previewSnapshot)return null;
    const values={};
    for(const key of ['benefit','interference','problem','affected','latency','rating']){
      values[key]=$(key).value;
      if(!values[key]){ $('previewSendStatus').textContent='Lütfen altı sorunun hepsini cevapla.';return null;}
    }
    const rating=Number(values.rating),benefit=Number(values.benefit);
    if(!Number.isInteger(rating)||!Number.isInteger(benefit))return null;
    return {
      ...state.previewSnapshot,consent:{information_ack:true,upload:!!upload},
      survey:{benefit,interference:values.interference,problem:values.problem,
        affected:values.affected,latency:values.latency,rating}
    };
  }
  function downloadPreview(){
    const data=createPreviewPayload(false);
    if(!data)return;
    const url=URL.createObjectURL(new Blob([JSON.stringify(data)],
      {type:'application/json'}));
    const link=document.createElement('a');link.href=url;
    link.download='guardadaptive-preview-'+data.session_id+'.json';
    document.body.append(link);link.click();link.remove();
    setTimeout(()=>URL.revokeObjectURL(url),3000);
    $('previewSendStatus').textContent='Önizleme ve anket JSON olarak indirildi.';
  }
  async function uploadPreview(){
    if(!state.previewSnapshot||state.previewUploaded||!state.uploadEnabled||
       !$('previewConsent').checked)return;
    const payload=createPreviewPayload(true);if(!payload)return;
    $('previewSendBtn').disabled=true;
    $('previewSendStatus').textContent='Ölçüm ve yanıtlar gönderiliyor...';
    try{
      const response=await fetch('/api/preview-feedback',{
        method:'POST',headers:{'Content-Type':'application/json'},
        cache:'no-store',credentials:'omit',referrerPolicy:'no-referrer',
        body:JSON.stringify(payload)});
      const result=await response.json();
      if(!response.ok)throw new Error(result.error||'UPLOAD_FAILED');
      if(result.receipt_id!==payload.session_id||result.production_gate!=='BLOCKED'||
         result.actual_game_input_modified!==false)throw new Error('RECEIPT_MISMATCH');
      state.previewUploaded=true;$('previewConsent').checked=false;
      $('previewSendStatus').textContent='Gönderildi. Kayıt numarası: '+result.receipt_id;
    }catch(error){
      $('previewSendStatus').textContent='Gönderilemedi ('+error.message+'). JSON indirebilirsin.';
      $('previewSendBtn').disabled=false;
    }
  }
  function exportResult(){
    if(!state.receipt)return;
    const blob=new Blob([JSON.stringify(state.receipt)],{type:'application/json'});
    const url=URL.createObjectURL(blob);
    const link=document.createElement('a');
    link.href=url;link.download='stick-drift-'+state.receipt.session_id+'.json';
    document.body.append(link);link.click();link.remove();
    setTimeout(()=>URL.revokeObjectURL(url),3000);
  }
  async function fetchStatus(){
    try{
      const res=await fetch('/api/status',{cache:'no-store'});
      if(!res.ok)throw new Error('NO_SERVER');
      state.uploadEnabled=(await res.json()).upload_enabled===true;
    }catch{state.uploadEnabled=false;}
  }
  async function upload(){
    if(!state.receipt||!state.uploadEnabled||!$('uploadConsent').checked||state.uploaded)return;
    $('uploadBtn').disabled=true;
    $('uploadStatus').textContent='Gönderiliyor...';
    try{
      const document={...state.receipt,consent:{information_ack:true,upload:true}};
      const res=await fetch('/api/submissions',{method:'POST',
        headers:{'Content-Type':'application/json'},credentials:'omit',
        cache:'no-store',referrerPolicy:'no-referrer',body:JSON.stringify(document)});
      const reply=await res.json();
      if(!res.ok)throw new Error(reply.error||'UPLOAD_FAILED');
      if(reply.receipt_id!==state.receipt.session_id)throw new Error('RECEIPT_MISMATCH');
      state.uploaded=true;$('uploadConsent').checked=false;
      $('uploadStatus').textContent='Gönderildi. Kayıt numarası: '+reply.receipt_id;
    }catch(e){
      $('uploadStatus').textContent='Gönderilemedi ('+e.message+'). Sonucu indirebilirsin.';
      $('uploadBtn').disabled=false;
    }
  }
  document.querySelectorAll('[data-kind]').forEach(button=>
    button.addEventListener('click',()=>choose(button.dataset.kind)));
  $('backBtn').addEventListener('click',()=>{
    if(state.mode==='running'||window.GuardAdaptiveNeutralUI?.busy())return;
    window.GuardAdaptiveNeutralUI?.clear();
    $('testScreen').classList.add('hidden');$('choiceScreen').classList.remove('hidden');
    state.kind=null;state.receipt=null;state.previewSnapshot=null;
    state.previewSamples=[];state.previewMode='idle';
    $('previewPanel').classList.add('hidden');
    window.scrollTo({top:0,behavior:'instant'});
  });
  $('controllerSelect').addEventListener('change',event=>{
    if(state.mode!=='running'&&!window.GuardAdaptiveNeutralUI?.busy()){
      window.GuardAdaptiveNeutralUI?.clear();state.index=event.target.value;updateDeviceInfo();
    }
  });
  AXIS_IDS.forEach((id,i)=>$(id).addEventListener('change',()=>{
    if(state.mode==='running'||state.previewMode==='capturing'||window.GuardAdaptiveNeutralUI?.busy())return;
    window.GuardAdaptiveNeutralUI?.clear();
    const value=Number($(id).value);
    state.axes[i]=value;
    state.mappingMode='custom';
    const g=chosen();
    if(!mappedAxes(g)){
      $('mappingStatus').textContent='Hatalı eşleme: iki farklı yön için aynı ekseni seçme.';
    }else{
      $('mappingStatus').textContent='Özel eşleme seçildi; analog hareketlerini kontrol et.';
    }
    updateDeviceInfo();
  }));
  $('previewBtn').addEventListener('click',beginPreview);
  $('previewStopBtn').addEventListener('click',()=>stopPreview('Deneme durduruldu; eksik örnek kaydedilmedi.'));
  $('previewExportBtn').addEventListener('click',downloadPreview);
  $('previewSendBtn').addEventListener('click',uploadPreview);
  $('previewConsent').addEventListener('change',()=>{
    $('previewSendBtn').disabled=!($('previewConsent').checked&&state.uploadEnabled&&
      state.previewSnapshot&&!state.previewUploaded);
  });
  $('startBtn').addEventListener('click',begin);
  $('stopBtn').addEventListener('click',()=>abort('Test durduruldu.'));
  $('exportBtn').addEventListener('click',exportResult);
  $('uploadBtn').addEventListener('click',upload);
  $('uploadConsent').addEventListener('change',()=>{$('uploadBtn').disabled=!
    (state.receipt&&state.uploadEnabled&&!state.uploaded&&$('uploadConsent').checked);});
  window.addEventListener('gamepadconnected',refreshDevices);
  window.addEventListener('gamepaddisconnected',()=>{
    if(state.mode==='running')abort('Kol bağlantısı koptu; test iptal edildi.');
    if(state.previewMode==='capturing')stopPreview('Kol bağlantısı koptu; önizleme iptal edildi.');
    refreshDevices();
  });
  document.addEventListener('visibilitychange',()=>{
    if(document.hidden&&state.mode==='running')abort('Sekme arka plana geçti; test iptal edildi.');
    if(document.hidden&&state.previewMode==='capturing')stopPreview('Sekme arka plana geçti; önizleme iptal edildi.');
  });
  refreshDevices();
  setInterval(refreshDevices,1500);
  requestAnimationFrame(frame);
  fetchStatus();
})();
