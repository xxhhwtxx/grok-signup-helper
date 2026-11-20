import {invoke,listen,preview} from './bridge.js';
import {RegistrationBatch,planAccounts} from './batch.js';
import './style.css';
const $=s=>document.querySelector(s);
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let profileLoaded=false;
let data={accounts:[],session:null},selected='',manualMail=null,fetching=false,starting=false,noticeTimer,refreshId=0;
const batch=new RegistrationBatch({startSession:async email=>{
 selected=email;manualMail=null;starting=true;render();
 try{await invoke('start_session',{email,mode:'signup',consent:true});return await refresh();}finally{starting=false;render();}
},onChange:()=>{if(batch.state.active)selected=batch.state.current;render();}});
const labels={new:'待注册',waiting:'进行中',done:'已注册',failed:'需重试'};
const stages={opening:'正在打开 Grok 官网',form:'自动填写邮箱和密码',code_requested:'已点击发送验证码，等待官网响应',verification:'自动读取验证邮件，最多等待 10 分钟',captcha:'请在官网窗口完成人机验证，完成后自动继续',code_filled:'验证码已自动填写',submitted:'已提交，等待官网确认注册或登录结果',registered:'官网显示注册成功',authenticated:'官网显示所选邮箱已登录',consent_required:'请先同意服务条款',page:'查看官网当前提示',access_blocked:'官网限制当前访问，请处理页面提示',manual_required:'请在官网完成必需的资料或验证，完成后自动继续',error:'官网流程需要重试'};
$('#app').innerHTML=`
<header><div class="brand"><span class="brand-icon">G</span><div><strong>Grok 注册助手</strong><span>连续注册 · 自动验证码 · 保存登录状态</span></div></div><span class="version">v0.1.0</span></header>
<div class="shell"><aside><div class="aside-heading"><h2>邮箱库</h2><span class="count" id="count"></span></div><div class="import-buttons"><button class="primary" id="paste-import">粘贴导入</button><button id="file-import">导入 TXT</button></div><input class="search" id="search" placeholder="搜索邮箱" aria-label="搜索邮箱"><div class="accounts" id="accounts"></div><div class="aside-bottom"><button class="quiet" id="export">导出备份</button></div></aside>
<main><section class="account-heading"><span class="eyebrow">当前邮箱</span><h1 id="current-email"></h1><div class="credentials"><button id="copy-email">复制邮箱</button><button id="copy-password">复制密码</button><span id="password-hint"></span></div></section>
<section class="panel"><div class="section-heading"><h2>账号资料与登录状态</h2><span class="step">保存</span></div><p class="muted">每个邮箱使用独立浏览器资料，登录状态自动保存在本机。账号资料可复制或导出备份。</p><div class="row-actions"><button id="copy-record">复制当前账号资料</button><button id="copy-records">复制全部已注册资料</button></div><div class="row-actions"><label>名<input id="first-name" autocomplete="given-name" maxlength="100"></label><label>姓<input id="last-name" autocomplete="family-name" maxlength="100"></label><button id="save-profile">保存注册姓名</button></div><p class="muted">官网要求年龄、生日或手机验证时，请亲自填写真实资料。</p></section>
<section class="panel"><div class="section-heading"><h2>自动注册 Grok 账号</h2><span class="step">01</span></div><p class="muted">自动填写邮箱和密码、发送并读取验证码、提交注册。连续注册会在当前邮箱成功后自动处理下一个。</p><label class="consent"><input type="checkbox" id="consent">我已阅读并同意 Grok <button type="button" id="terms-service" class="text-link">服务条款</button>和<button type="button" id="terms-privacy" class="text-link">隐私政策</button></label><div class="row-actions"><button class="primary" id="batch-start">连续注册</button><button id="start">注册当前邮箱</button><button id="login">自动登录</button><button id="stop" hidden>结束本次操作</button></div><p id="batch-progress" class="stage" hidden></p><p id="stage" class="stage"></p><p id="error" class="callout" hidden></p><p class="muted">已注册邮箱自动跳过。出现报错时停在当前邮箱；若官网要求人机验证，完成后自动继续。</p></section>
<section class="panel mail-panel"><div class="section-heading"><h2>邮箱验证码</h2><span class="step">02</span></div><div class="mail-toolbar"><span class="muted" id="poll-status">发送验证码后，每 5 秒检查邮件</span><button id="fetch">立即取信</button><button id="retry" hidden>重发验证码</button></div><div id="mail"></div></section>
<section class="panel"><div class="section-heading"><h2>打开 Grok</h2><span class="step">03</span></div><p class="muted">使用所选邮箱的独立浏览器资料打开 grok.com，可保留各账号的登录状态。</p><div class="row-actions"><button class="primary" id="work">打开此账号的 Grok</button></div><details><summary>修正注册记录</summary><p class="muted">只有在官网明确显示注册成功时，才手动确认。提交验证码不会自动算作成功。</p><div class="row-actions"><button id="mark-done">官网已确认注册成功</button><button id="mark-failed">标记需重试</button></div></details></section>

</main></div><div id="notice" class="notice" role="status" hidden></div>
<dialog id="import-dialog"><form method="dialog" class="dialog-heading"><h2>粘贴邮箱列表</h2><button class="close" aria-label="关闭">×</button></form><p class="muted">支持 iCloud 取信链接（“邮箱----取信链接”“邮箱----密码----取信链接”“邮箱---icsms取信链接”“邮箱---tok_密钥---icsms取信链接”），以及 Outlook/Microsoft 365 四段格式“邮箱----邮箱密码----client_id----refresh_token”。</p><label>统一注册密码<input type="password" id="import-password" placeholder="留空使用已保存的密码" autocomplete="new-password"></label><label>邮箱列表<textarea id="import-text" rows="9" spellcheck="false" placeholder="demo+tag@icloud.com----https://mokeyun.icu/share/v1/latest?email=demo%40icloud.com&amp;token=你的token&#10;alex@outlook.com----邮箱密码----9e5f94bc-e8a4-4e73-b8be-63364c29d753----M.C5xx_BAY.0.U.MsaArtifacts.-..."></textarea></label><div id="import-result" class="import-result" hidden></div><div class="dialog-actions"><button class="primary" id="import-submit">导入</button><button id="import-close">关闭</button></div></dialog>`;
function current(){return data.accounts.find(a=>a.email===selected);}
function active(){return data.session?.email===selected?data.session:null;}
function notice(message,error=false){$('#notice').textContent=message;$('#notice').className=error?'notice error':'notice';$('#notice').hidden=false;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>$('#notice').hidden=true,7000);}
async function action(fn){try{await fn();}catch(e){notice(String(e),true);}}
async function refresh(){const id=++refreshId,next=await invoke('get_state');if(id!==refreshId)return data;data=next;if(!profileLoaded){$('#first-name').value=data.first_name||'';$('#last-name').value=data.last_name||'';profileLoaded=true;}if(!selected&&data.accounts.length)selected=data.accounts[0].email;render();void batch.update(data).catch(e=>batch.halt(String(e)));
 return data;}
