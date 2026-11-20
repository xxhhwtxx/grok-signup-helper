(() => {
  const cfg = __GROK_CONFIG__;
  if (window.top !== window.self || location.protocol !== 'https:' || !cfg.hosts.includes(location.hostname)) return;
  let stopped=false,last='',pendingCode='',submitted=new Set(),retried=new Map(),completed='',timer,observer,started=false,startedAt=null;
  const managed=new WeakMap();
  let loadedAt=document.readyState==='complete'?Date.now():null;
  window.addEventListener('load',()=>{loadedAt=Date.now();},{once:true});
  const visible=e=>!!e&&e.getClientRects().length>0&&getComputedStyle(e).visibility!=='hidden'&&getComputedStyle(e).display!=='none';
  const all=s=>Array.from(document.querySelectorAll(s)).filter(visible);
  const text=e=>String(e?.innerText||e?.textContent||'').replace(/\s+/g,' ').trim();
  const meta=e=>[e.name,e.id,e.type,e.placeholder,e.autocomplete,e.getAttribute('aria-label'),...(e.labels||[])].map(x=>typeof x==='string'?x:text(x)).join(' ');
  const hydrated=()=> (loadedAt!==null&&Date.now()-loadedAt>=700)||(startedAt!==null&&Date.now()-startedAt>=5000);
  function report(stage,detail='',identity='',force=false){const key=[stage,detail,identity].join('|');if(!force&&key===last)return;last=key;try{window.chrome?.webview?.postMessage(JSON.stringify({grokHelper:true,id:cfg.id,stage,detail,identity}));}catch{}}
  function set(e,value){const setter=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set;setter?setter.call(e,value):e.value=value;e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));}
  function sync(e,value){
    if(!visible(e)||e.disabled)return false;
    const loaded=hydrated();
    let state=managed.get(e);
    if(!state){state={value,manual:false,replay:!loaded,at:Date.now()};managed.set(e,state);set(e,value);return false;}
    if(state.manual)return e.value===value&&Date.now()-state.at>=700;
    if(e.value&&e.value!==state.value){state.manual=true;return false;}
    if(!e.value||(state.replay&&loaded)){state.at=Date.now();state.replay=!loaded;set(e,value);return false;}
    return loaded&&!state.replay&&e.value===value&&Date.now()-state.at>=700;
  }
  document.addEventListener('input',e=>{if(e.isTrusted){const s=managed.get(e.target);if(s){s.manual=true;s.at=Date.now();}}},true);
  function challenge(){
    const body=document.body?.innerText||'';
    if(/verify you are human|complete the verification challenge|验证您是人类|请完成人机验证|拖动滑块|complete the puzzle|确认您是真人/i.test(body))return true;
    // Read completion state only. Never generate, set or export challenge tokens.
    const responses=Array.from(document.querySelectorAll('[name="cf-turnstile-response"], [name="g-recaptcha-response"], [name="h-captcha-response"]'));
    if(responses.length)return !responses.some(e=>String(e.value||'').length>0);
    if(all('form div[style*="64px"] div[style*="rgba(128, 128, 128"]').length)return true;
    const frames=all('iframe').filter(e=>/captcha|turnstile|challenge/i.test(e.src||e.title||''));
    return frames.length>0;
  }
  function clickUnique(pattern,key,scope=document,stage='submitted'){
    const matches=Array.from(scope.querySelectorAll('button, [role="button"], input[type="submit"]')).filter(e=>visible(e)&&!e.disabled&&e.getAttribute('aria-disabled')!=='true'&&pattern.test(e.getAttribute('aria-label')||text(e)||e.value||''));
    if(matches.length!==1||submitted.has(key))return false;
    submitted.add(key);report(stage);matches[0].click();return true;
  }
  const action=/^(?:continue|next|verify|confirm|confirm email|create account|complete sign ?up|complete your sign ?up|complete registration|sign up|register|submit|log in|login|sign in|continue registration|continue sign ?up|继续|下一步|验证|确认|确认邮箱|完成注册|继续注册|创建账号|创建账户|注册|提交|登录)$/i;
  function finish(){
    if(stopped)return true;
    const body=document.body?.innerText||'';
    const onApp=cfg.app_hosts.includes(location.hostname)&&!/^\/(?:auth|sign-)/i.test(location.pathname);
    const onAccount=location.hostname==='accounts.x.ai'&&/^\/account(?:\/|$)/.test(location.pathname);
    if((onApp||onAccount)&&!all('input[type="email"],input[type="password"]').some(e=>!e.readOnly&&!e.disabled)){
      const addresses=[...(body.match(/[a-zA-Z0-9._+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}/g)||[]),...all('input').filter(e=>e.readOnly||e.disabled).map(e=>e.value||'')];
      try{const w=document.createTreeWalker(document.body,4);for(let n;n=w.nextNode();){if(n.nodeValue&&n.nodeValue.includes('@')&&n.parentElement&&visible(n.parentElement)){for(const m of n.nodeValue.match(/[a-zA-Z0-9._+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}/g)||[])addresses.push(m);}}}catch{}
      if(addresses.some(e=>e.toLowerCase()===cfg.email.toLowerCase())&&/log out|sign out|退出登录|退出登陆|登出|settings|设置|account|账户|账号/i.test(body)){
        completed='authenticated';report(completed,'',cfg.email);stopped=true;observer?.disconnect();clearInterval(timer);return true;
      }
    }
    return false;
  }
  function cycle(){
    if(stopped||!document.body)return;
    const body=document.body.innerText||'';
    if(/not available in your (?:region|country)|unsupported country|country.*not supported|所在地区.*不可用|access denied|访问被拒绝|too many requests|请求过于频繁|temporarily blocked/i.test(body)){report('access_blocked','官网限制了当前访问，请处理官网提示后继续。');return;}
    if(challenge()){report('captcha');return;}
    if(finish())return;
    if(/reset.?password|change.?password|forgot.?password/i.test(location.pathname)){report('manual_required','此处要求修改已有密码，请亲自完成。');return;}
    if(cfg.mode==='work'){report('page');return;}
    if(!cfg.consent){report('consent_required');return;}
    const errors=all('[role="alert"], [data-error], .error-message').map(text).filter(Boolean).filter(t=>!/notification|通知|cookie/i.test(t));
    if(errors.length){report('error',errors.join(' · ').slice(0,300));return;}
    const readOnlyEmail=all('input').find(e=>e.readOnly&&String(e.value||'').includes('@'));
    if(readOnlyEmail&&readOnlyEmail.value.toLowerCase()!==cfg.email.toLowerCase()){report('error','官网当前邮箱与所选邮箱不匹配，请结束操作后重新打开。');return;}
    const fields=all('input').filter(e=>!e.disabled&&!e.readOnly&&!['hidden','button','submit','checkbox','radio','search','file'].includes(e.type));
    // Birth dates, legal age, phone/SMS and identity checks require real user data.
    if(fields.some(e=>/birth|birthday|\bdob\b|\bage\b|出生|生日|年龄|phone|mobile|手机|电话/i.test(meta(e)))||all('select').some(e=>/birth|birthday|year|month|day|出生|年月日/i.test(meta(e)))){report('manual_required','请按官网要求填写真实年龄、生日或完成手机验证；完成后自动继续。');return;}
    const otp=fields.filter(e=>/one-time-code|verification.?code|verify.?code|email.?code|\botp\b|passcode|验证码|验证代码/i.test(meta(e))||(/^code$/i.test(e.name)&&/code|验证|check your email|查看.*邮箱/i.test(body))||((e.maxLength===1||e.hasAttribute('data-input-otp'))&&/code|验证|check your email|查看.*邮箱/i.test(body)));
    if(otp.length){
      report('verification');
      if(!pendingCode)return;
      const raw=pendingCode.replace(/-/g,'');
      let ready=false;
      if(otp.length===1)ready=sync(otp[0],otp[0].maxLength===raw.length?raw:pendingCode);
      else if(otp.length===raw.length)ready=otp.map((e,i)=>sync(e,raw[i])).every(Boolean);
      else{report('manual_required','验证码输入框与邮件验证码格式不一致，请核对官网。');return;}
      if(ready){report('code_filled');clickUnique(action,'otp|'+location.pathname+'|'+pendingCode,otp[0].closest('form')||document);}
      return;
    }
    if(/check your email|sent.*(?:code|email)|验证码.*发送|查看.*邮箱|验证您的邮箱|verify your email/i.test(body)&&!fields.length){report('verification');return;}
    if(/please confirm your age|confirm your age|请确认你的年龄|select the year when you were born|select your date of birth|选择出生年份/i.test(body)){report('manual_required','请按官网要求确认出生年份或年龄；完成后自动继续。');return;}
    if(cfg.app_hosts.includes(location.hostname)&&!fields.length){
      // Open only an ordinary account menu so the visible account identity can
      // be confirmed. Do not infer success from an anonymous chat page.
      if(!hydrated()){report('page','等待官网页面加载完成。');return;}
      const controls=all('button, [role="menuitem"], [role="tab"], [role="button"], a');
      const signedOut=controls.some(e=>!e.disabled&&/^(?:sign in|log in|login|sign up|登录|注册)$/i.test(e.getAttribute('aria-label')||text(e)));
      const avatars=all('button[aria-haspopup="menu"]').filter(e=>!e.disabled&&e.getAttribute('aria-expanded')!=='true'&&!/model|grok|attach|upload|voice|mode|more|更多|模型|附件|语音/i.test(e.getAttribute('aria-label')||text(e))&&(e.querySelector('img[alt="pfp"]')||Array.from(e.querySelectorAll('span[class*="rounded-full"]')).some(s=>!s.querySelector('svg'))));
      if(avatars.length===1&&!submitted.has('identity-avatar')){submitted.add('identity-avatar');report('page','正在确认当前登录邮箱');avatars[0].click();return;}
      for(const [key,pattern] of [['account',/^(?:account|账户|账号)$/i],['settings',/^(?:settings|设置)$/i],['profile',/^(?:open profile menu|profile menu|user menu|account menu|profile|打开个人资料菜单|个人资料菜单|账户菜单|个人资料)$/i]]) {
        if(key==='settings'&&signedOut)continue;
        const matches=controls.filter(e=>!e.disabled&&pattern.test(e.getAttribute('aria-label')||text(e)));
        if(matches.length===1&&!submitted.has('identity-'+key)){submitted.add('identity-'+key);report('page','正在确认当前登录邮箱');matches[0].click();return;}
      }
      report('page','等待官网显示当前登录邮箱，必要时打开账号菜单。');return;
    }
    if(!fields.length){
      if(!hydrated()){report('form');return;}
      const key='email-choice|'+location.pathname;
      const prev=retried.get(key);
      if(submitted.has(key)&&prev&&Date.now()-prev.at>=2500&&prev.count<3)submitted.delete(key);
      if(clickUnique(/^(?:sign up with email|sign in with email|log in with email|login with email|continue with email|use email|使用邮箱注册|使用邮箱登录|使用电子邮件注册|使用电子邮箱注册|使用电子邮件登录|使用电子邮箱登录|用邮箱继续|使用邮箱|注册)$/i,key,document,'form')){
        retried.set(key,{count:(prev?.count||0)+1,at:Date.now()});
      }else report('page');
      return;
    }
    const emails=fields.filter(e=>e.type==='email'||/^(?:email|username)$/.test(e.name)||/邮箱|电子邮件|email address/i.test(meta(e)));
    const passwords=fields.filter(e=>e.type==='password');
    const names=fields.filter(e=>(/first.?name|last.?name|full.?name|given.?name|family.?name|姓名|名字|姓氏/i.test(meta(e))||e.name==='name')&&!emails.includes(e));
    const known=new Set([...emails,...passwords,...names]);
    if(fields.some(e=>!known.has(e))){report('manual_required','官网出现新的资料字段，请按页面提示完成。');return;}
    let ready=true;
    for(const e of emails)ready=sync(e,cfg.email)&&ready;
    for(const e of passwords)ready=sync(e,cfg.password)&&ready;
    for(const e of names){const label=meta(e),value=/last|family|姓氏/i.test(label)?cfg.last_name:/first|given|名字/i.test(label)?cfg.first_name:[cfg.first_name,cfg.last_name].filter(Boolean).join(' ');if(!value){report('manual_required','请先在助手中保存姓名，或在官网填写。');return;}ready=sync(e,value)&&ready;}
    if(!ready){report('form');return;}
    const scope=fields[0]?.closest('form')||document;
    clickUnique(action,'form|'+location.pathname+'|'+fields.map(e=>e.name||e.type||e.id).join(','),scope,emails.length?'code_requested':'submitted');
  }
  window.__grokAssistFillCode=code=>{if(/^(?:[A-Z0-9]{4,8}|[A-Z0-9]{3}-[A-Z0-9]{3})$/i.test(String(code))){pendingCode=String(code);cycle();}};
  window.__grokAssistRetry=()=>{if(challenge()){report('captcha');return;}pendingCode='';submitted=new Set([...submitted].filter(k=>!k.startsWith('otp|')&&k!=='explicit-resend'));for(const e of all('input[autocomplete="one-time-code"]')){managed.delete(e);set(e,'');}if(!clickUnique(/^(?:resend(?: code| email)?|send again|重新发送|重发验证码|重新发送验证码|重新发送电子邮件)$/i,'explicit-resend',document,'code_requested'))report('manual_required','请在官网的邮箱验证码步骤重发。');};
  window.__grokAssistStop=()=>{stopped=true;clearInterval(timer);observer?.disconnect();};
  window.__grokAssistReport=()=>{last='';if(completed)report(completed,'',cfg.email,true);else cycle();};
  timer=setInterval(cycle,700);
  function start(){if(started||stopped)return;started=true;startedAt=Date.now();observer=new MutationObserver(()=>{if(!stopped)finish();});observer.observe(document.body,{subtree:true,childList:true,characterData:true});cycle();}
  if(document.readyState==='loading')window.addEventListener('DOMContentLoaded',start,{once:true});else start();
})();
