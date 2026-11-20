import {readFileSync} from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {JSDOM} from 'jsdom';
const source=readFileSync(new URL('../src-tauri/src/pickup.js',import.meta.url),'utf8');
test('observed pickup article is parsed, old Meta email is identified and refresh is bounded',()=>{
 const dom=new JSDOM('<div class="pickup-page-account-identity"><strong>fixture@icloud.com</strong></div><article class="pickup-message"><h1>Fixture verification code</h1><dl><dt>发件人</dt><dd>Meta &lt;security@account.meta.com&gt;</dd><dt>收件人</dt><dd>fixture@icloud.com</dd><dt>邮箱收件时间</dt><dd>2026年10月09日 11:34:28</dd></dl><div class="pickup-message-body"><iframe class="message-html-frame"></iframe></div></article><button>刷新最新邮件</button>',{url:'https://icsms.top/pickup#email=fixture%40icloud.com&key=tok_fixture',runScripts:'outside-only'}),w=dom.window;const messages=[];let tick,clicks=0,now=10000;
 w.HTMLElement.prototype.getClientRects=()=>[{}];w.document.querySelector('iframe').setAttribute('srcdoc','<body>Verification code 012345</body>');w.Date.now=()=>now;w.setInterval=f=>{tick=f;};w.chrome={webview:{postMessage:s=>messages.push(JSON.parse(s))}};w.document.querySelector('button').onclick=()=>clicks++;
 w.eval(source.replace('__PICKUP_CONFIG__',JSON.stringify({id:'fixture',base:'fixture@icloud.com'})));tick();const msg=messages.at(-1).payload.data.message;assert.equal(msg.to,'fixture@icloud.com');assert.equal(msg.from,'Meta <security@account.meta.com>');assert.equal(msg.text,'Verification code 012345');assert.ok(new Date(msg.date).getTime()>0);w.__pickupRefresh();w.__pickupRefresh();assert.equal(clicks,1);now+=5000;w.__pickupRefresh();assert.equal(clicks,2);w.document.querySelector('strong').textContent='other@icloud.com';tick();assert.ok(messages.at(-1).error);w.close();
});
