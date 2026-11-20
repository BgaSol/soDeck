const $=id=>document.getElementById(id);
export function installConnectionForm(send,writable){
  let dirty=false,snapshot;
  const key=c=>JSON.stringify([c?.mode,c?.endpoint||'',c?.cliPath||'']);
  const config=()=>({mode:$('connection-mode').value,...($('connection-mode').value==='manual'?{endpoint:$('socket').value.trim()}:{}),...($('docker-cli').value.trim()?{cliPath:$('docker-cli').value.trim()}:{})});
  function fields(){const manual=$('connection-mode').value==='manual';$('socket').disabled=!manual||!writable();$('socket-label').hidden=!manual;$('socket').hidden=!manual; }
  function changed(){dirty=true;fields();$('connection-result').textContent='配置已编辑，请测试或保存以验证连接。';}
  $('endpoint-form').addEventListener('input',changed);
  $('endpoint-form').addEventListener('change',changed);
  $('endpoint-form').addEventListener('submit',async e=>{e.preventDefault();await send({action:'connection.save',config:config()});});
  $('connection-test').addEventListener('click',()=>void send({action:'connection.test',config:config()}));
  $('connection-identify').addEventListener('click',()=>void send({action:'connection.identify'}));
  $('connection-reset').addEventListener('click',()=>{dirty=false;render(snapshot);});
  function render(value){
    snapshot=value;if(!value)return;
    const saved=value.connection?.config||{mode:'auto'};
    if(!dirty){$('connection-mode').value=saved.mode;$('socket').value=saved.endpoint||'';$('docker-cli').value=saved.cliPath||'';fields();}
    fields();
    for(const id of ['connection-mode','docker-cli','connection-reset'])$(id).disabled=!writable();
    for(const id of ['connection-test','connection-save','connection-identify'])$(id).disabled=!!value.busy||!writable();
    const test=value.connection?.test;
    if(test&&key(test.config)===key(config())){
      $('connection-result').textContent=({checking:'正在识别并测试连接…',succeeded:'连接成功',failed:'连接失败'}[test.state]||'')+(test.message?'：'+test.message:'');
      if(test.state==='succeeded'&&key(saved)===key(config()))dirty=false;
    }
    const target=value.connection?.target;
    $('connection-target').textContent=target?[target.source,target.endpoint,'Engine '+target.engineId,'容器系统 '+target.os,'远端 CLI '+target.cliPath].join('\n'):'尚未识别活动连接';
  }
  return {render,reset(){dirty=false;}};
}
