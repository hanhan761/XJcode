#!/usr/bin/env node
'use strict';
const {spawn}=require('node:child_process');
const path=require('node:path');
const fs=require('node:fs');
const exe=path.join(__dirname,'..','native','xjcode.exe');
if(process.platform!=='win32'||process.arch!=='x64'){console.error('XJCode requires Windows x64.');process.exit(1);}
if(!fs.existsSync(exe)){console.error('Native files missing. Reinstall the complete XJCode npm package.');process.exit(1);}
const child=spawn(exe,process.argv.slice(2),{stdio:'inherit',windowsHide:true});
child.on('error',e=>{console.error(e.message);process.exitCode=1;});
child.on('exit',code=>{process.exitCode=code??1;});
process.on('SIGINT',()=>{});
process.on('SIGTERM',()=>child.kill());