function render(){
 const a=current(),s=active(),busy=!!data.session&&!['registered','authenticated','credential_saved'].includes(data.session.stage),b=batch.state,pending=planAccounts(data.accounts,selected).length;
 $('#count').textContent=data.accounts.length;
 const rows=data.accounts.filter(a=>a.email.toLowerCase().includes($('#search').value.toLowerCase()));
 $('#accounts').innerHTML=rows.length?rows.map(a=>`<button class="account ${a.email===selected?'selected':''}" data-email="${esc(a.email)}"><span class="account-email">${esc(a.email)}</span><span class="account-meta"><span class="status ${esc(a.status)}">${labels[a.status]||'待注册'}</span><span>${esc(a.updated_at)}</span></span></button>`).join(''):'<div class="list-empty">先粘贴或导入邮箱列表</div>';
 $('#current-email').textContent=a?.email||'选择一个邮箱开始';$('#password-hint').textContent=a?.password_set?'已保存注册密码':'';
 $('#start').textContent=starting?'正在打开官网…':a?.status==='done'?'自动登录此邮箱':'注册当前邮箱';
 for(const id of ['start','login','work'])$('#'+id).disabled=!a||busy||starting||b.active||(id==='start'&&a?.status!=='done'&&!$('#consent').checked);
 $('#batch-start').textContent=b.active?'连续注册进行中…':`连续注册（${pending} 个邮箱）`;
 $('#batch-start').disabled=!pending||busy||starting||b.active||!$('#consent').checked;
 $('#batch-progress').hidden=!b.total;$('#batch-progress').textContent=b.total?`连续注册：已完成 ${b.completed}/${b.total}${b.active?` · 当前第 ${b.completed+1} 个`:''} · ${b.message}`:'';
 $('#consent').disabled=b.active;for(const id of ['paste-import','file-import'])$('#'+id).disabled=b.active;
 $('#stop').hidden=!data.session&&!b.active;$('#stop').textContent=b.active?'停止连续操作':'结束本次操作';$('#stage').textContent=s?stages[s.stage]||'进行中':data.session?'另一个邮箱正在处理':'';
 $('#error').hidden=!s?.error;$('#error').textContent=s?.error||'';
 $('#retry').hidden=!s||s.mode!=='signup'||['registered','authenticated'].includes(s.stage);
 for(const id of ['fetch','copy-email','copy-password','export','mark-done','mark-failed'])$('#'+id).disabled=!a||(id==='fetch'&&fetching);
 $('#poll-status').textContent=fetching?'正在取信…':s?.polling?'每 5 秒检查新邮件':s?.mail?.eligible?'已收到本次流程的验证邮件':'发送验证码后，每 5 秒检查邮件';
 const m=s?.mail||manualMail;
 $('#mail').innerHTML=m?`<div class="mail-reason ${m.eligible?'ready':''}">${esc(m.reason)}</div>${m.codes.length?`<div class="codes">${m.codes.map(code=>`<div class="code-card"><strong>${esc(code)}</strong><div><button data-copy-code="${esc(code)}">复制</button><button data-fill-code="${esc(code)}" ${s&&m.eligible?'':'disabled'}>填入官网</button></div></div>`).join('')}</div>`:''}<details class="mail-detail"><summary>查看邮件正文<span>${esc(m.received_at?new Date(m.received_at).toLocaleString():'时间未知')}</span></summary><h3>${esc(m.subject)}</h3><p class="muted">发件人：${esc(m.from)}</p><pre>${esc(m.text)}</pre></details>`:'<div class="mail-empty"><span class="mail-glyph">✉</span><p>等待 Grok 验证邮件</p><small>本次新验证码会自动填入。其他平台邮件及旧邮件只展示供核对。</small></div>';
}
async function copy(text){try{await navigator.clipboard.writeText(text);}catch{const e=document.createElement('textarea');e.value=text;document.body.append(e);e.select();const ok=document.execCommand('copy');e.remove();if(!ok)throw new Error('复制失败。');}notice('已复制');}
async function start(mode){starting=true;render();try{await invoke('start_session',{email:selected,mode,consent:$('#consent').checked});}finally{starting=false;await refresh();}}
$('#accounts').addEventListener('click',e=>{if(batch.state.active)return;const b=e.target.closest('[data-email]');if(b){selected=b.dataset.email;manualMail=null;render();}});
$('#save-profile').onclick=()=>action(async()=>{await invoke('save_profile',{firstName:$('#first-name').value,lastName:$('#last-name').value});notice('姓名已保存，下次打开注册窗口时生效。');});
$('#copy-record').onclick=()=>action(async()=>copy(await invoke('account_text',{email:selected})));
$('#copy-records').onclick=()=>action(async()=>copy(await invoke('account_text',{email:null})));
$('#search').addEventListener('input',render);$('#consent').addEventListener('change',render);
$('#start').onclick=()=>action(()=>start(current()?.status==='done'?'login':'signup'));$('#login').onclick=()=>action(()=>start('login'));$('#work').onclick=()=>action(()=>start('work'));
$('#batch-start').onclick=()=>action(async()=>{if(!$('#consent').checked)throw new Error('请先阅读并勾选 Grok 服务条款与隐私政策。');await batch.start(data.accounts,selected);});
$('#stop').onclick=()=>action(async()=>{const pending=batch.cancel();await invoke('stop_session');await pending;await invoke('stop_session');await refresh();});
$('#retry').onclick=()=>action(async()=>{await invoke('retry_code');await refresh();});
$('#fetch').onclick=()=>action(async()=>{fetching=true;render();try{manualMail=await invoke('fetch_mail',{email:selected});await refresh();}finally{fetching=false;render();}});
$('#copy-email').onclick=()=>action(()=>copy(selected));$('#copy-password').onclick=()=>action(async()=>copy(await invoke('credential',{email:selected})));
for(const kind of ['service','privacy'])$('#terms-'+kind).onclick=()=>action(()=>invoke('open_terms',{kind}));
for(const done of [true,false])$('#mark-'+(done?'done':'failed')).onclick=()=>action(async()=>{await invoke('mark_result',{email:selected,done});await refresh();});
$('#mail').addEventListener('click',e=>action(async()=>{const c=e.target.closest('[data-copy-code]'),f=e.target.closest('[data-fill-code]');if(c)await copy(c.dataset.copyCode);if(f&&!f.disabled)await invoke('fill_code',{code:f.dataset.fillCode});}));
$('#paste-import').onclick=()=>$('#import-dialog').showModal();$('#import-close').onclick=()=>$('#import-dialog').close();
function reportImport(r){$('#import-result').hidden=false;$('#import-result').textContent=`新增 ${r.added}，更新 ${r.updated}，重复 ${r.skipped}${r.errors.length?'\n'+r.errors.join('\n'):''}`;}
$('#import-submit').onclick=()=>action(async()=>{reportImport(await invoke('import_text',{text:$('#import-text').value,password:$('#import-password').value||null}));$('#import-password').value='';await refresh();});
$('#file-import').onclick=()=>action(async()=>{const r=await invoke('import_file',{password:null});if(r){reportImport(r);$('#import-dialog').showModal();await refresh();}});
$('#export').onclick=()=>action(async()=>{if(await invoke('export_accounts'))notice('备份已导出，包含邮箱和密码，请只自己保存。');});
if(preview)$('.version').textContent='界面预览 · 模拟数据';
action(async()=>{await listen('grok://changed',()=>action(refresh));await refresh();});
