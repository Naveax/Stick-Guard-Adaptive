'use strict';
(()=>{
  const $=id=>document.getElementById(id);
  let secret=null;
  function feedback(text,error=false){
    $('recordsFeedback').textContent=text;
    $('recordsFeedback').classList.toggle('error',error);
  }
  async function adminRequest(url,options={}){
    if(!secret)throw new Error('Yönetici oturumu açık değil.');
    const r=await fetch(url,{
      ...options,credentials:'omit',cache:'no-store',referrerPolicy:'no-referrer',
      headers:{Authorization:'Bearer '+secret,...(options.headers||{})}
    });
    if(!r.ok)throw new Error(r.status===401?'Anahtar yanlış veya yetkisiz.':'Sunucu yanıtı: '+r.status);
    return r.json();
  }
  function cell(row,value){
    const td=document.createElement('td');
    td.textContent=value;
    row.append(td);
    return td;
  }
  function downloadJson(obj,id){
    const blob=new Blob([JSON.stringify(obj,null,2)],{type:'application/json'});
    const url=URL.createObjectURL(blob);
    const anchor=document.createElement('a');anchor.href=url;
    anchor.download='guardadaptive-'+id+'.json';
    document.body.append(anchor);anchor.click();anchor.remove();
    setTimeout(()=>URL.revokeObjectURL(url),4000);
  }
  async function refreshPreview(){
    const panel=$('previewAdminPanel'),body=$('previewRecordsBody');
    try{
      const status=await fetch('/api/status',{cache:'no-store'}).then(res=>res.json());
      if(status.preview_supported!==true){panel.classList.add('hidden');return;}
      const result=await adminRequest('/api/admin/preview-feedback');
      panel.classList.remove('hidden');body.replaceChildren();
      if(!result.items.length){
        const tr=document.createElement('tr'),td=document.createElement('td');
        td.colSpan=8;td.className='empty-records';
        td.textContent='Henüz filtre önizlemesi veya anket kaydı yok.';
        tr.append(td);body.append(tr);return;
      }
      for(const item of result.items){
        const tr=document.createElement('tr');
        cell(tr,item.session_id.slice(0,8)+'…');
        cell(tr,item.reported_severity);
        cell(tr,item.benefit+'/5');
        cell(tr,item.interference);
        cell(tr,item.problem+' / '+item.affected);
        cell(tr,item.latency);
        cell(tr,item.rating+'/5');
        const actions=document.createElement('td');
        const download=document.createElement('button');download.type='button';download.textContent='JSON ↓';
        download.addEventListener('click',async()=>{
          download.disabled=true;
          try{
            const doc=await adminRequest('/api/admin/preview-feedback/'+item.session_id);
            downloadJson(doc,item.session_id);
          }catch(e){$('previewRecordsFeedback').textContent=e.message;}
          finally{download.disabled=false;}
        });
        const del=document.createElement('button');del.type='button';del.className='danger';del.textContent='Sil';
        del.addEventListener('click',async()=>{
          if(!window.confirm('Yalnız bu önizleme ve anket kaydı kalıcı olarak silinsin mi?'))return;
          del.disabled=true;
          try{
            await adminRequest('/api/admin/preview-feedback/'+item.session_id,{method:'DELETE'});
            await refreshPreview();
          }catch(e){$('previewRecordsFeedback').textContent=e.message;del.disabled=false;}
        });
        actions.append(download,del);tr.append(actions);body.append(tr);
      }
      $('previewRecordsFeedback').textContent=result.items.length+
        ' önizleme anketi. Oyun girdisine müdahale edilmedi, güvenlik kanıtı değil.';
    }catch(e){
      $('previewRecordsFeedback').textContent=e.message;
    }
  }
  async function refresh(){
    const result=await adminRequest('/api/admin/sessions');
    $('recordCount').textContent=String(result.items.length);
    const body=$('recordsBody');body.replaceChildren();
    if(!result.items.length){
      const tr=document.createElement('tr'),td=document.createElement('td');
      td.colSpan=8;td.className='empty-records';td.textContent='Henüz gönüllü gönderilmiş ölçüm yok.';
      tr.append(td);body.append(tr);
      await refreshPreview();return;
    }
    for(const item of result.items){
      const tr=document.createElement('tr');
      cell(tr,item.session_id.slice(0,8)+'…');
      cell(tr,item.family);
      cell(tr,item.reported_condition);
      cell(tr,item.left_idle.rms?.toFixed(4)??'-');
      cell(tr,item.right_idle.rms?.toFixed(4)??'-');
      cell(tr,String(item.sample_count));
      const warningCell=cell(tr,item.quality_flags?.length?item.quality_flags.length+' uyarı':'İnceleme gerekli');
      warningCell.title=(item.quality_flags||[]).join(', ')||'Fiziksel niyet doğrulanmadı';
      const actions=document.createElement('td');
      const get=document.createElement('button');get.type='button';get.textContent='JSON ↓';
      get.addEventListener('click',async()=>{
        get.disabled=true;
        try{const doc=await adminRequest('/api/admin/sessions/'+item.session_id);
          downloadJson(doc,item.session_id);feedback('JSON dosyası indirildi.');}
        catch(e){feedback(e.message,true);}
        finally{get.disabled=false;}
      });
      const del=document.createElement('button');del.type='button';del.className='danger';del.textContent='Sil';
      del.addEventListener('click',async()=>{
        if(!window.confirm('Bu anonim ölçüm kalıcı olarak silinsin mi? İşlem geri alınamaz.'))return;
        del.disabled=true;
        try{
          await adminRequest('/api/admin/sessions/'+item.session_id,{method:'DELETE'});
          await refresh();feedback('Kayıt silindi.');
        }catch(e){feedback(e.message,true);del.disabled=false;}
      });
      actions.append(get,del);tr.append(actions);body.append(tr);
    }
    feedback('Kayıt listesi yenilendi. Fiziksel kullanıcı niyeti doğrulanmamıştır.');
    await refreshPreview();
  }
  $('loginBtn').addEventListener('click',async()=>{
    const key=$('adminToken').value;
    if(key.length<32){$('loginFeedback').textContent='En az 32 karakterlik yönetici anahtarı gerekli.';return;}
    secret=key;$('adminToken').value='';
    $('loginBtn').disabled=true;
    try{
      await refresh();
      $('loginPanel').classList.add('hidden');
      $('recordsPanel').classList.remove('hidden');
    }catch(e){
      secret=null;$('loginFeedback').textContent=e.message;
    }finally{$('loginBtn').disabled=false;}
  });
  $('refreshBtn').addEventListener('click',()=>refresh().catch(e=>feedback(e.message,true)));
  $('logoutBtn').addEventListener('click',()=>{
    $('previewAdminPanel').classList.add('hidden');
    $('previewRecordsBody').replaceChildren();
    secret=null;$('recordsPanel').classList.add('hidden');
    $('loginPanel').classList.remove('hidden');$('recordsBody').replaceChildren();
  });
})();
