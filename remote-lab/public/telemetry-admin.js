'use strict';(()=>{
 const $=id=>document.getElementById(id);
 let token=null;
 function note(v){$('notice').textContent=v;}
 async function req(url,options={}){
  if(!token)throw Error('Yönetici anahtarı gerekli.');
  const response=await fetch(url,{cache:'no-store',headers:{Authorization:'Bearer '+token},...options});
  const data=await response.json();
  if(!response.ok)throw Error(data.error||'HTTP '+response.status);
  return data;
 }
 function button(text,handler,secondary=false){
  const b=document.createElement('button');b.type='button';b.className=secondary?'secondary':'primary';
  b.textContent=text;b.addEventListener('click',handler);return b;
 }
 function cell(tr,text){const td=document.createElement('td');td.textContent=String(text);td.style.padding='10px';td.style.borderBottom='1px solid #345062';tr.append(td);return td;}
 async function load(){
  try{
   const data=await req('/api/admin/telemetry');
   const box=$('records');box.replaceChildren();
   const table=document.createElement('table');table.style.cssText='border-collapse:collapse;width:100%;font-size:12px';
   const header=document.createElement('tr');
   for(const h of ['Kayıt','Kategori','Örnek','Sol ham / filtreli','Sağ ham / filtreli','Anket','İşlemler']){
    const th=document.createElement('th');th.textContent=h;th.style.cssText='padding:10px;text-align:left';header.append(th);
   }
   table.append(header);
   for(const item of data.items){
    const tr=document.createElement('tr');
    cell(tr,item.session_id.slice(0,8)+'…');
    cell(tr,item.condition);cell(tr,item.sample_count);
    const pct=x=>x==null?'—':x.toFixed(2)+'%';
    cell(tr,pct(item.left.rest_raw_rms_percent)+' / '+pct(item.left.rest_filtered_rms_percent));
    cell(tr,pct(item.right.rest_raw_rms_percent)+' / '+pct(item.right.rest_filtered_rms_percent));
    cell(tr,item.survey_completed?'Dolu':'Boş');
    const actions=document.createElement('td');actions.style.padding='8px';
    actions.append(button('JSON indir',async()=>{
     try{
      const data=await req('/api/admin/telemetry/'+item.session_id);
      const url=URL.createObjectURL(new Blob([JSON.stringify(data,null,2)],{type:'application/json'}));
      const a=document.createElement('a');a.href=url;a.download='ga-'+item.session_id+'.json';a.click();
      setTimeout(()=>URL.revokeObjectURL(url),3000);
     }catch(e){note(e.message);}
    }));
    actions.append(document.createTextNode(' '));
    actions.append(button('Sil',async()=>{
     if(!confirm('Bu kayıt tamamen silinsin mi?'))return;
     try{await req('/api/admin/telemetry/'+item.session_id,{method:'DELETE'});await load();}
     catch(e){note(e.message);}
    },true));
    tr.append(actions);table.append(tr);
   }
   const wrap=document.createElement('div');wrap.style.overflowX='auto';wrap.append(table);box.append(wrap);
   $('list').classList.remove('hidden');
   note('Kayıt sayısı: '+data.count);
  }catch(e){note('Erişim başarısız: '+e.message);$('list').classList.add('hidden');}
 }
 $('load').addEventListener('click',()=>{const entered=$('adminKey').value.trim();if(entered)token=entered;$('adminKey').value='';load();});
 $('logout').addEventListener('click',()=>{token=null;$('adminKey').value='';$('records').replaceChildren();$('list').classList.add('hidden');note('Oturum kapandı.');});
 window.addEventListener('pagehide',()=>{token=null;});
})();