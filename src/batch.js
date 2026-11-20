export function planAccounts(accounts,selected,credentialsOnly=false){
  const pivot=Math.max(0,accounts.findIndex(a=>a.email===selected));
  const ordered=[...accounts.slice(pivot),...accounts.slice(0,pivot)];
  const seen=new Set();
  return ordered.filter(a=>{
    const key=a.email.toLowerCase();
    if((credentialsOnly?(a.status!=='done'||a.credential_ready):a.status==='done')||seen.has(key))return false;
    seen.add(key);return true;
  });
}

export class RegistrationBatch{
  constructor({startSession,startCredential,onChange=()=>{}}){
    this.startSession=startSession;this.startCredential=startCredential;this.onChange=onChange;
    this.state={active:false,total:0,completed:0,current:'',message:''};
    this.epoch=0;this.queue=[];this.sessionId='';this.inflight=null;this.latest=null;
  }
  async start(accounts,selected,{credentialsOnly=false,saveCredentials=false}={}){
    if(this.state.active)throw new Error('连续注册已经在运行。');
    const plan=planAccounts(accounts,selected,credentialsOnly);
    if(!plan.length)throw new Error(credentialsOnly?'所有已注册邮箱都已保存凭证。':'所有邮箱都已完成注册。');
    if((credentialsOnly||saveCredentials)&&!this.startCredential)throw new Error('未设置凭证保存操作。');
    if(plan.some(a=>!a.password_set))throw new Error('请先给所有待注册邮箱设置密码。');
    this.queue=plan.map(a=>a.email);this.sessionId='';this.latest=null;
    this.credentialsOnly=credentialsOnly;this.saveCredentials=saveCredentials;
    this.state={active:true,total:plan.length,completed:0,current:this.queue[0],message:''};
    await this.launch(++this.epoch);
  }
  async launch(epoch,phase=this.credentialsOnly?'credential':'signup'){
    if(epoch!==this.epoch||!this.state.active)return;
    this.state.current=this.queue[this.state.completed];this.state.message='正在打开下一个邮箱';
    this.phase=phase;
    this.sessionId='';this.latest=null;this.onChange();
    // Set the latch before invoking the API; a stage event can arrive first.
    const operation=Promise.resolve().then(()=>(phase==='credential'?this.startCredential:this.startSession)(this.state.current));
    this.inflight=operation;
    try{
      const data=await operation;
      if(epoch!==this.epoch||!this.state.active)return;
      this.inflight=null;
      await this.update(data||this.latest);
    }catch(error){
      if(epoch===this.epoch)this.halt(String(error));
    }finally{
      if(this.inflight===operation)this.inflight=null;
    }
  }
  async update(data){
    if(!this.state.active||!data)return;
    if(this.inflight){this.latest=data;return;}
    const session=data.session;
    if(!session){
      if(this.sessionId)this.halt('官网窗口已关闭，连续注册已停止。');
      return;
    }
    if(session.email!==this.state.current||session.mode!==this.phase){
      if(this.sessionId)this.halt('当前官网操作发生变化，连续注册已停止。');
      return;
    }
    if(this.sessionId&&session.id!==this.sessionId){this.halt('本次注册已被替换，连续注册已停止。');return;}
    this.sessionId=session.id;
    if(this.phase==='credential'&&session.stage==='error'){this.halt(session.error||'登录凭证保存失败，请重试当前邮箱。');return;}
    if(this.phase==='signup'?['registered','authenticated'].includes(session.stage):session.stage==='credential_saved'){
      const account=data.accounts.find(a=>a.email===this.state.current);
      if(this.phase==='signup'&&account?.status!=='done'){this.state.message='官网已确认成功，等待保存注册记录';this.onChange();return;}
      if(this.phase==='credential'&&!account?.credential_ready){this.state.message='官网登录成功，等待加密保存凭证';this.onChange();return;}
      if(this.phase==='signup'&&this.saveCredentials&&!account?.credential_ready){await this.launch(this.epoch,'credential');return;}
      this.state.completed++;this.sessionId='';
      if(this.state.completed===this.state.total){
        this.state.active=false;this.state.message=this.credentialsOnly?'本轮登录凭证已全部保存':this.saveCredentials?'本轮注册与凭证保存已全部完成':'本轮连续注册已全部完成';this.onChange();return;
      }
      // The next account begins only after both website confirmation and save.
      await this.launch(this.epoch);return;
    }
    this.state.message=session.stage==='captcha'?'等待完成人机验证，完成后自动继续':session.error?session.error:session.stage==='error'?'官网报错，请处理后重发验证码':'自动注册进行中';
    this.onChange();
  }
  halt(message){this.state.active=false;this.state.message=message;this.epoch++;this.onChange();}
  async cancel(){
    const pending=this.inflight;
    this.halt('已停止连续注册');
    if(pending)await pending.catch(()=>{});
  }
}
