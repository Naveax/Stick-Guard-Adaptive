'use strict';
(()=>{
 const $=id=>document.getElementById(id);let token='';
 const headers=()=>({Authorization:'Bearer '+token});
 async function request(u,method='GET'){
   const r=await fetch(u,{method,credentials:'omit',cache:'no-store',headers:headers()});
   if(!r.ok)throw Error('HTTP '+r.status);
   return r.json();
 }
 async function refresh(){
   const out=await request('/api/admin/sessions'),rows=$('rows');rows.replaceChildren();
   for(const item of out.records){
     const tr=document.createElement('tr');
     for(const v of [item.id.slice(0,8)+'…',item.controller.family,item.controller.condition,
       item.left_idle_rms?.toFixed(4)??'-',item.right_idle_rms?.toFixed(4)??'-',item.sample_count]){
       const td=document.createElement('td');td.textContent=String(v);td.style.padding='12px 8px';td.style.borderBottom='1px solid #30435a';tr.append(td);
     }
     const td=document.createElement('td'),download=document.createElement('button'),del=document.createElement('button');
     download.className='secondary';download.textContent='JSON';
     del.className='secondary';del.textContent='Sil';
     download.onclick=async()=>{
       try{
        const data=await request('/api/admin/sessions/'+item.id);
        const u=URL.createObjectURL(new Blob([JSON.stringify(data,null,2)],{type:'application/json'}));
        const a=document.createElement('a');a.href=u;a.download='ga-'+item.id+'.json';
        document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(u),1500);
       }catch(e){$('error').textContent=e.message;}
     };
     del.onclick=async()=>{if(!confirm('Bu ölçüm kalıcı olarak silinsin mi?'))return;
       try{await request('/api/admin/sessions/'+item.id,'DELETE');await refresh();}catch(e){$('error').textContent=e.message;}
     };
     td.append(download,del);tr.append(td);rows.append(tr);
   }
   $('error').textContent=out.records.length+' kayıt. Katılımcı etiketleri doğrulanmamıştır.';
 }
 $('signin').onclick=async()=>{
   const entered=$('token').value;
   if(entered.length<32){$('loginmsg').textContent='Geçerli bir yönetici tokenı gir.';return;}
   token=entered;$('token').value='';
   try{await refresh();$('login').classList.add('hidden');$('records').classList.remove('hidden');}
   catch(e){token='';$('loginmsg').textContent='Erişim reddedildi: '+e.message;}
 };
 $('refresh').onclick=()=>refresh().catch(e=>{$('error').textContent=e.message;});
 $('logout').onclick=()=>{token='';$('records').classList.add('hidden');$('login').classList.remove('hidden');$('rows').replaceChildren();};
})();