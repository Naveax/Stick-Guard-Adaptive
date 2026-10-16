'use strict';
(()=>{
 const $=id=>document.getElementById(id);let token='';
 const text=(tag,value)=>{const e=document.createElement(tag);e.textContent=value;return e;};
 async function request(url,options={}){
  if(!token)throw Error('Yönetici anahtarı gerekli');
  const res=await fetch(url,{...options,credentials:'omit',cache:'no-store',
   headers:{Authorization:'Bearer '+token,...(options.headers||{})}});
  if(!res.ok)throw Error('HTTP '+res.status);
  return res.json();
 }
 function download(data,id){
  const b=new Blob([JSON.stringify(data,null,2)],{type:'application/json'});
  const u=URL.createObjectURL(b),a=document.createElement('a');a.href=u;
  a.download='ga-study-'+id+'.json';document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(u),3000);
 }
 function format(m){
  if(!m)return 'Veri yok';
  const pct=n=>n==null?'belirsiz':(n>=0?'+':'')+n.toFixed(1)+'%';
  return m.stick_metrics.map(s=>(s.stick==='left'?'Sol':'Sağ')+': drift azalması '+pct(s.idle_reduction_percent)+
   ', küçük hareket kaybı '+pct(s.intentional_motion_loss_percent)+' ('+s.finding+')').join(' | ');
 }
 async function refresh(){
  const answer=await request('/api/admin/studies');
  $('records').replaceChildren();
  $('count').textContent=answer.studies.length+' karşılaştırmalı test. Bağımsız fiziksel niyet doğrulanmadı.';
  for(const s of answer.studies){
   const card=document.createElement('article');card.className='result-card';
   card.append(text('div',s.mode==='simulation'?'SANAL FİLTRE':'KATILIMCI BEYANLI ÖNCE/SONRA'));
   card.append(text('p','Kayıt '+s.id.slice(0,12)+'… • '+s.controller.family+' • '+s.controller.condition));
   card.append(text('p','Ölçüm: '+s.before_frames+' + '+s.after_frames+' örnek'));
   card.append(text('p',format(s.summary)));
   card.append(text('p','Anket: drift '+s.survey.baseline_severity+'/10, oyun etkisi '+s.survey.subjective_impact+
    ', his '+s.survey.perceived_effect+', küçük hareket '+s.survey.micro_motion+', öneri '+s.survey.recommendation));
   const b=document.createElement('button');b.className='secondary';b.textContent='JSON indir';
   b.addEventListener('click',async()=>{
    try{download(await request('/api/admin/studies/'+s.id),s.id);}catch(e){$('notice').textContent=e.message;}
   });
   const del=document.createElement('button');del.className='secondary';del.textContent='Sil';
   del.addEventListener('click',async()=>{
    if(!confirm('Bu karşılaştırmalı test, anket ve tüm ölçümleri kalıcı olarak silinsin mi?'))return;
    try{await request('/api/admin/studies/'+s.id,{method:'DELETE'});await refresh();$('notice').textContent='Kayıt silindi.';}
    catch(e){$('notice').textContent=e.message;}
   });
   const controls=document.createElement('div');controls.className='compare-actions';controls.append(b,del);
   card.append(controls);$('records').append(card);
  }
  if(!answer.studies.length)$('records').append(text('p','Henüz gönüllü karşılaştırmalı kayıt bulunmuyor.'));
 }
 $('login').addEventListener('click',async()=>{
  const entry=$('token').value;if(entry.length<32){$('authMessage').textContent='En az 32 karakter gerekli.';return;}
  token=entry;$('token').value='';
  try{await refresh();$('authPanel').classList.add('hidden');$('reviewPanel').classList.remove('hidden');}
  catch(e){token='';$('authMessage').textContent='Giriş başarısız: '+e.message;}
 });
 $('refresh').addEventListener('click',()=>refresh().catch(e=>{$('notice').textContent=e.message;}));
 $('logout').addEventListener('click',()=>{token='';$('reviewPanel').classList.add('hidden');$('authPanel').classList.remove('hidden');$('records').replaceChildren();});
})();
