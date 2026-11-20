import test from 'node:test';
import assert from 'node:assert/strict';
import {RegistrationBatch,planAccounts} from '../src/batch.js';
const accounts=[
  {email:'first@example.test',status:'new',password_set:true},
  {email:'done@example.test',status:'done',password_set:true},
  {email:'last@example.test',status:'new',password_set:true},
];
const session=(email,id,stage='verification',error='')=>({email,id,stage,error,mode:'signup'});
function fixture(){
  const started=[];
  const b=new RegistrationBatch({startSession:async email=>{started.push(email);return{accounts,session:session(email,String(started.length),'opening')};}});
  return{b,started};
}
test('queue begins at selected email and skips registered and duplicate accounts',()=>{
  assert.deepEqual(planAccounts([...accounts,{...accounts[0]}],accounts[2].email).map(a=>a.email),[accounts[2].email,accounts[0].email]);
});
test('next email starts only after confirmed success and persisted done status',async()=>{
  const {b,started}=fixture();await b.start(accounts,accounts[0].email);
  await b.update({accounts,session:session(accounts[0].email,'1','submitted')});assert.equal(started.length,1);
  await b.update({accounts,session:session(accounts[0].email,'1','registered')});assert.equal(started.length,1);
  const saved=accounts.map(a=>a.email===accounts[0].email?{...a,status:'done'}:a);
  await Promise.all([b.update({accounts:saved,session:session(accounts[0].email,'1','registered')}),b.update({accounts:saved,session:session(accounts[0].email,'1','registered')})]);
  assert.deepEqual(started,[accounts[0].email,accounts[2].email]);
  await b.update({accounts:accounts.map(a=>({...a,status:'done'})),session:session(accounts[2].email,'2','authenticated')});
  assert.equal(b.state.active,false);assert.equal(b.state.completed,2);
});
test('CAPTCHA, network errors and timeouts hold current email without skipping',async()=>{
  const {b,started}=fixture();await b.start(accounts,accounts[0].email);
  for(const stage of ['captcha','error','verification'])await b.update({accounts,session:session(accounts[0].email,'1',stage,stage==='verification'?'验证码等待超时':'')});
  assert.equal(started.length,1);assert.equal(b.state.completed,0);assert.equal(b.state.active,true);assert.equal(b.state.message,'验证码等待超时');
});
test('closing the current website window halts the queue',async()=>{const {b,started}=fixture();await b.start(accounts,accounts[0].email);await b.update({accounts,session:null});assert.equal(b.state.active,false);assert.equal(started.length,1);});
test('cancelling while opening prevents any subsequent email',async()=>{
  let resolveStart;const started=[];
  const b=new RegistrationBatch({startSession:email=>{started.push(email);return new Promise(resolve=>resolveStart=resolve);}});
  const start=b.start(accounts,accounts[0].email);await Promise.resolve();
  const cancel=b.cancel();resolveStart({accounts,session:session(accounts[0].email,'1','registered')});await Promise.all([start,cancel]);
  await b.update({accounts:accounts.map(a=>({...a,status:'done'})),session:session(accounts[0].email,'1','registered')});
  assert.equal(b.state.active,false);assert.equal(started.length,1);
});
test('start failure stops and missing passwords fail before opening a window',async()=>{
  const b=new RegistrationBatch({startSession:async()=>{throw new Error('窗口无法打开');}});await b.start(accounts,accounts[0].email);assert.equal(b.state.active,false);assert.match(b.state.message,/窗口无法打开/);
  await assert.rejects(()=>b.start([{...accounts[0],password_set:false}],accounts[0].email),/设置密码/);
});
test('registration waits for encrypted credential save before starting next email',async()=>{
 const started=[],saved=accounts.map(a=>({...a,status:'done',credential_ready:false}));
 const b=new RegistrationBatch({startSession:async email=>{started.push('signup:'+email);return{accounts,session:session(email,'signup','opening')};},startCredential:async email=>{started.push('credential:'+email);return{accounts:saved,session:{email,id:'credential',mode:'credential',stage:'opening'}};}});
 await b.start(accounts,accounts[0].email,{saveCredentials:true});
 await b.update({accounts:saved,session:session(accounts[0].email,'signup','registered')});
 assert.deepEqual(started,['signup:'+accounts[0].email,'credential:'+accounts[0].email]);
 assert.equal(b.state.completed,0);
 await b.update({accounts:saved,session:{email:accounts[0].email,id:'credential',mode:'credential',stage:'credential_saved'}});
 assert.equal(started.length,2);
 saved[0].credential_ready=true;
 await b.update({accounts:saved,session:{email:accounts[0].email,id:'credential',mode:'credential',stage:'credential_saved'}});
 assert.equal(b.state.completed,1);assert.equal(started[2],'signup:'+accounts[2].email);
});
test('credential-only queue skips new emails and already saved credentials',async()=>{
 const input=accounts.map(a=>({...a,status:'done',credential_ready:a.email===accounts[1].email}));input.push({email:'new@example.test',status:'new',password_set:true});
 assert.deepEqual(planAccounts(input,accounts[2].email,true).map(a=>a.email),[accounts[2].email,accounts[0].email]);
 const started=[];
 const b=new RegistrationBatch({startSession:async()=>assert.fail('must not register'),startCredential:async email=>{started.push(email);return{accounts:input,session:{email,id:String(started.length),mode:'credential',stage:'opening'}};}});
 await b.start(input,accounts[2].email,{credentialsOnly:true});input[2].credential_ready=true;
 await b.update({accounts:input,session:{email:accounts[2].email,id:'1',mode:'credential',stage:'credential_saved'}});
 assert.deepEqual(started,[accounts[2].email,accounts[0].email]);
 await b.update({accounts:input,session:{email:accounts[0].email,id:'2',mode:'credential',stage:'error',error:'账号不匹配'}});
 assert.equal(b.state.active,false);assert.equal(b.state.completed,1);assert.equal(b.state.message,'账号不匹配');
});
test('cancel during credential opening does not start another account',async()=>{
 let resolve;const started=[],input=accounts.map(a=>({...a,status:'done'}));
 const b=new RegistrationBatch({startCredential:email=>{started.push(email);return new Promise(r=>resolve=r);}});
 const opening=b.start(input,input[0].email,{credentialsOnly:true});await Promise.resolve();
 const stopping=b.cancel();resolve({accounts:input,session:{email:input[0].email,id:'1',mode:'credential',stage:'credential_saved'}});await Promise.all([opening,stopping]);
 assert.equal(started.length,1);assert.equal(b.state.active,false);
});
