'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const os=require('node:os');
const http=require('node:http');
const {spawn}=require('node:child_process');
const binary=path.resolve(process.env.XJCODE_TEST_PACKAGE_DIR||'dist/xjcode-0.1.5-windows-x64-dev','xjcode.exe');
function execute(args,env,cwd,input=''){
 return new Promise((resolve,reject)=>{const child=spawn(binary,args,{cwd,env,windowsHide:true,stdio:['pipe','pipe','pipe']});let out='',err='';
 child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);child.on('error',reject);child.on('close',code=>resolve({code,out,err}));child.stdin.end(input);});
}
function sse(events){return events.map(v=>'event: '+v.type+'\ndata: '+JSON.stringify(v)+'\n\n').join('');}
function done(id){return {type:'response.completed',response:{id,usage:{input_tokens:10,output_tokens:10,total_tokens:20}}};}
for(const wire of ['chat','responses']) test(wire+' API executes a validated patch and receives its result',{timeout:120000},async()=>{
 assert.ok(fs.existsSync(binary),'build the full XJCode package before this test');
 const dir=fs.mkdtempSync(path.join(process.env.XJCODE_TEST_WORK||os.tmpdir(),'xjcode-provider-smoke-'));
 const home=path.join(dir,'home');const project=path.join(dir,'project');fs.mkdirSync(project);
 const requests=[];const errors=[];
 const patch='*** Begin Patch\n*** Add File: api-smoke.txt\n+API_TOOL_OK\n*** End Patch\n';
 const server=http.createServer(async(req,res)=>{
  try{
   let bytes='';for await(const b of req)bytes+=b.toString('utf8');const body=JSON.parse(bytes);requests.push({url:req.url,body});
   assert.equal(req.headers.authorization,'Bearer isolated-mock-token');
   if(requests.length>4){res.writeHead(503);res.end();return;}
   res.writeHead(200,{'Content-Type':'text/event-stream','Connection':'close'});
   if(wire==='chat'){
    assert.equal(req.url,'/v1/chat/completions');const prompt=body.messages[0].content.find(p=>p.type==='text').text;
    const jsonPart=prompt.split('\ncontext=')[1].split('\n输出传输规则：')[0];const context=JSON.parse(jsonPart);
    let reply;
    if(requests.length===1){const tool=context.tools.find(t=>t.name==='apply_patch');assert.ok(tool,'patch tool exposed');reply={type:'tool_calls',calls:[{name:(tool.namespace?tool.namespace+'.':'')+tool.name,arguments:{input:patch}}]};}
    else {assert.ok(JSON.stringify(context.history).includes('api-smoke.txt'));reply={type:'final',text:'API_TOOL_OK'};}
    res.end('data: '+JSON.stringify({choices:[{delta:{content:JSON.stringify(reply)},finish_reason:null}]})+'\n\ndata: '+JSON.stringify({choices:[{delta:{},finish_reason:'stop'}]})+'\n\ndata: [DONE]\n\n');
   }else{
    assert.equal(req.url,'/v1/responses');const id='mock-response-'+requests.length;
    if(requests.length===1){res.end(sse([{type:'response.created',response:{id}},{type:'response.output_item.done',item:{type:'custom_tool_call',call_id:'patch-fixture',name:'apply_patch',input:patch}},done(id)]));}
    else {assert.ok(JSON.stringify(body.input).includes('patch-fixture'));res.end(sse([{type:'response.created',response:{id}},{type:'response.output_item.done',item:{type:'message',id:'msg-fixture',role:'assistant',content:[{type:'output_text',text:'API_TOOL_OK'}]}},done(id)]));}
   }
  }catch(e){errors.push(String(e.stack));if(!res.headersSent)res.writeHead(500);res.end();}
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 try{
  const env={...process.env,XJCODE_HOME:home,XJCODE_SKIP_UPDATE_CHECK:'1'};
  const setup=await execute(['login','--api','--base-url','http://127.0.0.1:'+server.address().port+'/v1','--model','fixture-model','--wire-api',wire,'--key-stdin'],env,project,'isolated-mock-token\n');
  assert.equal(setup.code,0,setup.err);assert.ok(!fs.readFileSync(path.join(home,'custom-api.dpapi')).includes(Buffer.from('isolated-mock-token')));
  const task=await execute(['exec','--skip-git-repo-check','--ephemeral','--dangerously-bypass-approvals-and-sandbox','--json','Create api-smoke.txt with API_TOOL_OK using apply_patch, then report API_TOOL_OK.'],env,project);
  assert.equal(task.code,0,task.err+'\n'+errors.join('\n'));assert.equal(errors.length,0,errors.join('\n'));
  assert.equal(fs.readFileSync(path.join(project,'api-smoke.txt'),'utf8').trim(),'API_TOOL_OK');assert.ok(task.out.includes('API_TOOL_OK'));assert.ok(requests.length>=2);assert.ok(!task.out.includes('isolated-mock-token'));assert.ok(!task.err.includes('isolated-mock-token'));
 }finally{await new Promise(resolve=>server.close(resolve));fs.rmSync(dir,{recursive:true,force:true});}
});
