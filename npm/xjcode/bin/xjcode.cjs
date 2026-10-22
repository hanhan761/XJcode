#!/usr/bin/env node
'use strict';
const {spawn}=require('node:child_process');
const path=require('node:path');
const fs=require('node:fs');
const os=require('node:os');
const exe=path.join(__dirname,'..','native','xjcode.exe');
if(process.platform!=='win32'||process.arch!=='x64'){console.error('XJCode requires Windows x64.');process.exit(1);}
if(!fs.existsSync(exe)){console.error('Native files missing. Reinstall the complete XJCode npm package.');process.exit(1);}
const jobDir=fs.mkdtempSync(path.join(os.tmpdir(),'xjcode-request-'));
const env={...process.env,XJCODE_NPM_PREFIX:path.resolve(__dirname,'../../..'),XJCODE_NODE:process.execPath,XJCODE_PARENT_PID:String(process.pid),XJCODE_JOB_DIR:jobDir};
const child=spawn(exe,process.argv.slice(2),{stdio:'inherit',windowsHide:true,env});
child.on('error',e=>{console.error(e.message);process.exitCode=1;});
child.on('exit',code=>{
 const request=path.join(jobDir,'request.json');
 if(code===0 && fs.existsSync(request)){
  const ps=path.join(process.env.SystemRoot||'C:/Windows','System32/WindowsPowerShell/v1.0/powershell.exe');
  const helper=spawn(ps,['-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',path.join(jobDir,'manage-xjcode.ps1'),'-JobFile',request],{stdio:'inherit',windowsHide:true});
  helper.on('error',e=>{console.error(e.message);process.exitCode=1;});
  helper.on('exit',c=>{process.exitCode=c??1;if(c===0)fs.rmSync(jobDir,{recursive:true,force:true});});
 }else{process.exitCode=code??1;fs.rmSync(jobDir,{recursive:true,force:true});}
});
process.on('SIGINT',()=>{});
process.on('SIGTERM',()=>child.kill());
