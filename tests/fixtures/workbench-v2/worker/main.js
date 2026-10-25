const settings={blocked:null,failed:null};const closes=[];const contexts=[];
globalThis.sodeckPlugin={
  async start(){return {ready:true};},async stop(){},
  async invoke(method,params={}){
    if(method==='fixture.configure'){Object.assign(settings,params);return {settings,closes,contexts};}
    if(method==='fixture.update')return sodeck.call('views.update',params);
    if(method==='lifecycle.prepareClose')return {allow:settings.blocked!==params.viewId};
    if(method==='lifecycle.close'){
      if(settings.failed===params.viewId)throw {code:'FIXTURE_RELEASE_FAILED',message:'隔离夹具模拟释放失败'};
      if(!closes.includes(params.viewId))closes.push(params.viewId);return {closed:true};
    }
    if(method==='lifecycle.initialize')return {holdReady:settings.holdReady===params.viewId};
    if(method==='lifecycle.restore')return {displayStatus:'ready',actions:[]};
    if(method==='lifecycle.cancel')return {};
    if(method==='command.execute'){
      const c=params.context,args=params.args??{};contexts.push(c);
      if(params.commandId==='sample.rename')return sodeck.call('views.update',{viewId:c.viewId,title:args.title,operationId:c.operationId});
      const kind=params.commandId==='sample.new'?'sample.page':params.commandId;
      return sodeck.call('views.open',{viewType:kind,workspaceId:c.workspaceId,instanceKey:args.key??'',title:args.title??kind,displayStatus:'ready',actions:['sample.rename'],operationId:c.operationId});
    }
    throw {code:'METHOD_NOT_FOUND',message:'夹具未声明此操作'};
  }
};
