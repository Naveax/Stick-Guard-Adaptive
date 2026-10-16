'use strict';
// GuardAdaptive Remote Lab: passive browser Gamepad API axis observation only.
// No vibration, calibration, game input writes, device ID upload or auto-send.
(() => {
  const $ = id => document.getElementById(id);
  const PHASES = [
    {name:'Sol analogu serbest bırak',tip:'Kontrolcüyü sabit tut. Sol analoga 12 saniye boyunca dokunma. Gözlem yalnız kendi beyanına dayanır.',icon:'◎',length:12000},
    {name:'Sol analog: küçük hareketler',tip:'Sol analogu merkeze yakın, çok küçük ve bilinçli hareketlerle kontrol et. Bu hareketleri driftle karıştırmamak için ölçüyoruz.',icon:'↗',length:12000},
    {name:'Sol analog: tam tur',tip:'Sol analogla yavaşça dış çember boyunca tam tur yap. Mümkünse farklı yönlerde hareket ettir.',icon:'◌',length:12000},
    {name:'Sağ analogu serbest bırak',tip:'Kontrolcüyü sabit tut. Sağ analoga 12 saniye boyunca dokunma.',icon:'◎',length:12000},
    {name:'Sağ analog: küçük hareketler',tip:'Sağ analogu merkeze yakın, küçük ve kasıtlı hareketlerle kontrol et.',icon:'↗',length:12000},
    {name:'Sağ analog: tam tur',tip:'Sağ analogla yavaşça dış çember boyunca tur yap, sonra merkeze bırak.',icon:'◌',length:12000}
  ];
  const state = {
    selected:null, mode:'idle', samples:[], capture:null, sessionStart:0,
    activeGamepadId:null, runToken:0, receipt:null, finished:false,
    countSinceTick:0, fpsLast:0, fps:0, uploads:false, statusLoaded:false
  };
  let paintCount=0;
  const phaseStepNodes=[...$('phaseSteps').children];
  function message(text,isError=false) {
    $('testMessage').textContent=text;
    $('testMessage').classList.toggle('error',isError);
  }
  function currentPads() {
    if(!('getGamepads' in navigator))return [];
    try { return [...(navigator.getGamepads() || [])].filter(g => g && g.connected); }
    catch { return []; }
  }
  function getChosen() {
    return currentPads().find(g=>String(g.index)===String(state.selected)) || null;
  }
  function compatible(g) {
    return !!g && g.mapping==='standard' && g.axes.length>=4 &&
      [0,1,2,3].every(i=>Number.isFinite(g.axes[i]) && Math.abs(g.axes[i])<=1.001);
  }
  function updateDevices() {
    const pads=currentPads(), selector=$('controllerSelect');
    const selected=state.selected;
    selector.replaceChildren();
    if(!pads.length){
      const opt=document.createElement('option');
      opt.value='';opt.textContent='Henüz kontrolcü algılanmadı';
      selector.append(opt);
      if(state.mode==='idle'||state.mode==='done')state.selected=null;
    }else{
      for(const gp of pads){
        const opt=document.createElement('option');
        opt.value=String(gp.index);
        opt.textContent=gp.id || 'Oyun kontrolcüsü '+(gp.index+1);
        selector.append(opt);
      }
      const matches=pads.some(g=>String(g.index)===String(selected));
      if(matches)selector.value=String(selected);
      else {
        state.selected=String(pads[0].index);
        selector.value=state.selected;
      }
    }
    syncDeviceStatus();
  }
  function syncDeviceStatus(){
    const g=getChosen(), badge=$('deviceBadge'),supported=compatible(g);
    badge.className='pill '+(supported?'ok':'warn');
    badge.innerHTML='<span class="dot"></span> '+(supported?'Bağlandı':'Bekleniyor');
    $('mappingText').textContent=g?(g.mapping||'eşleme yok'):'-';
    $('axesCount').textContent=g?String(g.axes.length):'-';
    $('fpsText').textContent=g?String(state.fps)+' FPS':'-';
    let notice='';
    if(!window.isSecureContext)notice='Bu site HTTPS veya localhost üzerinde açılmalı. Güvenli olmayan sayfalarda Gamepad API erişimi kısıtlanabilir.';
    else if(!('getGamepads' in navigator))notice='Bu tarayıcı Gamepad API desteklemiyor. Güncel masaüstü Chrome, Edge veya Firefox ile dene.';
    else if(!g)notice='Kontrolcünü USB/Bluetooth ile bağla. Sekme açıkken kol üzerindeki bir düğmeye bas veya bir analogu hareket ettir.';
    else if(!compatible(g))notice='Bu kontrolcü en az dört eksenli standart Gamepad eşlemesi sunmuyor. Yanlış analog ölçümü riskinden dolayı bu protokolde test edilemiyor.';
    else notice='Standart kontrolcü algılandı. Canlı analog konumlarını aşağıda görebilirsin.';
    $('compatNotice').textContent=notice;
    $('compatNotice').classList.toggle('error',!!g&&!supported);
    $('startBtn').disabled=state.mode!=='idle' && state.mode!=='done';
  }
  function drawStick(canvas,x,y,color){
    const ctx=canvas.getContext('2d');
    if(!ctx)return;
    const w=canvas.width,h=canvas.height,cx=w/2,cy=h/2,r=117;
    ctx.clearRect(0,0,w,h);
    ctx.strokeStyle='#31516a';ctx.lineWidth=2;
    [1,.65,.28].forEach(t=>{
      ctx.beginPath();ctx.arc(cx,cy,r*t,0,Math.PI*2);ctx.stroke();
    });
    ctx.strokeStyle='#375871';ctx.lineWidth=1.5;
    ctx.beginPath();ctx.moveTo(cx-r-7,cy);ctx.lineTo(cx+r+7,cy);
    ctx.moveTo(cx,cy-r-7);ctx.lineTo(cx,cy+r+7);ctx.stroke();
    ctx.fillStyle='#67d6dd24';ctx.beginPath();ctx.arc(cx,cy,r*.08,0,Math.PI*2);ctx.fill();
    const px=cx+x*r,py=cy+y*r;
    ctx.strokeStyle=color+'88';ctx.lineWidth=1.7;
    ctx.beginPath();ctx.moveTo(cx,cy);ctx.lineTo(px,py);ctx.stroke();
    ctx.shadowColor=color;ctx.shadowBlur=17;ctx.fillStyle=color;
    ctx.beginPath();ctx.arc(px,py,8,0,Math.PI*2);ctx.fill();ctx.shadowBlur=0;
    ctx.fillStyle='#dffaff';ctx.beginPath();ctx.arc(px,py,2.5,0,Math.PI*2);ctx.fill();
  }
  function showAnalog(g) {
    const a=compatible(g)?g.axes:[0,0,0,0];
    const round=v=>Number(v).toFixed(3);
    $('lxValue').textContent='X '+round(a[0]);$('lyValue').textContent='Y '+round(a[1]);
    $('rxValue').textContent='X '+round(a[2]);$('ryValue').textContent='Y '+round(a[3]);
    $('leftValue').textContent=Math.hypot(a[0],a[1]).toFixed(3);
    $('rightValue').textContent=Math.hypot(a[2],a[3]).toFixed(3);
    drawStick($('leftCanvas'),a[0],a[1],'#75e4ed');
    drawStick($('rightCanvas'),a[2],a[3],'#ada6ff');
  }
  function progress(index,frac=0){
    const amount=Math.min(1,Math.max(0,(index+frac)/PHASES.length));
    $('progressFill').style.width=(amount*100).toFixed(2)+'%';
    $('stepMarker').textContent=Math.min(index+1,6)+' / 6';
    phaseStepNodes.forEach((el,i)=>{
      el.classList.toggle('done',i<index);
      el.classList.toggle('active',i===index);
    });
  }
  function stopRecording(explanation,isError=false) {
    state.runToken++;
    if(state.capture){
      const task=state.capture;
      state.capture=null;
      task.reject(new Error('STOPPED'));
    }
    state.mode='idle';state.samples=[];state.finished=false;state.receipt=null;
    $('stopBtn').disabled=true;
    $('startBtn').disabled=false;
    $('startBtn').textContent='Ölçümü başlat →';
    $('resultPanel').classList.add('hidden');
    $('uploadConsent').checked=false;
    $('uploadBtn').disabled=true;
    progress(0,0);
    $('timer').textContent='00:00';
    $('phaseIcon').textContent='◎';
    $('phaseTitle').textContent='Teste hazır';
    $('phaseDescription').textContent='Test kesildi. Yeni bir ölçüm başlatmak için bilgileri kontrol et.';
    message(explanation,isError);
  }
  function loop(now){
    const g=getChosen();
    if(paintCount++%4===0)showAnalog(g);
    if(g && compatible(g)){
      state.countSinceTick++;
      if(now-state.fpsLast>=1000){
        state.fps=state.countSinceTick;
        state.countSinceTick=0;state.fpsLast=now;
        $('fpsText').textContent=state.fps+' FPS';
      }
    }
    const task=state.capture;
    if(task){
      if(!compatible(g) || g.id!==state.activeGamepadId || document.hidden){
        stopRecording('Kontrolcü bağlantısı veya sayfa görünürlüğü değişti. Hatalı veri göndermemek için test iptal edildi.',true);
      } else {
        const elapsed=now-task.startedAt;
        if(elapsed>=task.length) {
          state.capture=null;
          task.resolve();
        }else {
          $('timer').textContent=('0'+Math.floor((task.length-elapsed)/1000)).slice(-2)+' sn';
          progress(task.phase,elapsed/task.length);
          if(now-task.lastCapture>=16){
            const row=[task.phase,Math.round(now-state.sessionStart),...g.axes.slice(0,4).map(a=>Math.max(-1,Math.min(1,Math.round(a*10000)/10000)))];
            // Gap/dropout handling in final server-side validation, no fabricated samples.
            if(row[1] > (state.samples.at(-1)?.[1]??-1) && state.samples.length<9000)state.samples.push(row);
            task.lastCapture=now;
          }
        }
      }
    }
    requestAnimationFrame(loop);
  }
  async function countDown(phase,token){
    for(let second=3;second>0;second--){
      if(state.runToken!==token)throw new Error('STOPPED');
      $('phaseTitle').textContent=PHASES[phase].name;
      $('phaseIcon').textContent=PHASES[phase].icon;
      $('phaseDescription').textContent=PHASES[phase].tip;
      $('timer').textContent=String(second)+' sn';
      message('Hazırlan: '+second+' saniye sonra kayıt başlıyor.');
      await new Promise(resolve=>setTimeout(resolve,1000));
      if(state.runToken!==token)throw new Error('STOPPED');
      if(document.hidden)throw new Error('TAB_HIDDEN');
    }
  }
  function recordingPhase(phase,token){
    if(state.runToken!==token)return Promise.reject(new Error('STOPPED'));
    return new Promise((resolve,reject)=>{
      if(state.capture){reject(new Error('ALREADY_RECORDING'));return;}
      const now=performance.now();
      state.capture={phase,startedAt:now,lastCapture:now-20,length:PHASES[phase].length,resolve,reject};
      message('Kayıt sürüyor. Yönergeyi uygula ve sekmeyi değiştirme.');
    });
  }
  async function begin(){
    if(state.mode!=='idle' && state.mode!=='done')return;
    const g=getChosen();
    if(!compatible(g)){message('En az dört eksenli standart bir kontrolcü gerekli.',true);return;}
    const family=$('family').value,condition=$('condition').value,transport=$('transport').value;
    if(!family||!condition||!transport){message('Kontrolcü ailesi, durumu ve bağlantı türünü seç.',true);return;}
    if(!$('ackConsent').checked){message('Ölçümün araştırma niteliğini anladığını onayla.',true);return;}
    if(document.hidden){message('Teste başlamak için sekmeyi görünür tut.',true);return;}
    state.mode='running';state.samples=[];state.receipt=null;state.finished=false;
    state.activeGamepadId=g.id;
    state.sessionStart=performance.now();
    const token=++state.runToken;
    $('resultPanel').classList.add('hidden');
    $('startBtn').disabled=true;$('stopBtn').disabled=false;
    $('controllerSelect').disabled=true;
    for(const id of ['family','condition','transport'])$(id).disabled=true;
    try {
      for(let phase=0;phase<PHASES.length;phase++){
        progress(phase,0);
        await countDown(phase,token);
        await recordingPhase(phase,token);
        const count=state.samples.filter(row=>row[0]===phase).length;
        if(count<80)throw new Error('TOO_FEW_SAMPLES');
      }
      if(state.runToken!==token)throw new Error('STOPPED');
      finish();
    }catch(err){
      if(state.runToken===token)stopRecording(err.message==='TOO_FEW_SAMPLES'?'Örnekleme yetersizdi. Sekmeyi açık tutarak yeniden dene.':'Test durduruldu; eksik veri kaydedilmedi.',true);
    }finally {
      $('controllerSelect').disabled=false;
      for(const id of ['family','condition','transport'])$(id).disabled=false;
    }
  }
  function rms(rows,phase,a,b){
    const values=rows.filter(r=>r[0]===phase).map(r=>Math.hypot(r[a],r[b]));
    if(!values.length)return NaN;
    return Math.sqrt(values.reduce((sum,v)=>sum+v*v,0)/values.length);
  }
  function finish(){
    state.mode='done';state.finished=true;state.capture=null;
    $('startBtn').disabled=false;$('startBtn').textContent='Yeni test yap →';
    $('stopBtn').disabled=true;
    progress(5,1);
    $('timer').textContent='Bitti';
    $('phaseTitle').textContent='Ölçüm tamamlandı';
    $('phaseDescription').textContent='Verilerin yalnız tarayıcı belleğinde. İndir veya ayrıca onay vererek yükle.';
    const g=getChosen();
    // No gamepad.id, serial, IP address, User-Agent, location, notes or cookies in dataset.
    state.receipt={
      schema:'ga-remote-lab-v1',protocol:'GA-RL-01',
      session_id:crypto.randomUUID(),
      controller:{family:$('family').value,condition:$('condition').value,transport:$('transport').value},
      client:{mapping:'standard',version:'0.1.0'},
      consent:{information_ack:true,upload:false},
      samples:state.samples
    };
    $('leftRms').textContent=rms(state.samples,0,2,3).toFixed(4);
    $('rightRms').textContent=rms(state.samples,3,4,5).toFixed(4);
    $('sampleCount').textContent=state.samples.length.toLocaleString('tr-TR');
    $('resultPanel').classList.remove('hidden');
    $('uploadConsent').checked=false;
    $('uploadBtn').disabled=true;
    $('uploadStatus').textContent=state.uploads?'Gönüllü paylaşım isteğe bağlı.':'Sunucu yüklemesi kapalı; JSON indirebilirsin.';
    message('Tamamlandı. Ölçümleri önce incele, ardından istediğin seçeneği kullan.');
    $('resultPanel').scrollIntoView({behavior:'smooth',block:'nearest'});
  }
  function exportData(){
    if(!state.receipt)return;
    const json=JSON.stringify(state.receipt);
    const url=URL.createObjectURL(new Blob([json],{type:'application/json'}));
    const a=document.createElement('a');a.href=url;
    a.download='guardadaptive-test-'+state.receipt.session_id+'.json';
    document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),3000);
  }
  async function loadStatus(){
    try{
      const r=await fetch('/api/status',{cache:'no-store'});
      if(!r.ok)throw Error('unavailable');
      const obj=await r.json();
      state.uploads=obj.upload_enabled===true;
      state.statusLoaded=true;
    }catch{
      state.uploads=false;state.statusLoaded=false;
    }
    $('uploadStatus').textContent=state.uploads?'Gönüllü yükleme etkin.':'Sunucu yüklemesi kapalı; JSON indirebilirsin.';
    $('uploadBtn').disabled=!state.uploads||!$('uploadConsent').checked||!state.receipt;
  }
  async function uploadData(){
    if(!state.receipt||!state.uploads||!$('uploadConsent').checked)return;
    const btn=$('uploadBtn');btn.disabled=true;
    $('uploadStatus').textContent='Veriler gönderiliyor...';
    try{
      const obj={...state.receipt,consent:{information_ack:true,upload:true}};
      const res=await fetch('/api/submissions',{
        method:'POST',headers:{'Content-Type':'application/json'},
        cache:'no-store',credentials:'omit',referrerPolicy:'no-referrer',
        body:JSON.stringify(obj)
      });
      const result=await res.json();
      if(!res.ok)throw new Error(result.error||'UPLOAD_FAILED');
      $('uploadStatus').textContent='Kabul edildi. Kayıt no: '+result.receipt_id.slice(0,8)+'…';
      $('uploadConsent').checked=false;state.uploads=false;
    }catch(err){
      $('uploadStatus').textContent='Gönderilemedi ('+err.message+'). Veriyi JSON olarak indirebilirsin.';
      btn.disabled=false;
    }
  }
  $('controllerSelect').addEventListener('change',e=>{if(state.mode==='running')return;state.selected=e.target.value;syncDeviceStatus();});
  $('startBtn').addEventListener('click',begin);
  $('stopBtn').addEventListener('click',()=>stopRecording('Ölçüm isteğin üzerine durduruldu.'));
  $('exportBtn').addEventListener('click',exportData);
  $('uploadConsent').addEventListener('change',()=>{$('uploadBtn').disabled=!state.receipt||!state.uploads||!$('uploadConsent').checked;});
  $('uploadBtn').addEventListener('click',uploadData);
  window.addEventListener('gamepadconnected',updateDevices);
  window.addEventListener('gamepaddisconnected',()=>{
    if(state.mode==='running')stopRecording('Kontrolcü bağlantısı kesildi. Test iptal edildi.',true);
    updateDevices();
  });
  document.addEventListener('visibilitychange',()=>{
    if(document.hidden && state.mode==='running')stopRecording('Sekme arka plana geçti; kayıt iptal edildi.',true);
  });
  updateDevices();
  setInterval(()=>{if(state.mode!=='running')updateDevices();},1500);
  requestAnimationFrame(loop);
  loadStatus();
})();
