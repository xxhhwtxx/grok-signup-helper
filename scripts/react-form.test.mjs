import {readFileSync} from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {JSDOM} from 'jsdom';
import React from 'react';
import {renderToString} from 'react-dom/server';
import {act} from 'react-dom/test-utils';
const source=readFileSync(new URL('../src-tauri/src/driver.js',import.meta.url),'utf8');
const cfg={id:'react-test',email:'fixture@icloud.com',password:'Fixture-only-password',first_name:'Test',last_name:'Person',mode:'signup',consent:true,hosts:['accounts.x.ai'],app_hosts:['grok.com']};
const h=React.createElement;
test('Grok React email/password state survives pre-hydration fill and rerender before submission',async()=>{
 const submitted=[];let force,tick,now=1000;
 function Form(){const[email,setEmail]=React.useState(''),[password,setPassword]=React.useState('');const[,render]=React.useState(0);force=()=>render(n=>n+1);return h('form',{onSubmit:e=>{e.preventDefault();submitted.push({email,password});force();}},h('input',{type:'email',name:'email',value:email,onInput:e=>setEmail(e.target.value)}),h('input',{type:'password',name:'password',value:password,onInput:e=>setPassword(e.target.value)}),h('button',null,'Create Account'));}
 const dom=new JSDOM('<div id="root">'+renderToString(h(Form))+'</div>',{url:'https://accounts.x.ai/sign-up',runScripts:'outside-only'}),w=dom.window;
 const old={window:globalThis.window,document:globalThis.document,navigator:globalThis.navigator,act:globalThis.IS_REACT_ACT_ENVIRONMENT};
 globalThis.window=w;globalThis.document=w.document;Object.defineProperty(globalThis,'navigator',{configurable:true,value:w.navigator});globalThis.IS_REACT_ACT_ENVIRONMENT=true;
 Object.defineProperty(w.document,'readyState',{configurable:true,value:'complete'});Object.defineProperty(w.HTMLElement.prototype,'innerText',{get(){return this.textContent;}});w.HTMLElement.prototype.getClientRects=()=>[{}];w.Date.now=()=>now;w.setInterval=f=>{tick=f;return 1;};w.clearInterval=()=>{};
 w.chrome={webview:{postMessage(){}}};w.eval(source.replace('__GROK_CONFIG__',JSON.stringify(cfg)));
 let root;try{const{hydrateRoot}=await import('react-dom/client');await act(async()=>{root=hydrateRoot(w.document.getElementById('root'),h(Form));});for(let i=0;i<5;i++){now+=800;await act(async()=>tick());}assert.deepEqual(submitted,[{email:cfg.email,password:cfg.password}]);await act(async()=>force());assert.equal(w.document.querySelector('[name=email]').value,cfg.email);assert.equal(w.document.querySelector('[name=password]').value,cfg.password);}
 finally{w.__grokAssistStop();if(root)await act(async()=>root.unmount());w.close();globalThis.window=old.window;globalThis.document=old.document;Object.defineProperty(globalThis,'navigator',{configurable:true,value:old.navigator});globalThis.IS_REACT_ACT_ENVIRONMENT=old.act;}
});
