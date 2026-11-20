(() => {
  const cfg=__PICKUP_CONFIG__;
  if(window.top!==window.self||location.origin!=='https://icsms.top'||location.pathname!=='/pickup')return;
  let last='',lastRefresh=0;
  const visible=e=>e&&e.getClientRects().length>0;
  function send(value){const raw=JSON.stringify({pickupHelper:true,id:cfg.id,...value});if(raw!==last){last=raw;try{window.chrome?.webview?.postMessage(raw);}catch{}}}
  function read(){
    const current=document.querySelector('.pickup-page-account-identity strong')?.textContent?.trim();
    if(current&&current.toLowerCase()!==cfg.base.toLowerCase()){send({error:'取信页显示了其他邮箱，已停止取信。'});return;}
    if(Array.from(document.querySelectorAll('iframe')).some(e=>visible(e)&&/captcha|turnstile|challenge/i.test(e.src||e.title||''))||/verify you are human|checking your browser|just a moment|performing security verification|请完成人机验证/i.test(document.body?.innerText||'')){send({challenge:true});return;}
    const article=document.querySelector('article.pickup-message');if(!article)return;
    const rows={};for(const dt of article.querySelectorAll('dt'))rows[dt.textContent.trim()]=dt.nextElementSibling?.textContent?.trim()||'';
    const rawDate=rows['邮箱收件时间']||rows['发件时间']||'';
    const d=rawDate.match(/(\d{4})年(\d{1,2})月(\d{1,2})日\s*(\d{1,2}):(\d{2}):(\d{2})/);
    const date=d?new Date(+d[1],+d[2]-1,+d[3],+d[4],+d[5],+d[6]).toISOString():'';
    const frame=article.querySelector('iframe.message-html-frame');
    const html=frame?.getAttribute('srcdoc')||'';
    const plain=html?new DOMParser().parseFromString(html,'text/html').body.textContent:article.querySelector('.pickup-message-body')?.textContent||'';
    const subject=article.querySelector('h1')?.textContent?.trim()||'';
    send({payload:{ok:true,data:{alias:current||cfg.base,message:{uid:subject+'|'+date,subject,from:rows['发件人']||'',to:rows['收件人']||'',date,text:String(plain).slice(0,12000),html:html.slice(0,35000)}}}});
  }
  window.__pickupRefresh=()=>{read();const now=Date.now();if(now-lastRefresh<5000)return;const buttons=Array.from(document.querySelectorAll('button')).filter(e=>visible(e)&&!e.disabled&&e.textContent.trim()==='刷新最新邮件');if(buttons.length===1){lastRefresh=now;buttons[0].click();}};
  setInterval(read,1000);
})();
